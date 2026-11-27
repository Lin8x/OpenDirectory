//! Internet Gateway: agents enroll and check in over mutual TLS, requests
//! without a device certificate are refused, and the admin API isn't served.

use od_client::{Client, Identity, ServerTarget};
use serde_json::{json, Value};

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

#[tokio::test]
async fn agents_use_the_gateway_with_device_certificates() {
    let dir = std::env::temp_dir().join(format!("od-gw-{}", uuid::Uuid::new_v4()));
    let server = od_server::start(od_server::ServerConfig {
        data_dir: dir,
        listen: "127.0.0.1:0".parse().unwrap(),
        tls: true,
        local_listen: None,
        public_host: Some("127.0.0.1".into()),
        web_ui: None,
        tls_cert: None,
        web_assets: None,
    })
    .await
    .unwrap();
    let fp = server.info.tls_fingerprint.clone();
    let lan = ServerTarget { url: format!("https://127.0.0.1:{}", server.listen_addr.port()), fingerprint: fp.clone() };
    let anon = Client::new(&lan).unwrap();
    let v: Value = anon
        .post("/setup", &json!({"organization":"T","domain":"corp.test","admin_username":"admin1","admin_display_name":"A","admin_password":"a-long-password-1","baseline":"custom"}))
        .await
        .unwrap();
    let admin = Client::new(&lan).unwrap().with_session(v["token"].as_str().unwrap());

    // Turn the gateway on.
    let port = free_port();
    let public = format!("https://127.0.0.1:{port}");
    let bad = admin.put::<Value, _>("/internet-gateway", &json!({"enabled":true,"port":port,"public_url":"http://x"})).await;
    assert!(bad.is_err());
    let g: Value = admin.put("/internet-gateway", &json!({"enabled":true,"port":port,"public_url":public})).await.unwrap();
    assert!(g["status"].as_str().unwrap().starts_with("Listening"), "{g}");

    // A remote device enrolls through the gateway with no certificate yet.
    let gw = ServerTarget { url: public.clone(), fingerprint: fp.clone() };
    let tok: Value = admin.post("/enrollment-tokens", &json!({})).await.unwrap();
    let inv = json!({"hostname":"laptop-1","os_name":"x","os_version":"1","os_edition":"","kernel_version":"","arch":"x86_64","manufacturer":"","model":"","serial_number":"","cpu_model":"","cpu_cores":1,"memory_bytes":1,"uptime_secs":1,"disks":[],"interfaces":[],"logged_in_users":[],"disk_encrypted":null,"firewall_enabled":null});
    let enr: Value = Client::new(&gw)
        .unwrap()
        .post("/agent/enroll", &json!({"token":tok["token"],"os":"linux","agent_version":"0.13.0","inventory":inv}))
        .await
        .unwrap();
    let (id, secret) = (enr["endpoint_id"].as_str().unwrap().to_string(), enr["endpoint_secret"].as_str().unwrap().to_string());
    let agent = Client::new(&gw).unwrap().with_endpoint(&id, &secret);
    let ci = json!({"agent_version":"0.13.0","inventory":null,"compliance":[]});

    // Without a device certificate, only the first certificate request is allowed.
    let e = agent.post::<Value, _>("/agent/checkin", &ci).await.unwrap_err().to_string();
    assert!(e.contains("device certificate is required"), "{e}");
    let key = rcgen::KeyPair::generate().unwrap();
    let csr = rcgen::CertificateParams::new(Vec::<String>::new()).unwrap().serialize_request(&key).unwrap().pem().unwrap();
    let cert: Value = agent.post("/agent/device-certificate", &json!({"csr_pem": csr})).await.unwrap();
    let again = agent.post::<Value, _>("/agent/device-certificate", &json!({"csr_pem": csr})).await;
    assert!(again.is_err(), "a second certificate needs the first one");

    // With it, check-ins work, and the server list ends with the gateway.
    agent.set_identity(Some(Identity { cert_pem: cert["cert_pem"].as_str().unwrap().into(), key_pem: key.serialize_pem() })).unwrap();
    let r: Value = agent.post("/agent/checkin", &ci).await.unwrap();
    let servers = r["servers"].as_array().unwrap();
    assert_eq!(servers.last().unwrap()["url"], public.as_str(), "{r}");
    assert_eq!(servers[0]["primary"], true);
    // Renewal over the gateway with the current certificate is fine.
    let _: Value = agent.post("/agent/device-certificate", &json!({"csr_pem": csr})).await.unwrap();

    // Another endpoint's secret with this certificate is refused.
    let tok2: Value = admin.post("/enrollment-tokens", &json!({})).await.unwrap();
    let enr2: Value =
        anon.post("/agent/enroll", &json!({"token":tok2["token"],"os":"linux","agent_version":"0.13.0","inventory":inv})).await.unwrap();
    let mixed = Client::new(&gw).unwrap().with_endpoint(enr2["endpoint_id"].as_str().unwrap(), enr2["endpoint_secret"].as_str().unwrap());
    mixed.set_identity(Some(Identity { cert_pem: cert["cert_pem"].as_str().unwrap().into(), key_pem: key.serialize_pem() })).unwrap();
    let e = mixed.post::<Value, _>("/agent/checkin", &ci).await.unwrap_err().to_string();
    assert!(e.contains("doesn't belong"), "{e}");

    // The admin API isn't on the gateway.
    let gw_admin = Client::new(&gw).unwrap().with_session(v["token"].as_str().unwrap());
    let e = gw_admin.get::<Value>("/users").await.unwrap_err().to_string();
    assert!(e.contains("only serves Endpoint Agents"), "{e}");

    // Retiring the endpoint revokes its certificate.
    admin.delete(&format!("/endpoints/{id}")).await.unwrap();
    assert!(agent.post::<Value, _>("/agent/checkin", &ci).await.is_err());

    // Turning it off closes the port.
    let g: Value = admin.put("/internet-gateway", &json!({"enabled":false,"port":port,"public_url":public})).await.unwrap();
    assert_eq!(g["status"], "Off");
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    assert!(std::net::TcpStream::connect(("127.0.0.1", port)).is_err());
}
