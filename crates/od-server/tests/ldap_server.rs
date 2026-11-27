//! Binds and searches the built-in LDAP server with a real LDAP client.

use ldap3::{LdapConnAsync, Scope, SearchEntry};
use od_client::{Client, ServerTarget};
use serde_json::{json, Value};

#[tokio::test]
async fn ldap_bind_and_search_like_active_directory() {
    let dir = std::env::temp_dir().join(format!("od-ldap-{}", uuid::Uuid::new_v4()));
    let server = od_server::start(od_server::ServerConfig {
        data_dir: dir.clone(),
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
        .post("/setup", &json!({"organization":"T","domain":"corp.test","admin_username":"admin1","admin_display_name":"A","admin_password":"a-long-password-1","baseline":"small_business"}))
        .await
        .unwrap();
    let admin = Client::new(&target).unwrap().with_session(v["token"].as_str().unwrap());
    let ous: Value = admin.get("/ous").await.unwrap();
    let staff = ous.as_array().unwrap().iter().find(|o| o["name"] == "Staff").unwrap()["id"].clone();
    let u: Value = admin
        .post(
            "/users",
            &json!({"username":"jsmith","display_name":"Jane Smith","email":"jane@corp.test","password":"jane-password-123","ou_id":staff}),
        )
        .await
        .unwrap();
    let _: Value = admin.post("/groups", &json!({"name":"Wiki Users","member_ids":[u["id"]]})).await.unwrap();

    let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let st = admin
        .request(od_client::Method::PUT, "/ldap", Some(&json!({"enabled":true,"listen":format!("127.0.0.1:{port}"),"ldaps_listen":""})))
        .await
        .unwrap();
    assert_eq!(st["running"], true, "{st}");

    let (conn, mut ldap) = LdapConnAsync::new(&format!("ldap://127.0.0.1:{port}")).await.unwrap();
    ldap3::drive!(conn);
    // Anonymous searches are refused by default.
    let r = ldap.search("DC=corp,DC=test", Scope::Subtree, "(objectClass=user)", vec!["cn"]).await.unwrap();
    assert!(r.success().is_err());
    // Wrong password.
    assert!(ldap.simple_bind("jsmith@corp.test", "nope").await.unwrap().success().is_err());
    // Bind as the user in three common forms.
    for dn in ["jsmith@corp.test", "CORP\\jsmith", "CN=Jane Smith,OU=Staff,DC=corp,DC=test"] {
        ldap.simple_bind(dn, "jane-password-123").await.unwrap().success().unwrap_or_else(|e| panic!("{dn}: {e}"));
    }
    let (rs, _) = ldap
        .search(
            "DC=corp,DC=test",
            Scope::Subtree,
            "(&(objectCategory=person)(objectClass=user)(sAMAccountName=jsmith))",
            vec!["mail", "memberOf", "userPrincipalName"],
        )
        .await
        .unwrap()
        .success()
        .unwrap();
    assert_eq!(rs.len(), 1);
    let e = SearchEntry::construct(rs.into_iter().next().unwrap());
    assert_eq!(e.dn, "CN=Jane Smith,OU=Staff,DC=corp,DC=test");
    assert_eq!(e.attrs["mail"], vec!["jane@corp.test"]);
    assert!(e.attrs["memberOf"].iter().any(|g| g.starts_with("CN=Wiki Users")));
    // Group lookups with member=.
    let (rs, _) = ldap
        .search("DC=corp,DC=test", Scope::Subtree, "(&(objectClass=group)(member=CN=Jane Smith,OU=Staff,DC=corp,DC=test))", vec!["cn"])
        .await
        .unwrap()
        .success()
        .unwrap();
    assert!(rs.len() >= 2, "Wiki Users and Domain Users");
    // Writes are refused.
    assert!(ldap.delete("CN=Jane Smith,OU=Staff,DC=corp,DC=test").await.unwrap().success().is_err());
    let _ = ldap.unbind().await;
    server.shutdown();
    let _ = std::fs::remove_dir_all(dir);
}
