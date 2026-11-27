//! Agent updates: an uploaded build is offered to older agents in the
//! rollout ring, downloads intact, and is withheld when updates are off or
//! the endpoint isn't in a pilot OU.

use od_client::{Client, ServerTarget};
use serde_json::{json, Value};

#[tokio::test]
async fn agent_updates_follow_the_rollout_ring() {
    let dir = std::env::temp_dir().join(format!("od-upd-{}", uuid::Uuid::new_v4()));
    let server = od_server::start(od_server::ServerConfig {
        data_dir: dir,
        listen: "127.0.0.1:0".parse().unwrap(),
        tls: false,
        local_listen: None,
        public_host: Some("127.0.0.1".into()),
        web_ui: None,
        tls_cert: None,
        web_assets: None,
    })
    .await
    .unwrap();
    let target = ServerTarget { url: format!("http://127.0.0.1:{}", server.listen_addr.port()), fingerprint: None };
    let anon = Client::new(&target).unwrap();
    let v: Value = anon
        .post("/setup", &json!({"organization":"T","domain":"corp.test","admin_username":"admin1","admin_display_name":"A","admin_password":"a-long-password-1","baseline":"custom"}))
        .await
        .unwrap();
    let token = v["token"].as_str().unwrap().to_string();
    let admin = Client::new(&target).unwrap().with_session(&token);
    let tok: Value = admin.post("/enrollment-tokens", &json!({})).await.unwrap();
    let inv = json!({"hostname":"pc-1","os_name":"x","os_version":"1","os_edition":"","kernel_version":"","arch":"x86_64","manufacturer":"","model":"","serial_number":"","cpu_model":"","cpu_cores":1,"memory_bytes":1,"uptime_secs":1,"disks":[],"interfaces":[],"logged_in_users":[],"disk_encrypted":null,"firewall_enabled":null});
    let enr: Value =
        anon.post("/agent/enroll", &json!({"token":tok["token"],"os":"linux","agent_version":"0.1.0","inventory":inv})).await.unwrap();
    let agent = Client::new(&target).unwrap().with_endpoint(enr["endpoint_id"].as_str().unwrap(), enr["endpoint_secret"].as_str().unwrap());
    let checkin = |v: &str| json!({"agent_version": v, "inventory": null, "compliance": [], "platform": "linux-x64"});

    // Upload a build (as an admin would on an offline network).
    let build: Vec<u8> = (0..300_000u32).map(|i| (i % 251) as u8).collect();
    let resp = reqwest::Client::new()
        .post(format!("{}/api/v1/agent-updates/upload/linux-x64", target.url))
        .bearer_auth(&token)
        .body(build.clone())
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 201);

    // Default ring is "all": an older agent is offered the build.
    let r: Value = agent.post("/agent/checkin", &checkin("0.1.0")).await.unwrap();
    assert_eq!(r["update"]["size"], build.len(), "{r}");
    let bytes = agent.get_bytes("/agent/update/linux-x64").await.unwrap();
    assert_eq!(bytes, build);
    assert_eq!(r["update"]["sha256"].as_str().unwrap(), sha256(&bytes));

    // An up-to-date agent isn't.
    let r: Value = agent.post("/agent/checkin", &checkin(od_server::VERSION)).await.unwrap();
    assert!(r["update"].is_null());

    // Pilot ring: only endpoints in the pilot OU.
    let ou: Value = admin.post("/ous", &json!({"name":"Pilot"})).await.unwrap();
    let _: Value = admin.put("/agent-updates", &json!({"mode":"pilot","pilot_ou_ids":[ou["id"]]})).await.unwrap();
    let r: Value = agent.post("/agent/checkin", &checkin("0.1.0")).await.unwrap();
    assert!(r["update"].is_null(), "not in the pilot OU");

    // Off.
    let _: Value = admin.put("/agent-updates", &json!({"mode":"off"})).await.unwrap();
    let r: Value = agent.post("/agent/checkin", &checkin("0.1.0")).await.unwrap();
    assert!(r["update"].is_null());

    let st: Value = admin.get("/agent-updates").await.unwrap();
    assert_eq!(st["outdated"], 1, "{st}");
    assert!(st["builds"].to_string().contains("\"cached\":true"));
}

fn sha256(b: &[u8]) -> String {
    use sha2::Digest;
    hex::encode(sha2::Sha256::digest(b))
}
