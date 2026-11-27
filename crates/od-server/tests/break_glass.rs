//! Break-glass recovery: a local administrator resets a Domain Admin's
//! password on disk while the server is running. The old password and the
//! old sessions stop working, the new password signs in, and the reset is in
//! the audit log.

use od_client::{Client, ServerTarget};
use od_server::recovery::{reset_password, NewPassword, ResetOptions};
use serde_json::{json, Value};

#[tokio::test]
async fn break_glass_reset_signs_in_and_is_audited() {
    let dir = std::env::temp_dir().join(format!("od-bg-{}", uuid::Uuid::new_v4()));
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
        .post("/setup", &json!({"organization":"T","domain":"corp.test","admin_username":"admin1","admin_display_name":"A","admin_password":"forgotten-password-1","baseline":"custom"}))
        .await
        .unwrap();
    let old_session = Client::new(&target).unwrap().with_session(v["token"].as_str().unwrap());
    let _: Value = old_session.get("/auth/me").await.unwrap();

    // Unknown accounts and the wrong data directory are refused.
    let opts =
        |user: &str, pw: &str| ResetOptions { username: user.into(), password: NewPassword::Given(pw.into()), enable: false, via: None };
    assert!(reset_password(&dir, &opts("nobody", "new-password-12")).is_err());
    assert!(reset_password(&dir.join("missing"), &opts("admin1", "new-password-12")).is_err());
    // Password policy still applies.
    assert!(reset_password(&dir, &opts("admin1", "short")).is_err());

    reset_password(&dir, &opts("CORP\\Admin1", "new-password-12")).unwrap();

    let login = |pw: &'static str| {
        let anon = Client::new(&target).unwrap();
        async move { anon.post::<Value, _>("/auth/login", &json!({"username":"admin1","password":pw})).await }
    };
    assert!(login("forgotten-password-1").await.is_err());
    let s: Value = login("new-password-12").await.unwrap();
    assert!(old_session.get::<Value>("/auth/me").await.is_err(), "old sessions are signed out");

    let admin = Client::new(&target).unwrap().with_session(s["token"].as_str().unwrap());
    let audit: Value = admin.get("/audit?q=break_glass").await.unwrap();
    let entries = audit.as_array().or_else(|| audit["entries"].as_array()).cloned().unwrap_or_default();
    let e = entries.iter().find(|e| e["action"] == "user.password_reset.break_glass").expect("audit entry");
    assert_eq!(e["target"], "admin1");
    assert!(e["actor"].as_str().unwrap().starts_with("local:"));
    let _ = std::fs::remove_dir_all(&dir);
}

/// "Reset settings to defaults" needs the admin's password and the typed
/// domain, clears settings and keeps the directory. A factory reset ticket
/// works once, and the erase itself refuses to run without elevation.
#[tokio::test]
async fn reset_settings_and_factory_reset_ticket() {
    let dir = std::env::temp_dir().join(format!("od-rs-{}", uuid::Uuid::new_v4()));
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
    let v: Value = Client::new(&target)
        .unwrap()
        .post("/setup", &json!({"organization":"T","domain":"corp.test","admin_username":"admin1","admin_display_name":"A","admin_password":"a-long-password-1","baseline":"custom"}))
        .await
        .unwrap();
    let admin = Client::new(&target).unwrap().with_session(v["token"].as_str().unwrap());
    let _: Value = admin.put("/branding", &json!({"product_name":"Contoso IT"})).await.unwrap();

    let wrong_pw = admin.post::<Value, _>("/maintenance/reset-settings", &json!({"password":"nope","confirm":"corp.test"})).await;
    assert!(wrong_pw.is_err());
    let wrong_domain =
        admin.post::<Value, _>("/maintenance/reset-settings", &json!({"password":"a-long-password-1","confirm":"corp"})).await;
    assert!(wrong_domain.is_err());
    let b: Value = admin.get("/branding").await.unwrap();
    assert_eq!(b["product_name"], "Contoso IT");

    let r: Value = admin.post("/maintenance/reset-settings", &json!({"password":"a-long-password-1","confirm":"CORP.TEST"})).await.unwrap();
    assert!(r["reset"].as_array().unwrap().iter().any(|x| x == "Branding"));
    let b: Value = admin.get("/branding").await.unwrap();
    assert_eq!(b["product_name"], "");
    let users: Value = admin.get("/users").await.unwrap();
    assert!(users.as_array().unwrap().iter().any(|u| u["username"] == "admin1"), "the directory is kept");
    let audit: Value = admin.get("/audit?q=settings.reset_defaults").await.unwrap();
    assert!(!audit.as_array().unwrap().is_empty());

    let t: Value = admin.post("/maintenance/factory-reset", &json!({"password":"a-long-password-1","confirm":"corp.test"})).await.unwrap();
    let ticket = t["ticket"].as_str().unwrap();
    let db = od_server::db::Db::open(&dir.join("directory.db")).unwrap();
    assert!(!od_server::api::maintenance::take_factory_reset_ticket(&db, "wrong"), "a wrong ticket is refused");
    let t: Value = admin.post("/maintenance/factory-reset", &json!({"password":"a-long-password-1","confirm":"corp.test"})).await.unwrap();
    let ticket2 = t["ticket"].as_str().unwrap();
    assert_ne!(ticket, ticket2);
    assert!(od_server::api::maintenance::take_factory_reset_ticket(&db, ticket2));
    assert!(!od_server::api::maintenance::take_factory_reset_ticket(&db, ticket2), "tickets are single-use");
    if !od_server::recovery::is_elevated() {
        assert!(od_server::recovery::factory_reset(&dir, None).is_err());
        assert!(dir.join("directory.db").exists());
    }
    let _ = std::fs::remove_dir_all(&dir);
}
