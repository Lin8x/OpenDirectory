//! A real Management Server answering RADIUS PAP requests from a known client.

use md5::{Digest, Md5};
use od_client::{Client, ServerTarget};
use serde_json::{json, Value};

fn access_request(id: u8, user: &str, password: &str, secret: &[u8]) -> (Vec<u8>, [u8; 16]) {
    let ra: [u8; 16] = rand::random();
    let mut padded = password.as_bytes().to_vec();
    padded.resize(padded.len().div_ceil(16) * 16, 0);
    let mut enc = vec![];
    let mut prev = ra.to_vec();
    for chunk in padded.chunks(16) {
        let b = Md5::new().chain_update(secret).chain_update(&prev).finalize();
        let c: Vec<u8> = chunk.iter().zip(b.iter()).map(|(p, k)| p ^ k).collect();
        prev = c.clone();
        enc.extend(c);
    }
    let mut p = vec![1, id, 0, 0];
    p.extend(ra);
    for (t, v) in [(1u8, user.as_bytes().to_vec()), (2u8, enc)] {
        p.push(t);
        p.push((v.len() + 2) as u8);
        p.extend(v);
    }
    let len = p.len() as u16;
    p[2..4].copy_from_slice(&len.to_be_bytes());
    (p, ra)
}

#[tokio::test]
async fn radius_pap_accepts_group_members_only() {
    let dir = std::env::temp_dir().join(format!("od-radius-{}", uuid::Uuid::new_v4()));
    let server = od_server::start(od_server::ServerConfig {
        data_dir: dir.clone(),
        listen: "127.0.0.1:0".parse().unwrap(),
        tls: false,
        local_listen: None,
        public_host: Some("127.0.0.1".into()),
        web_ui: None,
        tls_cert: None,
    })
    .await
    .unwrap();
    let target = ServerTarget { url: format!("http://127.0.0.1:{}", server.listen_addr.port()), fingerprint: None };
    let anon = Client::new(&target).unwrap();
    let v: Value = anon
        .post("/setup", &json!({"organization":"T","domain":"corp.test","admin_username":"admin1","admin_display_name":"A","admin_password":"a-long-password-1","baseline":"custom"}))
        .await
        .unwrap();
    let admin = Client::new(&target).unwrap().with_session(v["token"].as_str().unwrap());
    let u: Value =
        admin.post("/users", &json!({"username":"vpnuser","display_name":"VPN User","password":"vpn-user-password-1"})).await.unwrap();
    let _: Value = admin.post("/users", &json!({"username":"other","display_name":"Other","password":"other-password-12"})).await.unwrap();
    let _: Value = admin.post("/groups", &json!({"name":"VPN Users","member_ids":[u["id"]]})).await.unwrap();

    let port = std::net::UdpSocket::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let secret = "a-shared-secret-123";
    let st = admin
        .request(
            od_client::Method::PUT,
            "/radius",
            Some(&json!({"enabled":true,"listen":format!("127.0.0.1:{port}"),"clients":[{"name":"VPN gateway","address":"127.0.0.1","secret":secret,"required_group":"VPN Users"}]})),
        )
        .await
        .unwrap();
    assert_eq!(st["running"], true, "{st}");

    let sock = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let mut ask = async |id: u8, user: &str, pw: &str| {
        let (p, ra) = access_request(id, user, pw, secret.as_bytes());
        sock.send_to(&p, ("127.0.0.1", port)).await.unwrap();
        let mut buf = [0u8; 1024];
        let (n, _) = tokio::time::timeout(std::time::Duration::from_secs(5), sock.recv_from(&mut buf)).await.unwrap().unwrap();
        let r = buf[..n].to_vec();
        // Check the Response Authenticator.
        let mut check = r.clone();
        check[4..20].copy_from_slice(&ra);
        let want = Md5::new().chain_update(&check).chain_update(secret.as_bytes()).finalize();
        assert_eq!(&r[4..20], want.as_slice(), "response authenticator");
        r[0]
    };
    assert_eq!(ask(1, "vpnuser", "vpn-user-password-1").await, 2, "Access-Accept");
    assert_eq!(ask(2, "CORP\\vpnuser", "wrong-password").await, 3, "Access-Reject");
    assert_eq!(ask(3, "other", "other-password-12").await, 3, "not in the required group");
    server.shutdown();
    let _ = std::fs::remove_dir_all(dir);
}
