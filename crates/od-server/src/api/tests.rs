//! End-to-end API tests against an in-memory directory.

use std::net::SocketAddr;
use std::sync::Arc;

use axum::body::Body;
use axum::extract::connect_info::MockConnectInfo;
use axum::http::{Method, Request, StatusCode};
use axum::Router;
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tower::ServiceExt;

use crate::{crypto::Sealer, db::Db, AppState, ServerInfo};

fn app() -> Router {
    let state = AppState {
        db: Db::open_in_memory().unwrap(),
        sealer: Sealer::from_key([1u8; 32]),
        info: Arc::new(ServerInfo { version: "test", endpoint_url: "http://test".into(), tls_fingerprint: None, data_dir: "mem".into() }),
        login_failures: Default::default(),
    };
    super::router(state).layer(MockConnectInfo(SocketAddr::from(([10, 0, 0, 5], 5555))))
}

async fn call(app: &Router, method: Method, path: &str, auth: Option<&str>, body: Option<Value>) -> (StatusCode, Value) {
    let mut req = Request::builder().method(method).uri(format!("/api/v1{path}"));
    if let Some(a) = auth {
        req = req.header("authorization", a);
    }
    let req = match body {
        Some(b) => req.header("content-type", "application/json").body(Body::from(b.to_string())).unwrap(),
        None => req.body(Body::empty()).unwrap(),
    };
    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let v = if bytes.is_empty() { Value::Null } else { serde_json::from_slice(&bytes).unwrap_or(Value::Null) };
    (status, v)
}

async fn setup(app: &Router) -> String {
    let (st, v) = call(
        app,
        Method::POST,
        "/setup",
        None,
        Some(json!({
            "organization": "Contoso", "domain": "corp.contoso.com",
            "admin_username": "dan", "admin_display_name": "Dan", "admin_password": "s3cure-pass!"
        })),
    )
    .await;
    assert_eq!(st, StatusCode::CREATED, "{v}");
    format!("Bearer {}", v["token"].as_str().unwrap())
}

#[tokio::test]
async fn setup_only_runs_once_and_login_works() {
    let app = app();
    let (_, v) = call(&app, Method::GET, "/setup", None, None).await;
    assert_eq!(v["needs_setup"], true);
    setup(&app).await;
    let (st, _) = call(
        &app,
        Method::POST,
        "/setup",
        None,
        Some(json!({"organization":"x","domain":"a.b","admin_username":"eve","admin_display_name":"Eve","admin_password":"aaaaaaaa"})),
    )
    .await;
    assert_eq!(st, StatusCode::CONFLICT);

    let (st, _) = call(&app, Method::POST, "/auth/login", None, Some(json!({"username":"dan","password":"nope"}))).await;
    assert_eq!(st, StatusCode::UNAUTHORIZED);
    let (st, v) = call(&app, Method::POST, "/auth/login", None, Some(json!({"username":"CORP\\Dan","password":"s3cure-pass!"}))).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(v["user"]["username"], "dan");
}

#[tokio::test]
async fn api_requires_admin_session() {
    let app = app();
    setup(&app).await;
    let (st, _) = call(&app, Method::GET, "/users", None, None).await;
    assert_eq!(st, StatusCode::UNAUTHORIZED);
    let (st, _) = call(&app, Method::GET, "/users", Some("Bearer bogus"), None).await;
    assert_eq!(st, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn users_groups_and_guards() {
    let app = app();
    let tok = setup(&app).await;
    let auth = Some(tok.as_str());

    let (st, v) = call(&app, Method::POST, "/users", auth, Some(json!({"username":"root"}))).await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "{v}");

    let (st, alice) =
        call(&app, Method::POST, "/users", auth, Some(json!({"username":"Alice","display_name":"Alice Smith","password":"password123"})))
            .await;
    assert_eq!(st, StatusCode::CREATED, "{alice}");
    assert_eq!(alice["username"], "alice");
    assert_eq!(alice["group_ids"].as_array().unwrap().len(), 1, "auto-added to Domain Users");

    let (st, _) = call(&app, Method::POST, "/users", auth, Some(json!({"username":"alice"}))).await;
    assert_eq!(st, StatusCode::CONFLICT);

    let (st, g) = call(&app, Method::POST, "/groups", auth, Some(json!({"name":"Engineering","member_ids":[alice["id"]]}))).await;
    assert_eq!(st, StatusCode::CREATED);
    assert_eq!(g["member_ids"].as_array().unwrap().len(), 1);

    // The last admin can't be disabled.
    let (_, me) = call(&app, Method::GET, "/auth/me", auth, None).await;
    let my_id = me["user"]["id"].as_str().unwrap().to_string();
    let (st, _) = call(&app, Method::PATCH, &format!("/users/{my_id}"), auth, Some(json!({"enabled": false}))).await;
    assert_eq!(st, StatusCode::CONFLICT);

    // Non-admins can't sign in to the Admin Center.
    let (st, _) = call(&app, Method::POST, "/auth/login", None, Some(json!({"username":"alice","password":"password123"}))).await;
    assert_eq!(st, StatusCode::FORBIDDEN);

    let (st, res) = call(&app, Method::GET, "/search?q=smith", auth, None).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(res.as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn ou_cycles_and_nonempty_deletes_are_rejected() {
    let app = app();
    let tok = setup(&app).await;
    let auth = Some(tok.as_str());
    let (_, a) = call(&app, Method::POST, "/ous", auth, Some(json!({"name":"A"}))).await;
    let (_, b) = call(&app, Method::POST, "/ous", auth, Some(json!({"name":"B","parent_id":a["id"]}))).await;
    let (st, _) =
        call(&app, Method::PATCH, &format!("/ous/{}", a["id"].as_str().unwrap()), auth, Some(json!({"parent_id": b["id"]}))).await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
    let (st, _) = call(&app, Method::DELETE, &format!("/ous/{}", a["id"].as_str().unwrap()), auth, None).await;
    assert_eq!(st, StatusCode::CONFLICT);
    let (st, _) = call(&app, Method::DELETE, &format!("/ous/{}", b["id"].as_str().unwrap()), auth, None).await;
    assert_eq!(st, StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn endpoint_enrollment_checkin_accounts_and_jobs() {
    let app = app();
    let tok = setup(&app).await;
    let auth = Some(tok.as_str());

    // A group whose members get sign-in accounts on every endpoint.
    let (_, bob) = call(&app, Method::POST, "/users", auth, Some(json!({"username":"bob","password":"bobpassword"}))).await;
    let (_, g) = call(&app, Method::POST, "/groups", auth, Some(json!({"name":"Laptop Users","member_ids":[bob["id"]]}))).await;
    let (st, v) = call(
        &app,
        Method::POST,
        "/policies",
        auth,
        Some(json!({"name":"Sign-in","targets":{"all_endpoints":true},"settings":[{"kind":"local_accounts","group_id":g["id"],"admin":false},{"kind":"screen_lock","minutes":5}],"priority":10})),
    )
    .await;
    assert_eq!(st, StatusCode::CREATED, "{v}");

    let (_, t) = call(&app, Method::POST, "/enrollment-tokens", auth, Some(json!({"description":"Office","max_uses":1}))).await;
    let code = t["token"].as_str().unwrap();

    let inv = json!({"hostname":"LAPTOP-01","os_name":"Windows 11 Home","os_version":"23H2","os_edition":"Core","kernel_version":"","arch":"x86_64","manufacturer":"","model":"","serial_number":"SN123","cpu_model":"","cpu_cores":8,"memory_bytes":0,"uptime_secs":0,"disks":[],"interfaces":[],"logged_in_users":[],"disk_encrypted":null,"firewall_enabled":true});
    let (st, enr) =
        call(&app, Method::POST, "/agent/enroll", None, Some(json!({"token":code,"os":"windows","agent_version":"0.1.0","inventory":inv})))
            .await;
    assert_eq!(st, StatusCode::OK, "{enr}");
    // Single-use token is now spent.
    let (st, _) =
        call(&app, Method::POST, "/agent/enroll", None, Some(json!({"token":code,"os":"windows","agent_version":"0.1.0","inventory":inv})))
            .await;
    assert_eq!(st, StatusCode::FORBIDDEN);

    let ep_auth = format!("Endpoint {}.{}", enr["endpoint_id"].as_str().unwrap(), enr["endpoint_secret"].as_str().unwrap());
    let (st, _) = call(
        &app,
        Method::POST,
        "/agent/checkin",
        Some(&format!("Endpoint {}.wrong", enr["endpoint_id"].as_str().unwrap())),
        Some(json!({"agent_version":"0.1.0","inventory":null,"compliance":[]})),
    )
    .await;
    assert_eq!(st, StatusCode::UNAUTHORIZED);

    let (st, ci) =
        call(&app, Method::POST, "/agent/checkin", Some(&ep_auth), Some(json!({"agent_version":"0.1.0","inventory":null,"compliance":[]})))
            .await;
    assert_eq!(st, StatusCode::OK, "{ci}");
    let accounts = ci["accounts"].as_array().unwrap();
    assert_eq!(accounts.len(), 1);
    assert_eq!(accounts[0]["username"], "bob");
    assert_eq!(accounts[0]["password"], "bobpassword");
    // Default policy (priority 1000) loses screen lock to ours (priority 10).
    assert!(ci["settings"].as_array().unwrap().iter().any(|s| s["kind"] == "screen_lock" && s["minutes"] == 5));

    // Once applied, the password isn't sent again.
    let ver = accounts[0]["password_version"].as_i64().unwrap();
    let (_, ci) = call(
        &app,
        Method::POST,
        "/agent/checkin",
        Some(&ep_auth),
        Some(json!({"agent_version":"0.1.0","inventory":null,"compliance":[],"account_versions":{"bob":ver}})),
    )
    .await;
    assert!(ci["accounts"][0]["password"].is_null());

    // Queue a script, the agent picks it up and reports back.
    let eid = enr["endpoint_id"].as_str().unwrap();
    let (st, job) = call(
        &app,
        Method::POST,
        &format!("/endpoints/{eid}/jobs"),
        auth,
        Some(json!({"type":"script","shell":"powershell","script":"Get-Date"})),
    )
    .await;
    assert_eq!(st, StatusCode::CREATED, "{job}");
    let (_, ci) = call(
        &app,
        Method::POST,
        "/agent/checkin",
        Some(&ep_auth),
        Some(json!({"agent_version":"0.1.0","inventory":null,"compliance":[],"account_versions":{"bob":ver}})),
    )
    .await;
    assert_eq!(ci["jobs"].as_array().unwrap().len(), 1);
    let jid = job["id"].as_str().unwrap();
    let (st, _) = call(
        &app,
        Method::POST,
        &format!("/agent/jobs/{jid}/result"),
        Some(&ep_auth),
        Some(json!({"exit_code":0,"output":"Tuesday","success":true})),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let (_, j) = call(&app, Method::GET, &format!("/jobs/{jid}"), auth, None).await;
    assert_eq!(j["status"], "succeeded");
    assert_eq!(j["output"], "Tuesday");

    let (st, rsop) = call(&app, Method::GET, &format!("/endpoints/{eid}/rsop"), auth, None).await;
    assert_eq!(st, StatusCode::OK);
    let lock = rsop.as_array().unwrap().iter().find(|e| e["setting"]["kind"] == "screen_lock").unwrap();
    assert_eq!(lock["policy_name"], "Sign-in");
    assert_eq!(lock["overridden"][0], "Default Endpoint Policy");

    let (_, ep) = call(&app, Method::GET, &format!("/endpoints/{eid}"), auth, None).await;
    assert_eq!(ep["public_ip"], "10.0.0.5");
    assert_eq!(ep["online"], true);
}

#[tokio::test]
async fn domain_admin_role_and_group_stay_in_sync() {
    let app = app();
    let auth = setup(&app).await;
    let (_, groups) = call(&app, Method::GET, "/groups", Some(&auth), None).await;
    let da = groups.as_array().unwrap().iter().find(|g| g["name"] == "Domain Admins").unwrap()["id"].as_str().unwrap().to_string();

    // Creating a user with the Admin role puts them in Domain Admins.
    let (st, u) =
        call(&app, Method::POST, "/users", Some(&auth), Some(json!({"username":"eve","display_name":"Eve","role":"admin"}))).await;
    assert_eq!(st, StatusCode::CREATED, "{u}");
    let eve = u["id"].as_str().unwrap().to_string();
    let (_, g) = call(&app, Method::GET, &format!("/groups/{da}"), Some(&auth), None).await;
    assert!(g["member_ids"].as_array().unwrap().iter().any(|m| m == &json!(eve)));

    // Removing her from the group takes the role away.
    let (_, g) = call(&app, Method::GET, &format!("/groups/{da}"), Some(&auth), None).await;
    let others: Vec<Value> = g["member_ids"].as_array().unwrap().iter().filter(|m| *m != &json!(eve)).cloned().collect();
    let (st, _) = call(&app, Method::PUT, &format!("/groups/{da}/members"), Some(&auth), Some(json!({ "ids": others }))).await;
    assert_eq!(st, StatusCode::OK);
    let (_, u) = call(&app, Method::GET, &format!("/users/{eve}"), Some(&auth), None).await;
    assert_eq!(u["role"], "user");

    // Emptying Domain Admins is refused.
    let (st, _) = call(&app, Method::PUT, &format!("/groups/{da}/members"), Some(&auth), Some(json!({ "ids": [] }))).await;
    assert_eq!(st, StatusCode::CONFLICT);

    // Built-in groups can't be renamed or duplicated.
    let (st, _) = call(&app, Method::PATCH, &format!("/groups/{da}"), Some(&auth), Some(json!({"name":"Admins"}))).await;
    assert_eq!(st, StatusCode::CONFLICT);
    let (st, _) = call(&app, Method::POST, "/groups", Some(&auth), Some(json!({"name":"domain admins"}))).await;
    assert_eq!(st, StatusCode::CONFLICT);
}

#[tokio::test]
async fn repeated_wrong_passwords_lock_the_account_temporarily() {
    let app = app();
    setup(&app).await;
    for _ in 0..5 {
        let (st, _) = call(&app, Method::POST, "/auth/login", None, Some(json!({"username":"dan","password":"wrong"}))).await;
        assert_eq!(st, StatusCode::UNAUTHORIZED);
    }
    let (st, v) = call(&app, Method::POST, "/auth/login", None, Some(json!({"username":"dan","password":"s3cure-pass!"}))).await;
    assert_eq!(st, StatusCode::TOO_MANY_REQUESTS, "{v}");
}
