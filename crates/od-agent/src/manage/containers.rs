//! Containers: a small built-in Portainer for Docker or Podman. Lists
//! containers and images, shows logs, starts/stops/restarts, and updates a
//! container by pulling its image and recreating it with the same settings.

use anyhow::{bail, Context, Result};
use od_core::manage::{ContainerControl, ContainerInfo, ContainerReport, ImageInfo};
use serde_json::Value;

use super::must;

/// docker or podman, whichever answers (Docker first).
pub async fn runtime() -> Option<&'static str> {
    for rt in ["docker", "podman"] {
        if crate::util::which(rt) && crate::util::run(rt, &["version"]).await.map(|o| o.ok()).unwrap_or(false) {
            return Some(rt);
        }
    }
    None
}

async fn need_runtime() -> Result<&'static str> {
    match runtime().await {
        Some(r) => Ok(r),
        None if crate::util::which("docker") || crate::util::which("podman") => {
            bail!("Docker or Podman is installed but not running (the daemon didn't answer)")
        }
        None => bail!("Neither Docker nor Podman is installed on this endpoint"),
    }
}

pub async fn list() -> Result<ContainerReport> {
    let Some(rt) = runtime().await else {
        let error = need_runtime().await.err().map(|e| e.to_string()).unwrap_or_default();
        return Ok(ContainerReport { error, ..Default::default() });
    };
    let version = if rt == "docker" {
        must(rt, &["version", "--format", "{{.Server.Version}}"], 30).await.unwrap_or_default()
    } else {
        must(rt, &["version", "--format", "{{.Version}}"], 30).await.unwrap_or_default()
    };
    let (containers, images) = if rt == "docker" {
        let c = must(rt, &["ps", "-a", "--no-trunc", "--format", "{{json .}}"], 60).await?;
        let i = must(rt, &["images", "--no-trunc", "--format", "{{json .}}"], 60).await?;
        (parse_docker_ps(&c), parse_docker_images(&i))
    } else {
        let c = must(rt, &["ps", "-a", "--format", "json"], 60).await?;
        let i = must(rt, &["images", "--format", "json"], 60).await?;
        (parse_podman_ps(&c), parse_podman_images(&i))
    };
    Ok(ContainerReport {
        runtime: rt.into(),
        version: version.lines().next().unwrap_or("").trim().into(),
        containers,
        images,
        error: String::new(),
    })
}

fn s(v: &Value, k: &str) -> String {
    match &v[k] {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

fn label(labels: &str, key: &str) -> String {
    labels.split(',').find_map(|kv| kv.strip_prefix(&format!("{key}="))).unwrap_or("").to_string()
}

fn parse_docker_ps(out: &str) -> Vec<ContainerInfo> {
    out.lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .map(|v| ContainerInfo {
            id: s(&v, "ID"),
            name: s(&v, "Names"),
            image: s(&v, "Image"),
            state: s(&v, "State"),
            status: s(&v, "Status"),
            ports: s(&v, "Ports"),
            created: s(&v, "CreatedAt"),
            project: label(&s(&v, "Labels"), "com.docker.compose.project"),
        })
        .collect()
}

fn parse_docker_images(out: &str) -> Vec<ImageInfo> {
    out.lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .map(|v| ImageInfo {
            id: s(&v, "ID"),
            repository: s(&v, "Repository"),
            tag: s(&v, "Tag"),
            size: s(&v, "Size"),
            created: s(&v, "CreatedSince"),
        })
        .collect()
}

fn parse_podman_ps(out: &str) -> Vec<ContainerInfo> {
    let arr: Vec<Value> = serde_json::from_str(out.trim()).unwrap_or_default();
    arr.iter()
        .map(|v| {
            let ports = v["Ports"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .map(|p| format!("{}:{}->{}/{}", s(p, "host_ip"), s(p, "host_port"), s(p, "container_port"), s(p, "protocol")))
                        .collect::<Vec<_>>()
                        .join(", ")
                })
                .unwrap_or_default();
            ContainerInfo {
                id: s(v, "Id"),
                name: v["Names"].as_array().and_then(|a| a.first()).and_then(|x| x.as_str()).unwrap_or("").to_string(),
                image: s(v, "Image"),
                state: s(v, "State"),
                status: s(v, "Status"),
                ports,
                created: s(v, "CreatedAt"),
                project: v["Labels"]["com.docker.compose.project"].as_str().unwrap_or("").to_string(),
            }
        })
        .collect()
}

fn parse_podman_images(out: &str) -> Vec<ImageInfo> {
    let arr: Vec<Value> = serde_json::from_str(out.trim()).unwrap_or_default();
    let mut v = vec![];
    for i in &arr {
        let names: Vec<String> =
            i["Names"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default();
        let size = i["Size"].as_u64().map(|b| format!("{:.1}MB", b as f64 / 1e6)).unwrap_or_default();
        let created = s(i, "CreatedAt");
        if names.is_empty() {
            v.push(ImageInfo { id: s(i, "Id"), repository: "<none>".into(), tag: "<none>".into(), size, created });
            continue;
        }
        for n in names {
            let (repo, tag) = n
                .rsplit_once(':')
                .filter(|(_, t)| !t.contains('/'))
                .map(|(r, t)| (r.to_string(), t.to_string()))
                .unwrap_or((n.clone(), String::new()));
            v.push(ImageInfo { id: s(i, "Id"), repository: repo, tag, size: size.clone(), created: created.clone() });
        }
    }
    v
}

pub async fn logs(id: &str, tail: u32) -> Result<String> {
    od_core::manage::check_name("container id", id, ".:-_/@").map_err(anyhow::Error::msg)?;
    let rt = need_runtime().await?;
    let n = tail.clamp(10, 5000).to_string();
    let o = crate::util::run_with(rt, &["logs", "--tail", &n, "--timestamps", "--", id], None, std::time::Duration::from_secs(60)).await?;
    if !o.ok() {
        bail!("{rt} logs failed: {}", o.message());
    }
    // Docker writes the container's stderr to ours; show both, capped at 1 MB.
    let mut out = super::combined(&o);
    if out.len() > 1_000_000 {
        let cut = out.len() - 1_000_000;
        let cut = (cut..out.len()).find(|i| out.is_char_boundary(*i)).unwrap_or(out.len());
        out = out[cut..].to_string();
    }
    Ok(out)
}

pub async fn control(id: &str, c: ContainerControl) -> Result<String> {
    let rt = need_runtime().await?;
    let verb = match c {
        ContainerControl::Start => "start",
        ContainerControl::Stop => "stop",
        ContainerControl::Restart => "restart",
        ContainerControl::Pause => "pause",
        ContainerControl::Unpause => "unpause",
        ContainerControl::Remove => {
            return must(rt, &["rm", "-f", "--", id], 120).await.map(|_| format!("Removed container {id}"));
        }
        ContainerControl::Update => return update(rt, id).await,
    };
    must(rt, &[verb, "--", id], 180).await.map(|_| format!("{rt} {verb} {id}: done"))
}

pub async fn pull(image: &str) -> Result<String> {
    let rt = need_runtime().await?;
    let out = must(rt, &["pull", "--", image], 1800).await?;
    Ok(last_lines(&out, 20))
}

pub async fn remove_image(id: &str) -> Result<String> {
    let rt = need_runtime().await?;
    must(rt, &["rmi", "--", id], 300).await
}

pub async fn prune() -> Result<String> {
    let rt = need_runtime().await?;
    let mut out = vec![];
    for what in ["container", "image", "network"] {
        out.push(must(rt, &[what, "prune", "-f"], 600).await?);
    }
    Ok(out.join("\n"))
}

fn last_lines(s: &str, n: usize) -> String {
    let l: Vec<&str> = s.lines().collect();
    l[l.len().saturating_sub(n)..].join("\n")
}

async fn inspect(rt: &str, what: &str, id: &str) -> Result<Value> {
    let out = must(rt, &[what, "inspect", "--", id], 60).await?;
    let v: Value = serde_json::from_str(&out).context("reading inspect output")?;
    v.as_array().and_then(|a| a.first()).cloned().context("nothing to inspect")
}

/// Pull the image and recreate the container if it changed (Watchtower /
/// Portainer "Recreate with pull"). Compose services go through Compose.
async fn update(rt: &str, id: &str) -> Result<String> {
    let c = inspect(rt, "container", id).await?;
    let labels = &c["Config"]["Labels"];
    let image = c["Config"]["Image"].as_str().context("the container has no image")?.to_string();
    let wd = labels["com.docker.compose.project.working_dir"].as_str().unwrap_or("");
    let service = labels["com.docker.compose.service"].as_str().unwrap_or("");
    let project = labels["com.docker.compose.project"].as_str().unwrap_or("");
    if rt == "docker" && !wd.is_empty() && !service.is_empty() && std::path::Path::new(wd).exists() {
        let mut args: Vec<String> = vec!["compose".into(), "--project-directory".into(), wd.into(), "-p".into(), project.into()];
        for f in labels["com.docker.compose.project.config_files"].as_str().unwrap_or("").split(',').filter(|f| !f.is_empty()) {
            args.extend(["-f".into(), f.into()]);
        }
        args.extend(["up", "-d", "--pull", "always", "--no-deps", "--", service].map(String::from));
        let a: Vec<&str> = args.iter().map(String::as_str).collect();
        let out = must(rt, &a, 1800).await?;
        return Ok(format!("Updated Compose service {service} in {project}\n{}", last_lines(&out, 20)));
    }

    let before = c["Image"].as_str().unwrap_or("").to_string();
    let pulled = must(rt, &["pull", "--", &image], 1800).await?;
    let after = inspect(rt, "image", &image).await?["Id"].as_str().unwrap_or("").to_string();
    if !after.is_empty() && after == before {
        return Ok(format!("{image} is already the newest version; nothing to do"));
    }
    let name = c["Name"].as_str().unwrap_or("").trim_start_matches('/').to_string();
    if name.is_empty() {
        bail!("the container has no name");
    }
    let args = run_args(&c, &name, &image);
    let backup = format!("{name}-old-{}", chrono::Utc::now().format("%Y%m%d%H%M%S"));
    let was_running = c["State"]["Running"].as_bool().unwrap_or(false);
    must(rt, &["rename", id, &backup], 60).await?;
    let _ = must(rt, &["stop", "--", &backup], 120).await;
    let a: Vec<&str> = args.iter().map(String::as_str).collect();
    match must(rt, &a, 300).await {
        Ok(new_id) => {
            if !was_running {
                let _ = must(rt, &["stop", "--", &name], 120).await;
            }
            let _ = must(rt, &["rm", "--", &backup], 120).await;
            Ok(format!(
                "Pulled {image} and recreated {name} (new container {})\n{}",
                new_id.trim().chars().take(12).collect::<String>(),
                last_lines(&pulled, 5)
            ))
        }
        Err(e) => {
            // Put the old one back the way it was.
            let _ = must(rt, &["rm", "-f", "--", &name], 60).await;
            let _ = must(rt, &["rename", &backup, &name], 60).await;
            if was_running {
                let _ = must(rt, &["start", "--", &name], 120).await;
            }
            bail!("Couldn't recreate {name}, so the old container was restored: {e:#}")
        }
    }
}

/// `docker run` arguments that reproduce a container's configuration.
fn run_args(c: &Value, name: &str, image: &str) -> Vec<String> {
    let mut a: Vec<String> = vec!["run".into(), "-d".into(), "--name".into(), name.into()];
    let cfg = &c["Config"];
    let host = &c["HostConfig"];
    for e in cfg["Env"].as_array().into_iter().flatten().filter_map(|x| x.as_str()) {
        a.extend(["-e".into(), e.into()]);
    }
    if let Some(ports) = host["PortBindings"].as_object() {
        for (cport, binds) in ports {
            for b in binds.as_array().into_iter().flatten() {
                let ip = b["HostIp"].as_str().unwrap_or("");
                let hp = b["HostPort"].as_str().unwrap_or("");
                let spec = match (ip.is_empty(), hp.is_empty()) {
                    (true, true) => cport.clone(),
                    (true, false) => format!("{hp}:{cport}"),
                    (false, _) => format!("{ip}:{hp}:{cport}"),
                };
                a.extend(["-p".into(), spec]);
            }
        }
    }
    for m in c["Mounts"].as_array().into_iter().flatten() {
        let dst = m["Destination"].as_str().unwrap_or("");
        let src = match m["Type"].as_str() {
            Some("volume") => m["Name"].as_str().unwrap_or(""),
            Some("bind") => m["Source"].as_str().unwrap_or(""),
            _ => continue,
        };
        if dst.is_empty() || src.is_empty() {
            continue;
        }
        let ro = if m["RW"].as_bool() == Some(false) { ":ro" } else { "" };
        a.extend(["-v".into(), format!("{src}:{dst}{ro}")]);
    }
    let restart = host["RestartPolicy"]["Name"].as_str().unwrap_or("");
    if !restart.is_empty() && restart != "no" {
        let n = host["RestartPolicy"]["MaximumRetryCount"].as_u64().unwrap_or(0);
        a.extend(["--restart".into(), if restart == "on-failure" && n > 0 { format!("on-failure:{n}") } else { restart.into() }]);
    }
    let net = host["NetworkMode"].as_str().unwrap_or("");
    if !net.is_empty() && net != "default" && net != "bridge" {
        a.extend(["--network".into(), net.into()]);
    }
    if host["Privileged"].as_bool() == Some(true) {
        a.push("--privileged".into());
    }
    for cap in host["CapAdd"].as_array().into_iter().flatten().filter_map(|x| x.as_str()) {
        a.extend(["--cap-add".into(), cap.into()]);
    }
    for d in host["Devices"].as_array().into_iter().flatten() {
        if let (Some(h), Some(cn)) = (d["PathOnHost"].as_str(), d["PathInContainer"].as_str()) {
            a.extend(["--device".into(), format!("{h}:{cn}")]);
        }
    }
    if let Some(u) = cfg["User"].as_str().filter(|u| !u.is_empty()) {
        a.extend(["--user".into(), u.into()]);
    }
    if let Some(w) = cfg["WorkingDir"].as_str().filter(|w| !w.is_empty()) {
        a.extend(["--workdir".into(), w.into()]);
    }
    if let Some(labels) = cfg["Labels"].as_object() {
        for (k, v) in labels {
            if let Some(v) = v.as_str() {
                a.extend(["--label".into(), format!("{k}={v}")]);
            }
        }
    }
    a.push("--".into());
    a.push(image.into());
    for x in cfg["Cmd"].as_array().into_iter().flatten().filter_map(|x| x.as_str()) {
        a.push(x.into());
    }
    a
}

/// The command line for `exec` into a container from the remote shell.
pub fn exec_command(rt: &str, container: &str, command: &str) -> Vec<String> {
    let mut v = vec![rt.to_string(), "exec".into(), "-it".into(), "-e".into(), "TERM=xterm-256color".into(), container.to_string()];
    match command {
        "" => v.extend(["sh", "-c", "command -v bash >/dev/null 2>&1 && exec bash || exec sh"].map(String::from)),
        c => v.push(c.into()),
    }
    v
}

/// Shells you can exec into a container.
pub const EXEC_SHELLS: &[&str] = &["", "sh", "bash", "ash", "zsh", "/bin/sh", "/bin/bash", "powershell", "pwsh", "cmd"];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_docker_and_podman() {
        let d = r#"{"ID":"abc","Names":"web","Image":"nginx:latest","State":"running","Status":"Up 2 hours","Ports":"0.0.0.0:80->80/tcp","CreatedAt":"2026-11-27","Labels":"com.docker.compose.project=site,x=y"}"#;
        let c = parse_docker_ps(d);
        assert_eq!(c[0].name, "web");
        assert_eq!(c[0].project, "site");
        let p = r#"[{"Id":"def","Names":["db"],"Image":"docker.io/library/postgres:16","State":"exited","Status":"Exited (0)","Ports":[{"host_ip":"","host_port":5432,"container_port":5432,"protocol":"tcp"}],"Labels":{}}]"#;
        let c = parse_podman_ps(p);
        assert_eq!(c[0].name, "db");
        assert_eq!(c[0].ports, ":5432->5432/tcp");
        let i = parse_podman_images(r#"[{"Id":"1","Names":["localhost:5000/app:v2"],"Size":2000000}]"#);
        assert_eq!((i[0].repository.as_str(), i[0].tag.as_str(), i[0].size.as_str()), ("localhost:5000/app", "v2", "2.0MB"));
    }

    #[test]
    fn recreate_args() {
        let c: Value = serde_json::from_str(
            r#"{"Config":{"Env":["A=1"],"Cmd":["serve"],"Labels":{"k":"v"}},
                "HostConfig":{"PortBindings":{"80/tcp":[{"HostIp":"","HostPort":"8080"}]},"RestartPolicy":{"Name":"unless-stopped"},"NetworkMode":"bridge"},
                "Mounts":[{"Type":"volume","Name":"data","Destination":"/data","RW":true},{"Type":"bind","Source":"/etc/x","Destination":"/x","RW":false}]}"#,
        )
        .unwrap();
        let a = run_args(&c, "web", "nginx:latest").join(" ");
        assert_eq!(
            a,
            "run -d --name web -e A=1 -p 8080:80/tcp -v data:/data -v /etc/x:/x:ro --restart unless-stopped --label k=v -- nginx:latest serve"
        );
    }
}
