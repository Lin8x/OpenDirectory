//! Interactive remote shell: a pseudo-terminal (ConPTY on Windows) running
//! PowerShell or the login shell as SYSTEM/root, connected back to the
//! Management Server over a WebSocket. Binary frames carry terminal data;
//! text frames carry control messages such as {"resize":[cols,rows]}.

use std::io::{Read, Write};

use futures_util::{SinkExt, StreamExt};
use od_client::WsMessage;
use od_core::protocol::ShellRequest;
use portable_pty::{native_pty_system, CommandBuilder, PtySize};

fn shell_command(exec: Option<&od_core::manage::ContainerExec>, runtime: Option<&str>) -> CommandBuilder {
    if let (Some(x), Some(rt)) = (exec, runtime) {
        let argv = crate::manage::containers::exec_command(rt, &x.container, &x.command);
        let mut c = CommandBuilder::new(&argv[0]);
        c.args(&argv[1..]);
        c.env("TERM", "xterm-256color");
        return c;
    }
    if cfg!(windows) {
        let mut c = CommandBuilder::new("powershell.exe");
        c.arg("-NoLogo");
        c
    } else {
        let sh = ["/bin/bash", "/bin/zsh", "/bin/sh"].into_iter().find(|p| std::path::Path::new(p).exists()).unwrap_or("/bin/sh");
        let mut c = CommandBuilder::new(sh);
        c.arg("-l");
        c.env("TERM", "xterm-256color");
        if std::env::var("HOME").is_err() {
            c.env("HOME", if cfg!(target_os = "macos") { "/var/root" } else { "/root" });
        }
        c
    }
}

pub fn spawn(req: ShellRequest, client: od_client::Client, rules: crate::enduser::SessionRules) {
    tokio::spawn(async move {
        let who = if req.requested_by.is_empty() { "An administrator".to_string() } else { req.requested_by.clone() };
        let background = req.stream.as_ref().is_some_and(|s| s.background.is_some());
        let allowed = if background { rules.check_background() } else { rules.check(req.tunnel_port.is_some(), &who).await };
        if let Err(why) = allowed {
            tracing::warn!("remote session from {who} refused: {why}");
            // Tell the admin's terminal (or stream viewer) why, then hang up.
            if let Ok(ws) = client.websocket(&format!("/agent/shell/{}", req.session_id)).await {
                let (mut sink, _) = ws.split();
                if req.tunnel_port.is_none() {
                    let _ = sink.send(WsMessage::Binary(format!("\r\n[{why}]\r\n").into_bytes().into())).await;
                } else if req.stream.is_some() {
                    let _ = sink.send(WsMessage::Binary(crate::stream::status(&why, true).into())).await;
                }
                let _ = sink.send(WsMessage::Close(None)).await;
            }
            return;
        }
        if let Some(x) = &req.exec {
            if let Err(why) = exec_allowed(x) {
                if let Ok(ws) = client.websocket(&format!("/agent/shell/{}", req.session_id)).await {
                    let (mut sink, _) = ws.split();
                    let _ = sink.send(WsMessage::Binary(format!("\r\n[{why}]\r\n").into_bytes().into())).await;
                    let _ = sink.send(WsMessage::Close(None)).await;
                }
                return;
            }
        }
        let res = match req.tunnel_port {
            Some(od_core::protocol::STREAM_PORT) => {
                crate::stream::session(req.session_id, req.stream.clone().unwrap_or_default(), client).await
            }
            Some(port) => tunnel(req.session_id, port, client).await,
            None => run(req, client).await,
        };
        if let Err(e) = res {
            tracing::warn!("remote shell ended: {e:#}");
        }
    });
}

fn exec_allowed(x: &od_core::manage::ContainerExec) -> Result<(), String> {
    use od_core::manage::Area;
    if !crate::manage::allowed(Area::Containers) {
        return Err("Container management is turned off on this device by policy.".into());
    }
    od_core::manage::check_name("container", &x.container, ".:-_/@")?;
    if !crate::manage::containers::EXEC_SHELLS.contains(&x.command.as_str()) {
        return Err(format!("{} isn't a shell this agent will start in a container", x.command));
    }
    Ok(())
}

async fn run(req: ShellRequest, client: od_client::Client) -> anyhow::Result<()> {
    let runtime = match &req.exec {
        Some(_) => Some(crate::manage::containers::runtime().await.ok_or_else(|| anyhow::anyhow!("Docker or Podman isn't running"))?),
        None => None,
    };
    let ws = client.websocket(&format!("/agent/shell/{}", req.session_id)).await?;
    let (mut sink, mut stream) = ws.split();

    let pty = native_pty_system();
    let pair = pty.openpty(PtySize { rows: req.rows, cols: req.cols, pixel_width: 0, pixel_height: 0 })?;
    let mut child = pair.slave.spawn_command(shell_command(req.exec.as_ref(), runtime))?;
    drop(pair.slave);
    let mut reader = pair.master.try_clone_reader()?;
    let mut writer = pair.master.take_writer()?;
    let master = pair.master;

    // PTY output -> WebSocket (the PTY API is blocking, so read on a thread).
    let (out_tx, mut out_rx) = tokio::sync::mpsc::channel::<Vec<u8>>(64);
    std::thread::spawn(move || {
        let mut buf = [0u8; 8192];
        loop {
            match reader.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if out_tx.blocking_send(buf[..n].to_vec()).is_err() {
                        break;
                    }
                }
            }
        }
    });
    // WebSocket -> PTY input, on a thread for the same reason.
    let (in_tx, in_rx) = std::sync::mpsc::channel::<Vec<u8>>();
    std::thread::spawn(move || {
        while let Ok(data) = in_rx.recv() {
            if writer.write_all(&data).and_then(|_| writer.flush()).is_err() {
                break;
            }
        }
    });

    loop {
        tokio::select! {
            out = out_rx.recv() => match out {
                Some(data) => { if sink.send(WsMessage::Binary(data.into())).await.is_err() { break; } }
                None => break, // the shell exited
            },
            msg = stream.next() => match msg {
                Some(Ok(WsMessage::Binary(b))) => { let _ = in_tx.send(b.to_vec()); }
                Some(Ok(WsMessage::Text(t))) => {
                    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&t) {
                        if let Some([c, r]) = v["resize"].as_array().map(|a| [a.first().and_then(|x| x.as_u64()), a.get(1).and_then(|x| x.as_u64())]) {
                            let _ = master.resize(PtySize { cols: c.unwrap_or(80) as u16, rows: r.unwrap_or(24) as u16, pixel_width: 0, pixel_height: 0 });
                        }
                        if let Some(s) = v["input"].as_str() { let _ = in_tx.send(s.as_bytes().to_vec()); }
                    }
                }
                Some(Ok(WsMessage::Close(_))) | None | Some(Err(_)) => break,
                _ => {}
            },
        }
    }
    let _ = child.kill();
    let _ = sink.send(WsMessage::Close(None)).await;
    Ok(())
}

/// TCP tunnel: connect to a service on this computer (Remote Desktop, VNC,
/// SSH) and pipe it through the WebSocket. Only loopback is reachable.
async fn tunnel(session: uuid::Uuid, port: u16, client: od_client::Client) -> anyhow::Result<()> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let ws = client.websocket(&format!("/agent/shell/{session}")).await?;
    let (mut sink, mut stream) = ws.split();
    let tcp = match tokio::net::TcpStream::connect(("127.0.0.1", port)).await {
        Ok(t) => t,
        Err(e) => {
            let _ = sink.send(WsMessage::Close(None)).await;
            anyhow::bail!("nothing is listening on port {port} ({e}); turn the service on first");
        }
    };
    let (mut rd, mut wr) = tcp.into_split();
    let mut buf = vec![0u8; 32 * 1024];
    loop {
        tokio::select! {
            n = rd.read(&mut buf) => match n {
                Ok(0) | Err(_) => break,
                Ok(n) => { if sink.send(WsMessage::Binary(buf[..n].to_vec().into())).await.is_err() { break; } }
            },
            msg = stream.next() => match msg {
                Some(Ok(WsMessage::Binary(b))) => { if wr.write_all(&b).await.is_err() { break; } }
                Some(Ok(WsMessage::Close(_))) | None | Some(Err(_)) => break,
                _ => {}
            },
        }
    }
    let _ = sink.send(WsMessage::Close(None)).await;
    Ok(())
}
