//! Endpoint management: live queries, actions as jobs, GPO switches and
//! delegated permissions.

use super::*;

async fn checkin(app: &Router, ep: &str, version: &str) {
    let (st, v) =
        call(app, Method::POST, "/agent/checkin", Some(ep), Some(json!({"agent_version": version, "inventory": null, "compliance": []})))
            .await;
    assert_eq!(st, StatusCode::OK, "{v}");
}

/// Answer one round of queries like an agent would.
async fn agent_answers(app: &Router, ep: &str, data: Value) -> Value {
    for _ in 0..50 {
        let (st, w) = call(app, Method::GET, "/agent/wait", Some(ep), None).await;
        assert_eq!(st, StatusCode::OK, "{w}");
        if let Some(q) = w["queries"].as_array().and_then(|a| a.first()).cloned() {
            let (st, v) = call(
                app,
                Method::POST,
                &format!("/agent/queries/{}", q["id"].as_str().unwrap()),
                Some(ep),
                Some(json!({"ok": true, "data": data})),
            )
            .await;
            assert_eq!(st, StatusCode::NO_CONTENT, "{v}");
            return q["query"].clone();
        }
    }
    panic!("no query arrived");
}

#[tokio::test]
async fn services_query_actions_gpo_and_delegation() {
    let app = app();
    let tok = setup(&app).await;
    let auth = Some(tok.as_str());
    let (id, ep) = enroll(&app, auth, "linux", "WEB-01", None).await;
    let (old, _) = enroll(&app, auth, "linux", "OLD-01", None).await;
    checkin(&app, &ep, "0.14.0").await;

    // Old agents get a clear message instead of a timeout.
    let (st, v) = call(&app, Method::GET, &format!("/endpoints/{old}/manage/services"), auth, None).await;
    assert_eq!(st, StatusCode::CONFLICT, "{v}");

    // A live query, answered over the long poll without a check-in.
    let report = json!({"manager":"systemd","services":[{"name":"nginx.service","state":"running","startup":"automatic"}]});
    let path = format!("/endpoints/{id}/manage/services");
    let ((st, v), q) = tokio::join!(call(&app, Method::GET, &path, auth, None), agent_answers(&app, &ep, report.clone()));
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(v, report);
    assert_eq!(q, json!({"op":"services"}));
    // Another endpoint can't answer someone else's query.
    let (st, _) = call(&app, Method::POST, &format!("/agent/queries/{}", uuid::Uuid::new_v4()), Some(&ep), Some(json!({"ok":true}))).await;
    assert_eq!(st, StatusCode::NOT_FOUND);

    // Actions are jobs, validated and audited.
    let acts = format!("/endpoints/{id}/manage/actions");
    let (st, job) = call(&app, Method::POST, &acts, auth, Some(json!({"op":"service","name":"nginx.service","control":"restart"}))).await;
    assert_eq!(st, StatusCode::CREATED, "{job}");
    assert_eq!(job["kind"]["type"], "manage");
    assert_eq!(job["kind"]["action"]["control"], "restart");
    let (st, _) = call(&app, Method::POST, &acts, auth, Some(json!({"op":"package","operation":"install","name":"--force"}))).await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
    let (st, _) =
        call(&app, Method::POST, &format!("/endpoints/{id}/jobs"), auth, Some(json!({"type":"manage","action":{"op":"prune"}}))).await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "the generic job route can't skip the checks");
    let (_, audit) = call(&app, Method::GET, "/audit", auth, None).await;
    assert!(audit.to_string().contains("service.restart"), "{audit}");
    let (_, r) =
        call(&app, Method::POST, "/agent/checkin", Some(&ep), Some(json!({"agent_version":"0.14.0","inventory":null,"compliance":[]})))
            .await;
    assert_eq!(r["jobs"][0]["kind"]["action"]["op"], "service");

    // The Management tools GPO setting turns an area off.
    let (st, v) = call(
        &app,
        Method::POST,
        "/policies",
        auth,
        Some(json!({
        "name":"No services","targets":{"all_endpoints":true},"priority":5,
        "settings":[{"kind":"management_tools","services":false,"software":true,"processes":true,"containers":true}]})),
    )
    .await;
    assert_eq!(st, StatusCode::CREATED, "{v}");
    let (st, _) = call(&app, Method::GET, &path, auth, None).await;
    assert_eq!(st, StatusCode::FORBIDDEN);
    let (st, _) = call(&app, Method::POST, &acts, auth, Some(json!({"op":"service","name":"cups.service","control":"stop"}))).await;
    assert_eq!(st, StatusCode::FORBIDDEN);

    // Delegated operators need the matching permission.
    let (_, alex) = call(&app, Method::POST, "/users", auth, Some(json!({"username":"alex","password":"password-123456"}))).await;
    let (st, d) = call(
        &app,
        Method::POST,
        "/delegations",
        auth,
        Some(json!({
        "name":"Web","trustee_kind":"user","trustee_id": alex["id"],
        "scope":[{"kind":"endpoint","id": id}],"permissions":["view_endpoints","manage_processes"]})),
    )
    .await;
    assert_eq!(st, StatusCode::CREATED, "{d}");
    let (_, v) = login_as(&app, "alex", "password-123456").await;
    let at = format!("Bearer {}", v["token"].as_str().unwrap());
    let op = Some(at.as_str());
    let (st, _) = call(&app, Method::POST, &acts, op, Some(json!({"op":"kill_process","pid":4242,"expect_name":"node"}))).await;
    assert_eq!(st, StatusCode::CREATED);
    let (st, _) = call(&app, Method::POST, &acts, op, Some(json!({"op":"container","id":"web","control":"restart"}))).await;
    assert_eq!(st, StatusCode::FORBIDDEN, "no manage_containers");
    let (st, _) = call(&app, Method::GET, &format!("/endpoints/{id}/manage/containers/web/logs"), op, None).await;
    assert_eq!(st, StatusCode::FORBIDDEN, "logs need manage_containers");
    let (st, _) =
        call(&app, Method::POST, &format!("/endpoints/{old}/manage/actions"), op, Some(json!({"op":"kill_process","pid":4242}))).await;
    assert_eq!(st, StatusCode::NOT_FOUND, "out of scope looks missing");
    let (st, _) = call(&app, Method::PUT, "/portainer", op, Some(json!({"url":"https://p.example"}))).await;
    assert_eq!(st, StatusCode::FORBIDDEN);

    // Container exec needs Remote shell as well as Manage containers.
    let did = d["id"].as_str().unwrap();
    let mut body = d.clone();
    body["permissions"] = json!(["view_endpoints", "manage_containers"]);
    let (st, _) = call(&app, Method::PUT, &format!("/delegations/{did}"), auth, Some(body.clone())).await;
    assert_eq!(st, StatusCode::OK);
    let exec = format!("/endpoints/{id}/manage/containers/web/exec");
    let (st, _) = call(&app, Method::POST, &exec, op, Some(json!({}))).await;
    assert_eq!(st, StatusCode::FORBIDDEN);
    body["permissions"] = json!(["view_endpoints", "manage_containers", "remote_shell"]);
    call(&app, Method::PUT, &format!("/delegations/{did}"), auth, Some(body)).await;
    let (st, _) = call(&app, Method::POST, &exec, op, Some(json!({"command":"rm -rf /"}))).await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
    let (st, v) = call(&app, Method::POST, &exec, op, Some(json!({"command":"bash"}))).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let (_, r) =
        call(&app, Method::POST, "/agent/checkin", Some(&ep), Some(json!({"agent_version":"0.14.0","inventory":null,"compliance":[]})))
            .await;
    assert_eq!(r["shells"][0]["exec"], json!({"container":"web","command":"bash"}));

    // Portainer link (Domain Admins set it, operators can read it).
    let (st, _) = call(&app, Method::PUT, "/portainer", auth, Some(json!({"url":"javascript:alert(1)"}))).await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
    let (st, _) = call(&app, Method::PUT, "/portainer", auth, Some(json!({"url":"https://portainer.corp.example:9443/"}))).await;
    assert_eq!(st, StatusCode::OK);
    let (_, v) = call(&app, Method::GET, "/portainer", op, None).await;
    assert_eq!(v["url"], "https://portainer.corp.example:9443");
}
