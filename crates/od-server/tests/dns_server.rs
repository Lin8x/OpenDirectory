//! Runs a real Management Server with its DNS server on and queries it over UDP.

use od_client::{Client, ServerTarget};
use serde_json::{json, Value};

fn query(name: &str, qtype: u16) -> Vec<u8> {
    let mut m = vec![0xab, 0xcd, 0x01, 0x00, 0, 1, 0, 0, 0, 0, 0, 0];
    for l in name.split('.') {
        m.push(l.len() as u8);
        m.extend(l.as_bytes());
    }
    m.push(0);
    m.extend(qtype.to_be_bytes());
    m.extend(1u16.to_be_bytes());
    m
}

#[tokio::test]
async fn serves_zone_records_and_endpoints() {
    let dir = std::env::temp_dir().join(format!("od-dns-{}", uuid::Uuid::new_v4()));
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

    // An endpoint reports its address; it gets a dynamic A record.
    let tok: Value = admin.post("/enrollment-tokens", &json!({})).await.unwrap();
    let inv = json!({"hostname":"PC-07","os_name":"","os_version":"","os_edition":"","kernel_version":"","arch":"","manufacturer":"","model":"","serial_number":"","cpu_model":"","cpu_cores":1,"memory_bytes":1,"uptime_secs":1,"disks":[],"interfaces":[{"name":"eth0","mac":"","ips":["10.1.2.7"]}],"logged_in_users":[],"disk_encrypted":null,"firewall_enabled":null});
    let _: Value =
        anon.post("/agent/enroll", &json!({"token":tok["token"],"os":"windows","agent_version":"0.5.0","inventory":inv})).await.unwrap();

    let port = std::net::UdpSocket::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let _: Value = admin
        .request(
            od_client::Method::PUT,
            "/dns/records",
            Some(&json!([{"id":"00000000-0000-0000-0000-000000000000","name":"files","type":"A","value":"10.1.2.50"}])),
        )
        .await
        .unwrap();
    let st = admin
        .request(
            od_client::Method::PUT,
            "/dns/config",
            Some(&json!({"enabled":true,"listen":format!("127.0.0.1:{port}"),"forwarders":["127.0.0.1"]})),
        )
        .await
        .unwrap();
    assert_eq!(st["running"], true, "{st}");

    let sock = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let ask = |name: &str, t: u16| {
        let q = query(name, t);
        let sock = &sock;
        async move {
            sock.send_to(&q, ("127.0.0.1", port)).await.unwrap();
            let mut buf = [0u8; 1500];
            let (n, _) = tokio::time::timeout(std::time::Duration::from_secs(5), sock.recv_from(&mut buf)).await.unwrap().unwrap();
            buf[..n].to_vec()
        }
    };
    let r = ask("files.corp.test", 1).await;
    assert_eq!(u16::from_be_bytes([r[6], r[7]]), 1, "one answer");
    assert_eq!(&r[r.len() - 4..], &[10, 1, 2, 50]);
    let r = ask("pc-07.corp.test", 1).await;
    assert_eq!(&r[r.len() - 4..], &[10, 1, 2, 7], "dynamic record for the endpoint");
    let r = ask("od.corp.test", 1).await;
    assert_eq!(&r[r.len() - 4..], &[127, 0, 0, 1], "the Management Server itself");
    let r = ask("missing.corp.test", 1).await;
    assert_eq!(r[3] & 0x0f, 3, "NXDOMAIN");
    server.shutdown();
    let _ = std::fs::remove_dir_all(dir);
}
