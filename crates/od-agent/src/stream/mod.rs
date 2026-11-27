//! Low-latency desktop streaming for remote desktop in the browser.
//!
//! The agent starts a *stream host* in the signed-in user's desktop session
//! (on Windows a service can't see the desktop, so the host is launched into
//! the console session as SYSTEM, the way game-streaming hosts do it). The
//! host captures the screen with FFmpeg, encodes H.264 on the GPU (NVENC,
//! AMD AMF, Intel Quick Sync, VAAPI or VideoToolbox, falling back to
//! software x264), and sends one frame per message. It injects the browser's
//! keyboard and mouse input (SendInput on Windows, uinput on Linux).
//!
//! The host connects back to the agent on loopback and proves itself with a
//! one-time token; the agent pipes that connection through the session's
//! WebSocket, so the browser talks to the host through the Management Server.
//!
//! Wire format, both directions: `[type: u8][length: u32 BE][payload]`.
//! Host to browser: 1 = hello (JSON), 2 = video frame (`[flags u8][µs u64 BE]
//! [Annex B access unit]`, flag 1 = keyframe), 3 = status or error (JSON).
//! Browser to host: 10 = input event (JSON).

mod input;
mod keys;
mod launch;

use std::process::Stdio;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use futures_util::{SinkExt, StreamExt};
use od_client::WsMessage;
use od_core::protocol::StreamOptions;
use serde::Deserialize;
use serde_json::json;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

pub const HELLO: u8 = 1;
pub const FRAME: u8 = 2;
pub const STATUS: u8 = 3;
pub const INPUT: u8 = 10;

pub fn frame(kind: u8, payload: &[u8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(payload.len() + 5);
    v.push(kind);
    v.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    v.extend_from_slice(payload);
    v
}

fn status(msg: &str, error: bool) -> Vec<u8> {
    frame(STATUS, json!({ "message": msg, "error": error }).to_string().as_bytes())
}

// ------------------------------------------------------------ agent side

/// Run one stream session for the Management Server: start the host in the
/// user's session and pipe it through the session WebSocket.
pub async fn session(session: uuid::Uuid, opts: StreamOptions, client: od_client::Client) -> Result<()> {
    let ws = client.websocket(&format!("/agent/shell/{session}")).await?;
    let (mut sink, mut stream) = ws.split();
    let host = match start_host(&opts).await {
        Ok(h) => h,
        Err(e) => {
            let _ = sink.send(WsMessage::Binary(status(&format!("{e:#}"), true).into())).await;
            let _ = sink.send(WsMessage::Close(None)).await;
            return Err(e);
        }
    };
    let (mut rd, mut wr) = host.into_split();
    let mut buf = vec![0u8; 256 * 1024];
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

async fn start_host(opts: &StreamOptions) -> Result<TcpStream> {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await?;
    let port = listener.local_addr()?.port();
    let token = hex::encode(rand::random::<[u8; 16]>());
    let opts_json = serde_json::to_string(opts)?;
    let exe = std::env::current_exe()?;
    let args = vec![
        "stream-host".to_string(),
        "--connect".into(),
        format!("127.0.0.1:{port}"),
        "--token".into(),
        token.clone(),
        "--options".into(),
        hex::encode(opts_json.as_bytes()),
    ];
    let mut child = launch::in_console_session(&exe, &args).await?;
    let accepted = tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            let (mut tcp, _) = listener.accept().await?;
            let mut line = String::new();
            let mut r = tokio::io::BufReader::new(&mut tcp);
            let ok = tokio::time::timeout(Duration::from_secs(5), r.read_line(&mut line)).await.is_ok();
            if ok && crate::util::ct_eq(line.trim(), &token) {
                return anyhow::Ok(tcp);
            }
        }
    })
    .await;
    match accepted {
        Ok(Ok(tcp)) => {
            launch::detach(&mut child);
            Ok(tcp)
        }
        Ok(Err(e)) => Err(e),
        Err(_) => {
            launch::kill(&mut child);
            bail!("The stream host didn't start on the endpoint. Is someone signed in, and is FFmpeg installed?")
        }
    }
}

// ------------------------------------------------------------ host side

/// Entry point of `opendirectory-agent stream-host`.
pub async fn host_main(connect: String, token: String, options_hex: String) -> Result<()> {
    let opts: StreamOptions = serde_json::from_slice(&hex::decode(options_hex)?)?;
    let mut tcp = TcpStream::connect(&connect).await.context("connect to the agent")?;
    tcp.write_all(format!("{token}\n").as_bytes()).await?;
    let (mut rd, wr) = tcp.into_split();
    let wr = std::sync::Arc::new(tokio::sync::Mutex::new(wr));

    let ffmpeg = match find_ffmpeg() {
        Some(f) => f,
        None => {
            let _ = wr.lock().await.write_all(&status(FFMPEG_MISSING, true)).await;
            bail!("FFmpeg not found");
        }
    };
    let encoder = pick_encoder(&ffmpeg, &opts.encoder).await;
    let capture = Capture::detect().await?;
    let mut injector = if opts.input { input::Injector::new().map_err(|e| tracing::warn!("input: {e:#}")).ok() } else { None };
    let hello = json!({
        "encoder": encoder.label(),
        "capture": capture.label(),
        "input": injector.is_some(),
        "input_note": if opts.input && injector.is_none() { Some(input::UNAVAILABLE) } else { None },
        "fps": opts.fps.clamp(10, 240),
    });
    wr.lock().await.write_all(&frame(HELLO, hello.to_string().as_bytes())).await?;

    let args = ffmpeg_args(&capture, &encoder, &opts);
    tracing::info!("stream host: {} {}", ffmpeg.display(), args.join(" "));
    let mut cmd = tokio::process::Command::new(&ffmpeg);
    cmd.args(&args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
    capture.env(&mut cmd);
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    let mut child = cmd.spawn().context("start FFmpeg")?;
    let mut out = child.stdout.take().expect("piped");
    let err = child.stderr.take().expect("piped");
    let tail = std::sync::Arc::new(std::sync::Mutex::new(std::collections::VecDeque::<String>::new()));
    let t2 = tail.clone();
    tokio::spawn(async move {
        let mut lines = tokio::io::BufReader::new(err).lines();
        while let Ok(Some(l)) = lines.next_line().await {
            let mut t = t2.lock().unwrap_or_else(|e| e.into_inner());
            t.push_back(l);
            if t.len() > 12 {
                t.pop_front();
            }
        }
    });

    // Video: split FFmpeg's Annex B output into access units.
    let w2 = wr.clone();
    let video = async move {
        let start = Instant::now();
        let mut splitter = AuSplitter::default();
        let mut buf = vec![0u8; 512 * 1024];
        loop {
            let n = out.read(&mut buf).await?;
            if n == 0 {
                break;
            }
            for (au, key) in splitter.push(&buf[..n]) {
                let mut p = Vec::with_capacity(au.len() + 9);
                p.push(key as u8);
                p.extend_from_slice(&(start.elapsed().as_micros() as u64).to_be_bytes());
                p.extend_from_slice(&au);
                w2.lock().await.write_all(&frame(FRAME, &p)).await?;
            }
        }
        anyhow::Ok(())
    };
    // Input from the browser.
    let control = async move {
        let mut hdr = [0u8; 5];
        loop {
            rd.read_exact(&mut hdr).await?;
            let len = u32::from_be_bytes([hdr[1], hdr[2], hdr[3], hdr[4]]) as usize;
            if len > 64 * 1024 {
                bail!("oversized input message");
            }
            let mut body = vec![0u8; len];
            rd.read_exact(&mut body).await?;
            if hdr[0] == INPUT {
                if let (Some(inj), Ok(ev)) = (injector.as_mut(), serde_json::from_slice::<InputEvent>(&body)) {
                    inj.apply(&ev);
                }
            }
        }
        #[allow(unreachable_code)]
        anyhow::Ok(())
    };
    let res = tokio::select! {
        r = video => r,
        r = control => r,
    };
    let _ = child.start_kill();
    let ended = child.try_wait().ok().flatten();
    if res.is_ok() || ended.is_some() {
        let t = tail.lock().unwrap_or_else(|e| e.into_inner()).iter().cloned().collect::<Vec<_>>().join("\n");
        let msg = if t.is_empty() { "The stream ended.".to_string() } else { format!("The stream ended. FFmpeg said:\n{t}") };
        let _ = wr.lock().await.write_all(&status(&msg, true)).await;
    }
    Ok(())
}

const FFMPEG_MISSING: &str = "FFmpeg isn't installed on this endpoint, and the low-latency stream needs it. \
Windows: winget install --id Gyan.FFmpeg -e --scope machine (or put ffmpeg.exe next to the agent). \
Linux: install the ffmpeg package. macOS: brew install ffmpeg.";

/// One browser input event. Coordinates are 0..1 across the picture.
#[derive(Debug, Deserialize)]
#[serde(tag = "t")]
#[cfg_attr(not(any(target_os = "linux", windows)), allow(dead_code))]
pub enum InputEvent {
    /// Pointer moved.
    #[serde(rename = "m")]
    Move { x: f64, y: f64 },
    /// Button 0 left, 1 middle, 2 right, 3 back, 4 forward.
    #[serde(rename = "b")]
    Button { b: u8, d: bool },
    /// Wheel, in notches (positive is down / right).
    #[serde(rename = "w")]
    Wheel {
        #[serde(default)]
        dx: f64,
        #[serde(default)]
        dy: f64,
    },
    /// Key by its physical position (KeyboardEvent.code).
    #[serde(rename = "k")]
    Key { code: String, d: bool },
    /// Release everything (window lost focus).
    #[serde(rename = "r")]
    ReleaseAll,
}

fn find_ffmpeg() -> Option<std::path::PathBuf> {
    let name = if cfg!(windows) { "ffmpeg.exe" } else { "ffmpeg" };
    let mut dirs: Vec<std::path::PathBuf> = vec![];
    if let Some(d) = std::env::current_exe().ok().and_then(|e| e.parent().map(|p| p.to_path_buf())) {
        dirs.push(d.join("ffmpeg"));
        dirs.push(d);
    }
    if cfg!(windows) {
        for var in ["ProgramFiles", "ProgramW6432", "ProgramData"] {
            if let Some(p) = std::env::var_os(var) {
                let p = std::path::PathBuf::from(p);
                dirs.push(p.join("ffmpeg").join("bin"));
                dirs.push(p.join("Microsoft").join("WinGet").join("Links"));
            }
        }
        if let Some(p) = std::env::var_os("SystemDrive") {
            dirs.push(std::path::PathBuf::from(format!("{}\\ffmpeg\\bin", p.to_string_lossy())));
        }
    } else {
        for d in ["/usr/bin", "/usr/local/bin", "/opt/homebrew/bin", "/snap/bin"] {
            dirs.push(d.into());
        }
    }
    if let Some(p) = std::env::var_os("PATH") {
        dirs.extend(std::env::split_paths(&p));
    }
    if cfg!(windows) {
        // winget's machine-wide packages live under Program Files\WinGet\Packages.
        if let Some(pf) = std::env::var_os("ProgramFiles") {
            let root = std::path::PathBuf::from(pf).join("WinGet").join("Packages");
            if let Ok(rd) = std::fs::read_dir(&root) {
                for e in rd.flatten().filter(|e| e.file_name().to_string_lossy().contains("FFmpeg")) {
                    if let Ok(inner) = std::fs::read_dir(e.path()) {
                        for i in inner.flatten() {
                            dirs.push(i.path().join("bin"));
                        }
                    }
                }
            }
        }
    }
    dirs.into_iter().map(|d| d.join(name)).find(|p| p.is_file())
}

// ------------------------------------------------------------ encoders

#[derive(Debug, Clone, Copy, PartialEq)]
enum Encoder {
    Nvenc,
    Amf,
    Qsv,
    Vaapi,
    VideoToolbox,
    X264,
    OpenH264,
}

impl Encoder {
    fn codec(self) -> &'static str {
        match self {
            Encoder::Nvenc => "h264_nvenc",
            Encoder::Amf => "h264_amf",
            Encoder::Qsv => "h264_qsv",
            Encoder::Vaapi => "h264_vaapi",
            Encoder::VideoToolbox => "h264_videotoolbox",
            Encoder::X264 => "libx264",
            Encoder::OpenH264 => "libopenh264",
        }
    }
    fn label(self) -> &'static str {
        match self {
            Encoder::Nvenc => "NVIDIA NVENC",
            Encoder::Amf => "AMD AMF",
            Encoder::Qsv => "Intel Quick Sync",
            Encoder::Vaapi => "VAAPI",
            Encoder::VideoToolbox => "Apple VideoToolbox",
            Encoder::X264 => "Software (x264)",
            Encoder::OpenH264 => "Software (OpenH264)",
        }
    }
    fn from_name(n: &str) -> Option<Encoder> {
        Some(match n {
            "nvenc" => Encoder::Nvenc,
            "amf" => Encoder::Amf,
            "qsv" => Encoder::Qsv,
            "vaapi" => Encoder::Vaapi,
            "videotoolbox" => Encoder::VideoToolbox,
            "software" => Encoder::X264,
            _ => return None,
        })
    }
    fn candidates() -> &'static [Encoder] {
        if cfg!(windows) {
            &[Encoder::Nvenc, Encoder::Amf, Encoder::Qsv, Encoder::X264, Encoder::OpenH264]
        } else if cfg!(target_os = "macos") {
            &[Encoder::VideoToolbox, Encoder::X264, Encoder::OpenH264]
        } else {
            &[Encoder::Nvenc, Encoder::Vaapi, Encoder::Qsv, Encoder::X264, Encoder::OpenH264]
        }
    }
    /// Arguments to initialise a hardware device, before the input.
    fn device_args(self) -> Vec<String> {
        match self {
            Encoder::Vaapi => vec![
                "-vaapi_device".into(),
                VAAPI_NODE.lock().unwrap_or_else(|e| e.into_inner()).clone().unwrap_or_else(|| "/dev/dri/renderD128".into()),
            ],
            _ => vec![],
        }
    }
}

/// The VAAPI render node that worked in the probe (machines with two GPUs
/// often have one without a VAAPI driver).
static VAAPI_NODE: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

fn render_nodes() -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir("/dev/dri")
        .map(|rd| rd.flatten().map(|e| e.path().display().to_string()).filter(|p| p.contains("renderD")).collect())
        .unwrap_or_default();
    v.sort();
    v
}

/// The first encoder that actually works on this machine (a GPU encoder
/// listed by FFmpeg can still fail when there's no matching GPU or driver).
async fn pick_encoder(ffmpeg: &std::path::Path, wanted: &str) -> Encoder {
    let mut order: Vec<Encoder> = Encoder::candidates().to_vec();
    if let Some(w) = Encoder::from_name(wanted) {
        order.retain(|e| *e != w);
        order.insert(0, w);
    }
    for e in order {
        if e == Encoder::Vaapi {
            for node in render_nodes() {
                *VAAPI_NODE.lock().unwrap_or_else(|e| e.into_inner()) = Some(node);
                if probe(ffmpeg, e).await {
                    return e;
                }
            }
            continue;
        }
        if probe(ffmpeg, e).await {
            return e;
        }
    }
    Encoder::X264
}

async fn probe(ffmpeg: &std::path::Path, e: Encoder) -> bool {
    let mut args: Vec<String> = vec!["-hide_banner".into(), "-loglevel".into(), "error".into()];
    args.extend(e.device_args());
    args.extend(["-f", "lavfi", "-i", "color=black:s=640x360:r=30", "-frames:v", "3"].map(String::from));
    if e == Encoder::Vaapi {
        args.extend(["-vf", "format=nv12,hwupload"].map(String::from));
    } else if e != Encoder::VideoToolbox {
        args.extend(["-pix_fmt", if e == Encoder::Qsv { "nv12" } else { "yuv420p" }].map(String::from));
    }
    args.extend(["-c:v", e.codec(), "-f", "null", "-"].map(String::from));
    let mut cmd = tokio::process::Command::new(ffmpeg);
    cmd.args(&args).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).kill_on_drop(true);
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000);
    matches!(tokio::time::timeout(Duration::from_secs(10), cmd.status()).await, Ok(Ok(s)) if s.success())
}

// ------------------------------------------------------------ capture

enum Capture {
    /// Windows Desktop Duplication.
    DdaGrab,
    /// X11, with the session's display and authority file.
    X11 { display: String, xauthority: Option<String> },
    /// Wayland (or no X): straight from the kernel's framebuffer (root only).
    KmsGrab,
    /// macOS screen capture.
    AvFoundation,
}

impl Capture {
    async fn detect() -> Result<Capture> {
        if cfg!(windows) {
            return Ok(Capture::DdaGrab);
        }
        if cfg!(target_os = "macos") {
            return Ok(Capture::AvFoundation);
        }
        // Linux: find a process in the active graphical session with DISPLAY.
        if let Some((display, xauthority, wayland)) = linux_session_env() {
            if !wayland && !display.is_empty() {
                return Ok(Capture::X11 { display, xauthority });
            }
        }
        Ok(Capture::KmsGrab)
    }
    fn label(&self) -> &'static str {
        match self {
            Capture::DdaGrab => "Desktop Duplication",
            Capture::X11 { .. } => "X11",
            Capture::KmsGrab => "KMS (Wayland)",
            Capture::AvFoundation => "AVFoundation",
        }
    }
    fn env(&self, cmd: &mut tokio::process::Command) {
        if let Capture::X11 { display, xauthority } = self {
            cmd.env("DISPLAY", display);
            if let Some(x) = xauthority {
                cmd.env("XAUTHORITY", x);
            }
        }
    }
}

/// DISPLAY and XAUTHORITY of the graphical session, read from its processes.
fn linux_session_env() -> Option<(String, Option<String>, bool)> {
    #[cfg(target_os = "linux")]
    {
        let mut best: Option<(String, Option<String>, bool)> = None;
        for e in std::fs::read_dir("/proc").ok()?.flatten() {
            let name = e.file_name();
            if !name.to_string_lossy().chars().all(|c| c.is_ascii_digit()) {
                continue;
            }
            let Ok(env) = std::fs::read(e.path().join("environ")) else { continue };
            let vars: Vec<&[u8]> = env.split(|b| *b == 0).collect();
            let get = |k: &str| {
                vars.iter().find_map(|v| v.strip_prefix(format!("{k}=").as_bytes())).map(|v| String::from_utf8_lossy(v).to_string())
            };
            let Some(display) = get("DISPLAY") else { continue };
            let wayland = get("XDG_SESSION_TYPE").as_deref() == Some("wayland");
            let cand = (display, get("XAUTHORITY"), wayland);
            // Prefer a process that has an XAUTHORITY.
            if cand.1.is_some() {
                return Some(cand);
            }
            best.get_or_insert(cand);
        }
        best
    }
    #[cfg(not(target_os = "linux"))]
    None
}

fn ffmpeg_args(cap: &Capture, enc: &Encoder, o: &StreamOptions) -> Vec<String> {
    let fps = o.fps.clamp(10, 240);
    let kbps = o.bitrate_kbps.clamp(500, 150_000);
    let mut a: Vec<String> =
        ["-hide_banner", "-loglevel", "error", "-fflags", "nobuffer", "-flags", "low_delay"].map(String::from).to_vec();
    a.extend(enc.device_args());
    match cap {
        Capture::DdaGrab => {
            a.extend(["-f", "lavfi", "-i", &format!("ddagrab=output_idx=0:framerate={fps}:draw_mouse=1")].map(String::from));
        }
        Capture::X11 { display, .. } => {
            a.extend(["-f", "x11grab", "-framerate", &fps.to_string(), "-draw_mouse", "1", "-i", display].map(String::from));
        }
        Capture::KmsGrab => {
            a.extend(["-device", "/dev/dri/card0", "-f", "kmsgrab", "-framerate", &fps.to_string(), "-i", "-"].map(String::from));
        }
        Capture::AvFoundation => {
            a.extend(
                ["-f", "avfoundation", "-capture_cursor", "1", "-framerate", &fps.to_string(), "-i", "Capture screen 0:none"]
                    .map(String::from),
            );
        }
    }
    // Filters: get frames into a format the encoder takes, scaling if asked.
    let scale = (o.max_height > 0).then(|| o.max_height.clamp(240, 4320));
    let vf: Option<String> = match (cap, enc) {
        (Capture::KmsGrab, Encoder::Vaapi) => Some(match scale {
            Some(h) => format!("hwmap=derive_device=vaapi,scale_vaapi=w=-2:h={h}:format=nv12"),
            None => "hwmap=derive_device=vaapi,scale_vaapi=format=nv12".into(),
        }),
        (Capture::KmsGrab, _) => {
            Some(format!("hwmap=derive_device=vaapi,scale_vaapi=format=nv12,hwdownload,format=nv12{}", sw_scale(scale)))
        }
        (Capture::DdaGrab, Encoder::Nvenc | Encoder::Amf) if scale.is_none() => None,
        (Capture::DdaGrab, Encoder::Qsv) if scale.is_none() => Some("hwmap=derive_device=qsv,format=qsv".into()),
        (Capture::DdaGrab, _) => Some(format!("hwdownload,format=bgra{},format=yuv420p", sw_scale(scale))),
        (_, Encoder::Vaapi) => Some(match scale {
            Some(h) => format!("hwupload,scale_vaapi=w=-2:h={h}:format=nv12"),
            None => "hwupload,scale_vaapi=format=nv12".into(),
        }),
        (_, Encoder::Qsv) => Some(format!("format=nv12{}", sw_scale(scale))),
        (_, Encoder::VideoToolbox) => scale.map(|h| format!("scale=-2:{h}")),
        _ => Some(format!("format=yuv420p{}", sw_scale(scale))),
    };
    if let Some(vf) = vf {
        a.extend(["-vf".into(), vf]);
    }
    a.extend(["-an".into(), "-c:v".into(), enc.codec().into()]);
    let gop = (fps * 2).to_string();
    let (b, buf) = (format!("{kbps}k"), format!("{}k", (kbps / fps.max(1)).max(100) * 2));
    match enc {
        Encoder::Nvenc => a.extend(
            [
                "-preset",
                "p1",
                "-tune",
                "ull",
                "-zerolatency",
                "1",
                "-rc",
                "cbr",
                "-b:v",
                &b,
                "-maxrate",
                &b,
                "-bufsize",
                &buf,
                "-profile:v",
                "high",
            ]
            .map(String::from),
        ),
        Encoder::Amf => a.extend(
            ["-usage", "ultralowlatency", "-quality", "speed", "-rc", "cbr", "-b:v", &b, "-maxrate", &b, "-bufsize", &buf]
                .map(String::from),
        ),
        Encoder::Qsv => a.extend(
            ["-preset", "veryfast", "-look_ahead", "0", "-async_depth", "1", "-b:v", &b, "-maxrate", &b, "-bufsize", &buf]
                .map(String::from),
        ),
        Encoder::Vaapi => a.extend(["-rc_mode", "CBR", "-b:v", &b, "-maxrate", &b, "-bufsize", &buf].map(String::from)),
        Encoder::VideoToolbox => a.extend(["-realtime", "1", "-prio_speed", "1", "-b:v", &b, "-pix_fmt", "yuv420p"].map(String::from)),
        Encoder::X264 => a.extend(
            ["-preset", "ultrafast", "-tune", "zerolatency", "-b:v", &b, "-maxrate", &b, "-bufsize", &buf, "-profile:v", "high"]
                .map(String::from),
        ),
        Encoder::OpenH264 => a.extend(["-b:v", &b].map(String::from)),
    }
    a.extend(["-g", &gop, "-bf", "0"].map(String::from));
    // Mark frame boundaries with access unit delimiters (the encoder's own
    // option where it has one: h264_metadata chokes on some GPU output) and
    // repeat SPS/PPS on keyframes so a viewer can join at any keyframe.
    let bsf = match enc {
        Encoder::Nvenc | Encoder::Amf | Encoder::Qsv | Encoder::Vaapi => {
            a.extend(["-aud", "1"].map(String::from));
            "dump_extra=freq=keyframe"
        }
        Encoder::X264 => {
            a.extend(["-x264-params", "aud=1"].map(String::from));
            "dump_extra=freq=keyframe"
        }
        Encoder::VideoToolbox | Encoder::OpenH264 => "dump_extra=freq=keyframe,h264_metadata=aud=insert",
    };
    a.extend(["-bsf:v", bsf, "-flush_packets", "1", "-f", "h264", "pipe:1"].map(String::from));
    a
}

fn sw_scale(h: Option<u32>) -> String {
    h.map(|h| format!(",scale=-2:{h}")).unwrap_or_default()
}

// ------------------------------------------------------------ H.264 framing

/// Splits an H.264 Annex B byte stream into access units at AUD NAL units.
#[derive(Default)]
struct AuSplitter {
    buf: Vec<u8>,
}

impl AuSplitter {
    /// Feed bytes; returns finished access units and whether each is a keyframe.
    fn push(&mut self, data: &[u8]) -> Vec<(Vec<u8>, bool)> {
        self.buf.extend_from_slice(data);
        let mut out = vec![];
        // Find AUD start codes (00 00 01 09 or 00 00 00 01 09), skipping offset 0.
        let mut starts = vec![];
        let b = &self.buf;
        let mut i = 0;
        while i + 3 < b.len() {
            if b[i] == 0 && b[i + 1] == 0 && b[i + 2] == 1 {
                if b[i + 3] & 0x1f == 9 {
                    let s = if i > 0 && b[i - 1] == 0 { i - 1 } else { i };
                    starts.push(s);
                }
                i += 3;
            } else {
                i += 1;
            }
        }
        let starts: Vec<usize> = starts.into_iter().filter(|s| *s > 0).collect();
        let mut prev = 0;
        for s in starts {
            let au = self.buf[prev..s].to_vec();
            if !au.is_empty() {
                let key = has_idr(&au);
                out.push((au, key));
            }
            prev = s;
        }
        self.buf.drain(..prev);
        // A runaway buffer means no AUDs are coming; flush it as one unit.
        if self.buf.len() > 16 * 1024 * 1024 {
            let au = std::mem::take(&mut self.buf);
            let key = has_idr(&au);
            out.push((au, key));
        }
        out
    }
}

fn has_idr(au: &[u8]) -> bool {
    au.windows(4).any(|w| w[0] == 0 && w[1] == 0 && w[2] == 1 && w[3] & 0x1f == 5)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_access_units_at_aud() {
        let aud = [0u8, 0, 0, 1, 9, 0xf0];
        let sps = [0u8, 0, 0, 1, 0x67, 0x64, 0, 0x1f];
        let idr = [0u8, 0, 1, 0x65, 1, 2, 3];
        let p = [0u8, 0, 1, 0x41, 9, 9];
        let mut s = AuSplitter::default();
        let mut stream = vec![];
        stream.extend(aud);
        stream.extend(sps);
        stream.extend(idr);
        stream.extend(aud);
        stream.extend(p);
        stream.extend(aud);
        // Feed in awkward pieces.
        let mut got = vec![];
        for c in stream.chunks(5) {
            got.extend(s.push(c));
        }
        assert_eq!(got.len(), 2);
        assert!(got[0].1, "first unit has the IDR");
        assert!(!got[1].1);
        assert_eq!(got[0].0.len(), aud.len() + sps.len() + idr.len());
        assert!(got[1].0.ends_with(&p));
    }

    #[test]
    fn input_events_parse() {
        let e: InputEvent = serde_json::from_str(r#"{"t":"m","x":0.5,"y":0.25}"#).unwrap();
        assert!(matches!(e, InputEvent::Move { .. }));
        let e: InputEvent = serde_json::from_str(r#"{"t":"k","code":"KeyW","d":true}"#).unwrap();
        assert!(matches!(e, InputEvent::Key { d: true, .. }));
        assert!(keys::lookup("KeyW").is_some());
        assert!(keys::lookup("ArrowLeft").unwrap().ext);
    }

    #[test]
    fn ffmpeg_command_shapes() {
        let o = StreamOptions::default();
        let a = ffmpeg_args(&Capture::DdaGrab, &Encoder::Nvenc, &o).join(" ");
        assert!(a.contains("ddagrab") && a.contains("h264_nvenc") && !a.contains("-vf"));
        let a = ffmpeg_args(&Capture::X11 { display: ":0".into(), xauthority: None }, &Encoder::Vaapi, &o).join(" ");
        assert!(a.contains("x11grab") && a.contains("hwupload") && a.contains("-vaapi_device"));
        let a = ffmpeg_args(&Capture::X11 { display: ":0".into(), xauthority: None }, &Encoder::X264, &o).join(" ");
        assert!(a.contains("zerolatency") && a.ends_with("pipe:1"));
    }
}
