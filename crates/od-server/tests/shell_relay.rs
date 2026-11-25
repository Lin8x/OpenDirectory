//! Runs a real Management Server and relays a remote shell session between an
//! "admin" WebSocket and an "agent" WebSocket.

use futures_util::{SinkExt, StreamExt};
use od_client::{Client, ServerTarget, WsMessage};
use serde_json::{json, Value};

#[tokio::test]
async fn shell_session_relays_both_ways() {
    let dir = std::env::temp_dir().join(format!("od-shell-{}", uuid::Uuid::new_v4()));
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
    let url = format!("http://127.0.0.1:{}", server.listen_addr.port());
    let target = ServerTarget { url: url.clone(), fingerprint: None };
    let anon = Client::new(&target).unwrap();

    let v: Value = anon
        .post("/setup", &json!({"organization":"T","domain":"t.example.com","admin_username":"admin1","admin_display_name":"A","admin_password":"a-long-password-1"}))
        .await
        .unwrap();
    let admin = Client::new(&target).unwrap().with_session(v["token"].as_str().unwrap());
    let tok: Value = admin.post("/enrollment-tokens", &json!({})).await.unwrap();
    let inv = json!({"hostname":"pc","os_name":"","os_version":"","os_edition":"","kernel_version":"","arch":"","manufacturer":"","model":"","serial_number":"","cpu_model":"","cpu_cores":1,"memory_bytes":1,"uptime_secs":1,"disks":[],"interfaces":[],"logged_in_users":[],"disk_encrypted":null,"firewall_enabled":null});
    let enr: Value =
        anon.post("/agent/enroll", &json!({"token":tok["token"],"os":"linux","agent_version":"0.5.0","inventory":inv})).await.unwrap();
    let agent = Client::new(&target).unwrap().with_endpoint(enr["endpoint_id"].as_str().unwrap(), enr["endpoint_secret"].as_str().unwrap());

    // The agent's long poll wakes up when the admin opens a shell.
    let waiter = {
        let a = agent.clone();
        tokio::spawn(async move { a.get::<Value>("/agent/wait").await.unwrap() })
    };
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    let opened: Value =
        admin.post(&format!("/endpoints/{}/shell", enr["endpoint_id"].as_str().unwrap()), &json!({"cols":100,"rows":30})).await.unwrap();
    assert_eq!(waiter.await.unwrap()["wake"], true);

    let ci: Value = agent.post("/agent/checkin", &json!({"agent_version":"0.5.0","inventory":null,"compliance":[]})).await.unwrap();
    let sid = ci["shells"][0]["session_id"].as_str().unwrap().to_string();
    assert_eq!(ci["shells"][0]["cols"], 100);

    let mut agent_ws = agent.websocket(&format!("/agent/shell/{sid}")).await.unwrap();
    // Output before the admin connects is kept for them.
    agent_ws.send(WsMessage::Binary(b"login banner\r\n".to_vec().into())).await.unwrap();

    let mut admin_ws = anon.websocket(&format!("/shell/{sid}/ws?ticket={}", opened["ticket"].as_str().unwrap())).await.unwrap();
    let first = admin_ws.next().await.unwrap().unwrap();
    assert_eq!(first.into_data().to_vec(), b"login banner\r\n");

    admin_ws.send(WsMessage::Binary(b"whoami\n".to_vec().into())).await.unwrap();
    let got = agent_ws.next().await.unwrap().unwrap();
    assert_eq!(got.into_data().to_vec(), b"whoami\n");

    // The ticket only works once.
    assert!(anon.websocket(&format!("/shell/{sid}/ws?ticket={}", opened["ticket"].as_str().unwrap())).await.is_err());

    // Closing the admin side ends the agent side.
    admin_ws.close(None).await.unwrap();
    let end = tokio::time::timeout(std::time::Duration::from_secs(5), agent_ws.next()).await.unwrap();
    assert!(matches!(end, Some(Ok(WsMessage::Close(_))) | None));
    // A tunnel request reaches the agent with its port.
    admin
        .post::<Value, _>(&format!("/endpoints/{}/shell", enr["endpoint_id"].as_str().unwrap()), &json!({"tunnel_port":3389}))
        .await
        .unwrap();
    let ci: Value = agent.post("/agent/checkin", &json!({"agent_version":"0.5.0","inventory":null,"compliance":[]})).await.unwrap();
    assert_eq!(ci["shells"][0]["tunnel_port"], 3389);
    server.shutdown();
    let _ = std::fs::remove_dir_all(dir);
}
