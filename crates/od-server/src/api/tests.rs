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

    let (st, alice) = call(
        &app,
        Method::POST,
        "/users",
        auth,
        Some(json!({"username":"Alice","display_name":"Alice Smith","password":"password-123456"})),
    )
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
    let (st, _) = call(&app, Method::POST, "/auth/login", None, Some(json!({"username":"alice","password":"password-123456"}))).await;
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
    let (_, bob) = call(&app, Method::POST, "/users", auth, Some(json!({"username":"bob","password":"bob-password-123"}))).await;
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
    assert_eq!(accounts[0]["password"], "bob-password-123");
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

async fn enroll(app: &Router, auth: Option<&str>, os: &str, host: &str, ou: Option<&str>) -> (String, String) {
    let (_, t) = call(app, Method::POST, "/enrollment-tokens", auth, Some(json!({"ou_id": ou}))).await;
    let inv = json!({"hostname": host, "os_name":"x","os_version":"1","os_edition":"","kernel_version":"","arch":"x86_64","manufacturer":"","model":"","serial_number":"","cpu_model":"","cpu_cores":1,"memory_bytes":1,"uptime_secs":1,"disks":[],"interfaces":[],"logged_in_users":[],"disk_encrypted":null,"firewall_enabled":null});
    let (st, enr) =
        call(app, Method::POST, "/agent/enroll", None, Some(json!({"token":t["token"],"os":os,"agent_version":"0.3.0","inventory":inv})))
            .await;
    assert_eq!(st, StatusCode::OK, "{enr}");
    let id = enr["endpoint_id"].as_str().unwrap().to_string();
    (id.clone(), format!("Endpoint {}.{}", id, enr["endpoint_secret"].as_str().unwrap()))
}

#[tokio::test]
async fn script_library_targets_and_gpo_scripts() {
    let app = app();
    let tok = setup(&app).await;
    let auth = Some(tok.as_str());

    let (_, ous) = call(&app, Method::GET, "/ous", auth, None).await;
    let ws = ous.as_array().unwrap().iter().find(|o| o["name"] == "Workstations").unwrap()["id"].as_str().unwrap().to_string();
    let (win, win_auth) = enroll(&app, auth, "windows", "PC-01", Some(&ws)).await;
    let (_lin, _) = enroll(&app, auth, "linux", "web-01", None).await;
    let (mac, _) = enroll(&app, auth, "macos", "MAC-01", None).await;

    let (st, tpls) = call(&app, Method::GET, "/scripts/templates", auth, None).await;
    assert_eq!(st, StatusCode::OK);
    assert!(tpls.as_array().unwrap().len() >= 15);

    // From a template: Windows-only with a required parameter.
    let (st, sc) = call(&app, Method::POST, "/scripts", auth, Some(json!({"template":"win-restart-service"}))).await;
    assert_eq!(st, StatusCode::CREATED, "{sc}");
    let sid = sc["id"].as_str().unwrap().to_string();

    // Custom script for every OS.
    let (st, any) = call(
        &app,
        Method::POST,
        "/scripts",
        auth,
        Some(
            json!({"name":"Hello","shell":"default","body":"echo $OD_PARAM_WHO","params":[{"name":"WHO","label":"Who","default":"world"}]}),
        ),
    )
    .await;
    assert_eq!(st, StatusCode::CREATED, "{any}");
    let (st, _) =
        call(&app, Method::POST, "/scripts", auth, Some(json!({"name":"bad","body":"x","params":[{"name":"lower","label":"x"}]}))).await;
    assert_eq!(st, StatusCode::BAD_REQUEST);

    // A group of computers.
    let (_, g) = call(&app, Method::POST, "/groups", auth, Some(json!({"name":"Kiosks"}))).await;
    let gid = g["id"].as_str().unwrap().to_string();
    let (st, g) = call(&app, Method::PUT, &format!("/groups/{gid}/endpoints"), auth, Some(json!({"ids":[mac]}))).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(g["endpoint_ids"].as_array().unwrap().len(), 1);

    // Dry run: OU + group reach PC-01 and MAC-01; the Windows script skips the Mac.
    let (st, pv) = call(
        &app,
        Method::POST,
        &format!("/scripts/{sid}/run"),
        auth,
        Some(json!({"targets":{"ou_ids":[ws],"group_ids":[gid]},"dry_run":true})),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{pv}");
    assert_eq!(pv["targets"].as_array().unwrap().len(), 1);
    assert_eq!(pv["skipped"].as_array().unwrap().len(), 1);

    // Run for real against all endpoints with a parameter.
    let aid = any["id"].as_str().unwrap().to_string();
    let (st, run) = call(
        &app,
        Method::POST,
        &format!("/scripts/{aid}/run"),
        auth,
        Some(json!({"targets":{"all_endpoints":true},"params":{"WHO":"admins"}})),
    )
    .await;
    assert_eq!(st, StatusCode::CREATED, "{run}");
    assert_eq!(run["jobs"].as_array().unwrap().len(), 3);
    assert_eq!(run["jobs"][0]["kind"]["env"]["OD_PARAM_WHO"], "admins");
    let batch = run["batch_id"].as_str().unwrap();
    let (_, bj) = call(&app, Method::GET, &format!("/batches/{batch}/jobs"), auth, None).await;
    assert_eq!(bj.as_array().unwrap().len(), 3);

    // Required parameter left empty is refused when the template default is cleared.
    let (st, _) = call(
        &app,
        Method::POST,
        &format!("/scripts/{sid}/run"),
        auth,
        Some(json!({"targets":{"endpoint_ids":[win]},"params":{"SERVICE":""}})),
    )
    .await;
    assert_eq!(st, StatusCode::BAD_REQUEST);

    // Attach to a GPO linked to Workstations as a startup script.
    let (st, p) = call(
        &app,
        Method::POST,
        "/policies",
        auth,
        Some(json!({"name":"Workstation scripts","targets":{"ou_ids":[ws]},
        "settings":[{"kind":"run_script","script_id":sid,"trigger":{"type":"startup"},"params":{"SERVICE":"Spooler"}}]})),
    )
    .await;
    assert_eq!(st, StatusCode::CREATED, "{p}");
    let (st, _) = call(&app, Method::DELETE, &format!("/scripts/{sid}"), auth, None).await;
    assert_eq!(st, StatusCode::CONFLICT, "in use by a GPO");

    let (st, ci) = call(
        &app,
        Method::POST,
        "/agent/checkin",
        Some(&win_auth),
        Some(json!({"agent_version":"0.3.0","inventory":null,"compliance":[]})),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{ci}");
    let scripts = ci["scripts"].as_array().unwrap();
    assert_eq!(scripts.len(), 1);
    assert_eq!(scripts[0]["env"]["OD_PARAM_SERVICE"], "Spooler");
    assert_eq!(scripts[0]["trigger"]["type"], "startup");
    assert!(scripts[0]["source"].as_str().unwrap().contains("Workstation scripts"));

    // The agent reports the run; it shows up in the script's history.
    let (st, _) = call(&app, Method::POST, "/agent/script-runs", Some(&win_auth), Some(json!({"script_id":sid,"trigger":{"type":"startup"},"source":"GPO \"Workstation scripts\" (at startup)","shell":"powershell","body":"x","exit_code":0,"output":"ok","success":true,"started_at":"2026-11-25T10:00:00Z"}))).await;
    assert_eq!(st, StatusCode::OK);
    let (_, hist) = call(&app, Method::GET, &format!("/scripts/{sid}/jobs"), auth, None).await;
    assert_eq!(hist.as_array().unwrap().len(), 1);
    assert_eq!(hist[0]["status"], "succeeded");
}

#[tokio::test]
async fn onboarding_with_a_baseline_and_modifiers() {
    let app = app();
    let (st, cat) = call(&app, Method::GET, "/baselines", None, None).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(cat["baselines"].as_array().unwrap().len(), 7);

    let body = json!({"baseline":"enterprise","modifiers":{"compliance":["soc2"],"usb":"block","time_server":"time.corp.contoso.com"}});
    let (st, plan) = call(&app, Method::POST, "/baselines/preview", None, Some(body.clone())).await;
    assert_eq!(st, StatusCode::OK, "{plan}");
    assert!(plan["ous"].as_array().unwrap().iter().any(|o| o["name"] == "Finance" && o["parent"] == "Departments"));

    let (st, v) = call(
        &app,
        Method::POST,
        "/setup",
        None,
        Some(json!({
            "organization": "Contoso", "domain": "corp.contoso.com", "admin_username": "dan",
            "admin_display_name": "Dan", "admin_password": "s3cure-enterprise-pass",
            "baseline": "enterprise", "modifiers": body["modifiers"]
        })),
    )
    .await;
    assert_eq!(st, StatusCode::CREATED, "{v}");
    let tok = format!("Bearer {}", v["token"].as_str().unwrap());
    let auth = Some(tok.as_str());

    let (_, ous) = call(&app, Method::GET, "/ous", auth, None).await;
    let names: Vec<&str> = ous.as_array().unwrap().iter().map(|o| o["name"].as_str().unwrap()).collect();
    for n in ["Departments", "Finance", "Workstations", "Laptops", "Admin", "Tier 0", "Pilot"] {
        assert!(names.contains(&n), "missing OU {n}: {names:?}");
    }
    let (_, gpos) = call(&app, Method::GET, "/policies", auth, None).await;
    let gpos = gpos.as_array().unwrap();
    let def = gpos.iter().find(|p| p["name"] == "Default Endpoint Policy").unwrap();
    let kinds: Vec<&str> = def["settings"].as_array().unwrap().iter().map(|s| s["kind"].as_str().unwrap()).collect();
    assert!(kinds.contains(&"usb_storage") && kinds.contains(&"time_server") && kinds.contains(&"security_auditing"), "{kinds:?}");
    assert!(gpos.iter().any(|p| p["name"] == "Patch Ring: Pilot"));
    assert!(gpos.iter().any(|p| p["name"] == "Sign-in Accounts"));

    // The admin is a Domain Admin and in Tier 0.
    let (_, me) = call(&app, Method::GET, "/auth/me", auth, None).await;
    let (_, groups) = call(&app, Method::GET, "/groups", auth, None).await;
    let t0 = groups.as_array().unwrap().iter().find(|g| g["name"] == "Tier 0 Admins").unwrap();
    assert!(t0["member_ids"].as_array().unwrap().contains(&me["user"]["id"]));
}

#[tokio::test]
async fn backup_plan_to_a_folder_with_retention_and_restore() {
    let app = app();
    let tok = setup(&app).await;
    let auth = Some(tok.as_str());
    let dir = std::env::temp_dir().join(format!("od-backups-{}", uuid::Uuid::new_v4()));

    // Encryption needs a passphrase.
    let (st, _) = call(
        &app,
        Method::POST,
        "/backups/plans",
        auth,
        Some(json!({"name":"Nightly","destination":{"type":"local","path":dir},"schedule":{"frequency":"daily","time":"02:00"}})),
    )
    .await;
    assert_eq!(st, StatusCode::BAD_REQUEST);

    let (st, plan) = call(
        &app,
        Method::POST,
        "/backups/plans",
        auth,
        Some(json!({"name":"Nightly","destination":{"type":"local","path":dir},"schedule":{"frequency":"daily","time":"02:00"},
                    "retention":{"keep_last":2},"passphrase":"a long backup passphrase"})),
    )
    .await;
    assert_eq!(st, StatusCode::CREATED, "{plan}");
    assert!(plan["next_run_at"].is_string());
    assert_eq!(plan["has_passphrase"], true);
    let id = plan["id"].as_str().unwrap();

    let (st, t) = call(&app, Method::POST, &format!("/backups/plans/{id}/test"), auth, Some(json!({}))).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(t["ok"], true, "{t}");

    for _ in 0..3 {
        let (_, r) = call(&app, Method::POST, &format!("/backups/plans/{id}/run"), auth, None).await;
        assert_eq!(r["ok"], true, "{r}");
        tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
    }
    let (_, files) = call(&app, Method::GET, &format!("/backups/plans/{id}/files"), auth, None).await;
    assert_eq!(files.as_array().unwrap().len(), 2, "retention keeps 2: {files}");

    // Verify the newest backup with the plan's stored passphrase.
    let name = files[0]["name"].as_str().unwrap();
    let (st, v) =
        call(&app, Method::POST, &format!("/backups/plans/{id}/restore"), auth, Some(json!({"name":name,"verify_only":true}))).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(v["verified"]["users"], 1);

    // Download now: a wrong passphrase on upload-restore is refused.
    let (_, dl) = call(&app, Method::POST, "/backups/download", auth, Some(json!({"passphrase":"another long passphrase"}))).await;
    let (st, _) = call(
        &app,
        Method::POST,
        "/backups/restore",
        auth,
        Some(json!({"file":dl["data"],"passphrase":"wrong wrong wrong","verify_only":true})),
    )
    .await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
    let (st, _) = call(
        &app,
        Method::POST,
        "/backups/restore",
        auth,
        Some(json!({"file":dl["data"],"passphrase":"another long passphrase","verify_only":true})),
    )
    .await;
    assert_eq!(st, StatusCode::OK);

    // Secrets never come back in the listing.
    let (_, list) = call(&app, Method::GET, "/backups/plans", auth, None).await;
    assert!(!list.to_string().contains("a long backup passphrase"));
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn webhook_integration_receives_signed_events() {
    use std::sync::{Arc as A, Mutex};
    let got: A<Mutex<Vec<(String, String)>>> = Default::default();
    let g2 = got.clone();
    let recv = axum::Router::new().route(
        "/hook",
        axum::routing::post(move |headers: axum::http::HeaderMap, body: String| {
            let g = g2.clone();
            async move {
                let sig = headers.get("x-opendirectory-signature").map(|v| v.to_str().unwrap().to_string()).unwrap_or_default();
                g.lock().unwrap().push((sig, body));
                "ok"
            }
        }),
    );
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = l.local_addr().unwrap().port();
    tokio::spawn(async move { axum::serve(l, recv).await.unwrap() });

    let state = AppState {
        db: Db::open_in_memory().unwrap(),
        sealer: Sealer::from_key([1u8; 32]),
        info: Arc::new(ServerInfo { version: "test", endpoint_url: "http://test".into(), tls_fingerprint: None, data_dir: "mem".into() }),
        login_failures: Default::default(),
    };
    super::integrations::spawn_dispatcher(state.clone());
    let app = super::router(state).layer(MockConnectInfo(SocketAddr::from(([10, 0, 0, 5], 5555))));
    let tok = setup(&app).await;
    let auth = Some(tok.as_str());
    let (st, x) = call(&app, Method::POST, "/integrations", auth, Some(json!({"name":"SIEM","connector":{"type":"webhook","url":format!("http://127.0.0.1:{port}/hook"),"secret":"s3cret","events":["user."]}}))).await;
    assert_eq!(st, StatusCode::CREATED, "{x}");
    assert!(!x.to_string().contains("s3cret"));
    let id = x["id"].as_str().unwrap();
    let (_, t) = call(&app, Method::POST, &format!("/integrations/{id}/test"), auth, None).await;
    assert_eq!(t["ok"], true, "{t}");

    // A matching event is delivered; a non-matching one isn't.
    call(&app, Method::POST, "/users", auth, Some(json!({"username":"zoe","display_name":"Zoe","password":"password-123456"}))).await;
    call(&app, Method::POST, "/groups", auth, Some(json!({"name":"Ignored"}))).await;
    for _ in 0..50 {
        if got.lock().unwrap().len() >= 2 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    let got = got.lock().unwrap();
    assert_eq!(got.len(), 2, "{got:?}");
    assert!(got[1].1.contains("user.create") && got[1].0.starts_with("sha256="));
}

#[tokio::test]
async fn recycle_bin_restores_users_with_password_and_groups() {
    let app = app();
    let tok = setup(&app).await;
    let auth = Some(tok.as_str());
    let (_, u) =
        call(&app, Method::POST, "/users", auth, Some(json!({"username":"carol","display_name":"Carol","password":"password-123456"})))
            .await;
    let uid = u["id"].as_str().unwrap().to_string();
    let (_, g) = call(&app, Method::POST, "/groups", auth, Some(json!({"name":"Finance","member_ids":[uid]}))).await;
    let gid = g["id"].as_str().unwrap().to_string();

    assert_eq!(call(&app, Method::DELETE, &format!("/users/{uid}"), auth, None).await.0, StatusCode::NO_CONTENT);
    assert_eq!(call(&app, Method::DELETE, &format!("/groups/{gid}"), auth, None).await.0, StatusCode::NO_CONTENT);
    let (_, bin) = call(&app, Method::GET, "/recycle-bin", auth, None).await;
    assert_eq!(bin.as_array().unwrap().len(), 2);

    // Restore the group first, then the user: membership comes back.
    let gid_entry = bin.as_array().unwrap().iter().find(|e| e["kind"] == "group").unwrap()["id"].as_str().unwrap().to_string();
    let uid_entry = bin.as_array().unwrap().iter().find(|e| e["kind"] == "user").unwrap()["id"].as_str().unwrap().to_string();
    let (st, r) = call(&app, Method::POST, &format!("/recycle-bin/{gid_entry}/restore"), auth, None).await;
    assert_eq!(st, StatusCode::OK, "{r}");
    let (st, r) = call(&app, Method::POST, &format!("/recycle-bin/{uid_entry}/restore"), auth, None).await;
    assert_eq!(st, StatusCode::OK, "{r}");
    let (_, back) = call(&app, Method::GET, &format!("/users/{uid}"), auth, None).await;
    assert!(back["group_ids"].as_array().unwrap().iter().any(|x| x == &json!(gid)));
    assert!(back["password_set_at"].is_string(), "password kept: {back}");
    let (_, bin) = call(&app, Method::GET, "/recycle-bin", auth, None).await;
    assert!(bin.as_array().unwrap().is_empty());
}

#[tokio::test]
async fn password_policies_psos_history_and_self_service() {
    let app = app();
    let tok = setup(&app).await;
    let auth = Some(tok.as_str());
    let (st, _) = call(
        &app,
        Method::PUT,
        "/password-policy",
        auth,
        Some(json!({"min_length":10,"history":2,"lockout_threshold":3,"lockout_minutes":5})),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let (st, _) =
        call(&app, Method::POST, "/users", auth, Some(json!({"username":"dave","display_name":"Dave","password":"short123"}))).await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "domain min length 10");
    let (_, u) =
        call(&app, Method::POST, "/users", auth, Some(json!({"username":"dave","display_name":"Dave","password":"long-enough-1"}))).await;
    let uid = u["id"].as_str().unwrap().to_string();

    // A stricter PSO for a group wins for its members.
    let (_, g) = call(&app, Method::POST, "/groups", auth, Some(json!({"name":"Privileged","member_ids":[uid]}))).await;
    let (st, pso) = call(
        &app,
        Method::POST,
        "/password-policies",
        auth,
        Some(json!({"name":"Admins PSO","precedence":1,"group_ids":[g["id"]],"rules":{"min_length":16,"complexity":true,"history":3}})),
    )
    .await;
    assert_eq!(st, StatusCode::CREATED, "{pso}");
    let (_, r) = call(&app, Method::GET, &format!("/users/{uid}/resultant-password-policy"), auth, None).await;
    assert_eq!(r["source"], "Admins PSO");
    let (st, _) = call(&app, Method::POST, &format!("/users/{uid}/password"), auth, Some(json!({"password":"onlylowercaseletters"}))).await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "complexity");

    // Self-service change with the current password; history blocks reuse.
    let (st, v) = call(
        &app,
        Method::POST,
        "/auth/change-password",
        None,
        Some(json!({"username":"dave","current_password":"long-enough-1","new_password":"Brand-New-Passw0rd"})),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let (st, _) = call(
        &app,
        Method::POST,
        "/auth/change-password",
        None,
        Some(json!({"username":"dave","current_password":"Brand-New-Passw0rd","new_password":"long-enough-1"})),
    )
    .await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "reuse refused");

    // Help-desk reset code works once.
    let (_, code) = call(&app, Method::POST, &format!("/users/{uid}/reset-code"), auth, None).await;
    let c = code["code"].as_str().unwrap();
    let (st, _) = call(
        &app,
        Method::POST,
        "/auth/change-password",
        None,
        Some(json!({"username":"dave","reset_code":c,"new_password":"Another-Passw0rd-2"})),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let (st, _) = call(
        &app,
        Method::POST,
        "/auth/change-password",
        None,
        Some(json!({"username":"dave","reset_code":c,"new_password":"Third-Passw0rd-333"})),
    )
    .await;
    assert_eq!(st, StatusCode::UNAUTHORIZED, "code used up");

    // Wrong current passwords lock the account (threshold 3 from the domain policy; dave's PSO has default lockout 5).
    for _ in 0..5 {
        call(
            &app,
            Method::POST,
            "/auth/change-password",
            None,
            Some(json!({"username":"dave","current_password":"nope","new_password":"Whatever-Passw0rd-1"})),
        )
        .await;
    }
    let (st, _) = call(
        &app,
        Method::POST,
        "/auth/change-password",
        None,
        Some(json!({"username":"dave","current_password":"Another-Passw0rd-2","new_password":"Whatever-Passw0rd-1"})),
    )
    .await;
    assert_eq!(st, StatusCode::TOO_MANY_REQUESTS);
}

#[tokio::test]
async fn laps_escrow_read_and_rotate() {
    let app = app();
    let tok = setup(&app).await;
    let auth = Some(tok.as_str());
    let (eid, ep_auth) = enroll(&app, auth, "linux", "srv-01", None).await;
    let (st, _) = call(&app, Method::POST, "/policies", auth, Some(json!({"name":"LAPS","targets":{"all_endpoints":true},"settings":[{"kind":"local_admin_password","account":"root","length":20,"rotate_days":30}]}))).await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "built-in names refused");
    let (st, _) = call(&app, Method::POST, "/policies", auth, Some(json!({"name":"LAPS","targets":{"all_endpoints":true},"settings":[{"kind":"local_admin_password","account":"odadmin","length":20,"rotate_days":30}]}))).await;
    assert_eq!(st, StatusCode::CREATED);
    let (_, ci) =
        call(&app, Method::POST, "/agent/checkin", Some(&ep_auth), Some(json!({"agent_version":"0.5.0","inventory":null,"compliance":[]})))
            .await;
    assert!(ci["settings"].as_array().unwrap().iter().any(|s| s["kind"] == "local_admin_password"));
    assert_eq!(ci["laps_rotate"], false);

    let (st, _) = call(&app, Method::GET, &format!("/endpoints/{eid}/laps"), auth, None).await;
    assert_eq!(st, StatusCode::NOT_FOUND, "nothing escrowed yet");
    call(&app, Method::POST, "/agent/laps", Some(&ep_auth), Some(json!({"account":"odadmin","password":"First-Random-Pw-123"}))).await;
    call(&app, Method::POST, "/agent/laps", Some(&ep_auth), Some(json!({"account":"odadmin","password":"Second-Random-Pw-456"}))).await;
    let (st, v) = call(&app, Method::GET, &format!("/endpoints/{eid}/laps"), auth, None).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(v["password"], "Second-Random-Pw-456");
    assert_eq!(v["previous_password"], "First-Random-Pw-123");

    call(&app, Method::POST, &format!("/endpoints/{eid}/laps/rotate"), auth, None).await;
    let (_, ci) =
        call(&app, Method::POST, "/agent/checkin", Some(&ep_auth), Some(json!({"agent_version":"0.5.0","inventory":null,"compliance":[]})))
            .await;
    assert_eq!(ci["laps_rotate"], true);
    let (_, audit) = call(&app, Method::GET, "/audit?limit=50", auth, None).await;
    assert!(audit.to_string().contains("laps.read"));
}
