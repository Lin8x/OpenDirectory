//! A primary and a replica server: join, copy, read-only replica, agent
//! failover, promotion and detecting the old primary.

use od_client::{Client, ServerTarget};
use serde_json::{json, Value};

async fn start(name: &str) -> (od_server::RunningServer, ServerTarget) {
    let dir = std::env::temp_dir().join(format!("od-repl-{name}-{}", uuid::Uuid::new_v4()));
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
    (server, target)
}

fn inv(host: &str) -> Value {
    json!({"hostname": host, "os_name":"x","os_version":"1","os_edition":"","kernel_version":"","arch":"x86_64","manufacturer":"","model":"","serial_number":"","cpu_model":"","cpu_cores":1,"memory_bytes":1,"uptime_secs":1,"disks":[],"interfaces":[],"logged_in_users":[],"disk_encrypted":null,"firewall_enabled":null})
}

fn checkin(epoch: u64) -> Value {
    json!({"agent_version":"0.10.0","inventory":null,"compliance":[],"known_epoch":epoch})
}

#[tokio::test]
async fn replica_copies_serves_read_only_and_can_be_promoted() {
    let (_p, pt) = start("primary").await;
    let anon = Client::new(&pt).unwrap();
    let v: Value = anon
        .post("/setup", &json!({"organization":"T","domain":"corp.test","admin_username":"admin1","admin_display_name":"A","admin_password":"a-long-password-1","baseline":"custom"}))
        .await
        .unwrap();
    let admin = Client::new(&pt).unwrap().with_session(v["token"].as_str().unwrap());
    let _: Value = admin.post("/users", &json!({"username":"alice","display_name":"Alice","password":"alice-password-12"})).await.unwrap();
    let tok: Value = admin.post("/enrollment-tokens", &json!({})).await.unwrap();
    let enr: Value = anon
        .post("/agent/enroll", &json!({"token":tok["token"],"os":"linux","agent_version":"0.10.0","inventory":inv("pc-1")}))
        .await
        .unwrap();
    let agent_p = Client::new(&pt).unwrap().with_endpoint(enr["endpoint_id"].as_str().unwrap(), enr["endpoint_secret"].as_str().unwrap());

    // Add a replica and join a fresh server with the code.
    let add: Value = admin.post("/replication/replicas", &json!({"name":"DC02"})).await.unwrap();
    let code = add["join_code"].as_str().unwrap();
    assert!(code.starts_with("ODREPL-"));
    let (_r, rt) = start("replica").await;
    let st: Value = Client::new(&rt).unwrap().post("/replication/join", &json!({ "join_code": code })).await.unwrap();
    assert_eq!(st["role"], "replica", "{st}");

    // The replica has the directory, and the primary's admin can sign in.
    let login: Value =
        Client::new(&rt).unwrap().post("/auth/login", &json!({"username":"admin1","password":"a-long-password-1"})).await.unwrap();
    let radmin = Client::new(&rt).unwrap().with_session(login["token"].as_str().unwrap());
    let users: Value = radmin.get("/users").await.unwrap();
    assert!(users.to_string().contains("alice"), "{users}");

    // Read-only: changes are refused with a pointer to the primary.
    let err =
        radmin.request(reqwest::Method::POST, "/users", Some(&json!({"username":"bob","password":"bob-password-123"}))).await.unwrap_err();
    assert_eq!(err.status.map(|s| s.as_u16()), Some(409), "{}", err.message);
    assert!(err.message.contains("replica"));

    // Agents can check in to the replica, learn both servers, and get no jobs.
    let agent_r = Client::new(&rt).unwrap().with_endpoint(enr["endpoint_id"].as_str().unwrap(), enr["endpoint_secret"].as_str().unwrap());
    let ci: Value = agent_r.post("/agent/checkin", &checkin(0)).await.unwrap();
    assert_eq!(ci["servers"].as_array().unwrap().len(), 2, "{ci}");
    assert_eq!(ci["servers"][0]["primary"], true);
    assert_eq!(ci["jobs"].as_array().unwrap().len(), 0);

    // New changes on the primary reach the replica.
    let _: Value = admin.post("/users", &json!({"username":"carol","display_name":"Carol","password":"carol-password-12"})).await.unwrap();
    let _: Value = radmin.post("/replication/sync", &json!({})).await.unwrap();
    assert!(radmin.get::<Value>("/users").await.unwrap().to_string().contains("carol"));
    // The Admin Center session made on the replica survives the sync.
    let status: Value = radmin.get("/replication").await.unwrap();
    assert!(status["last_sync"].is_string());

    // The primary lists the replica as online.
    let ps: Value = admin.get("/replication").await.unwrap();
    assert_eq!(ps["replicas"][0]["online"], true, "{ps}");

    // An agent client fails over when its preferred server is unreachable.
    let dead = ServerTarget { url: "http://127.0.0.1:9".into(), fingerprint: None };
    let fo = Client::new(&dead).unwrap().with_endpoint(enr["endpoint_id"].as_str().unwrap(), enr["endpoint_secret"].as_str().unwrap());
    fo.set_servers(&[dead.clone(), rt.clone()]);
    let _: Value = fo.post("/agent/checkin", &checkin(0)).await.unwrap();
    assert_eq!(fo.base_url(), rt.url);

    // Promote the replica: it accepts changes and the epoch goes up.
    let pr: Value = radmin.post("/replication/promote", &json!({})).await.unwrap();
    assert_eq!(pr["role"], "primary");
    assert_eq!(pr["epoch"], 1);
    let _: Value = radmin.post("/users", &json!({"username":"dave","display_name":"Dave","password":"dave-password-123"})).await.unwrap();

    // When an agent that knows the new epoch reaches the old primary, the old
    // primary learns it was superseded.
    let _: Value = agent_p.post("/agent/checkin", &checkin(1)).await.unwrap();
    let old: Value = admin.get("/replication").await.unwrap();
    assert_eq!(old["superseded"], true, "{old}");
}
