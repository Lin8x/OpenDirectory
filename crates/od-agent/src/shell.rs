//! Interactive remote shell: a pseudo-terminal (ConPTY on Windows) running
//! PowerShell or the login shell as SYSTEM/root, connected back to the
//! Management Server over a WebSocket. Binary frames carry terminal data;
//! text frames carry control messages such as {"resize":[cols,rows]}.

use std::io::{Read, Write};

use futures_util::{SinkExt, StreamExt};
use od_client::WsMessage;
use od_core::protocol::ShellRequest;
use portable_pty::{native_pty_system, CommandBuilder, PtySize};

fn shell_command() -> CommandBuilder {
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

pub fn spawn(req: ShellRequest, client: od_client::Client) {
    tokio::spawn(async move {
        if let Err(e) = run(req, client).await {
            tracing::warn!("remote shell ended: {e:#}");
        }
    });
}

async fn run(req: ShellRequest, client: od_client::Client) -> anyhow::Result<()> {
    let ws = client.websocket(&format!("/agent/shell/{}", req.session_id)).await?;
    let (mut sink, mut stream) = ws.split();

    let pty = native_pty_system();
    let pair = pty.openpty(PtySize { rows: req.rows, cols: req.cols, pixel_width: 0, pixel_height: 0 })?;
    let mut child = pair.slave.spawn_command(shell_command())?;
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
