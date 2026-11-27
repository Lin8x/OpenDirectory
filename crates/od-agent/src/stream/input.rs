//! Keyboard and mouse injection for the desktop stream.

use super::InputEvent;

#[cfg(any(target_os = "linux", windows))]
use super::keys;

pub const UNAVAILABLE: &str = if cfg!(target_os = "macos") {
    "Keyboard and mouse control isn't available on macOS yet; this session is view-only."
} else {
    "Keyboard and mouse control isn't available on this endpoint; this session is view-only."
};

// ------------------------------------------------------------ Linux (uinput)

#[cfg(target_os = "linux")]
pub use linux::Injector;

#[cfg(target_os = "linux")]
mod linux {
    use super::*;
    use anyhow::{bail, Result};
    use std::collections::HashSet;
    use std::io::Write;
    use std::os::fd::AsRawFd;

    const EV_SYN: u16 = 0;
    const EV_KEY: u16 = 1;
    const EV_REL: u16 = 2;
    const EV_ABS: u16 = 3;
    const REL_HWHEEL: u16 = 6;
    const REL_WHEEL: u16 = 8;
    const ABS_X: u16 = 0;
    const ABS_Y: u16 = 1;
    const BTN_LEFT: u16 = 0x110;
    const BTN_RIGHT: u16 = 0x111;
    const BTN_MIDDLE: u16 = 0x112;
    const BTN_SIDE: u16 = 0x113;
    const BTN_EXTRA: u16 = 0x114;
    const INPUT_PROP_DIRECT: u16 = 1;
    const ABS_MAX: i32 = 65535;

    // ioctl numbers from linux/uinput.h.
    const UI_DEV_CREATE: u64 = 0x5501;
    const UI_DEV_DESTROY: u64 = 0x5502;
    const UI_SET_EVBIT: u64 = 0x4004_5564;
    const UI_SET_KEYBIT: u64 = 0x4004_5565;
    const UI_SET_RELBIT: u64 = 0x4004_5566;
    const UI_SET_ABSBIT: u64 = 0x4004_5567;
    const UI_SET_PROPBIT: u64 = 0x4004_556e;

    struct Device(std::fs::File);

    impl Device {
        fn create(name: &str, setup: impl Fn(&std::fs::File) -> Result<()>, abs: bool) -> Result<Device> {
            let f = std::fs::OpenOptions::new()
                .write(true)
                .open("/dev/uinput")
                .or_else(|_| std::fs::OpenOptions::new().write(true).open("/dev/input/uinput"))?;
            setup(&f)?;
            let mut dev: libc::uinput_user_dev = unsafe { std::mem::zeroed() };
            for (i, b) in name.bytes().take(libc::UINPUT_MAX_NAME_SIZE - 1).enumerate() {
                dev.name[i] = b as libc::c_char;
            }
            dev.id.bustype = 0x06; // BUS_VIRTUAL
            dev.id.vendor = 0x4f44; // "OD"
            dev.id.product = if abs { 2 } else { 1 };
            dev.id.version = 1;
            if abs {
                dev.absmax[ABS_X as usize] = ABS_MAX;
                dev.absmax[ABS_Y as usize] = ABS_MAX;
            }
            let bytes = unsafe { std::slice::from_raw_parts(&dev as *const _ as *const u8, std::mem::size_of::<libc::uinput_user_dev>()) };
            (&f).write_all(bytes)?;
            ioctl(&f, UI_DEV_CREATE, 0)?;
            Ok(Device(f))
        }
        fn emit(&self, ty: u16, code: u16, value: i32) {
            let mut ev: libc::input_event = unsafe { std::mem::zeroed() };
            ev.type_ = ty;
            ev.code = code;
            ev.value = value;
            let bytes = unsafe { std::slice::from_raw_parts(&ev as *const _ as *const u8, std::mem::size_of::<libc::input_event>()) };
            let _ = (&self.0).write_all(bytes);
        }
        fn sync(&self) {
            self.emit(EV_SYN, 0, 0);
        }
    }

    impl Drop for Device {
        fn drop(&mut self) {
            let _ = ioctl(&self.0, UI_DEV_DESTROY, 0);
        }
    }

    fn ioctl(f: &std::fs::File, req: u64, arg: u64) -> Result<()> {
        let r = unsafe { libc::ioctl(f.as_raw_fd(), req as _, arg as libc::c_ulong) };
        if r < 0 {
            bail!("uinput ioctl {req:#x}: {}", std::io::Error::last_os_error());
        }
        Ok(())
    }

    /// Two virtual devices: a keyboard and an absolute pointer (like a
    /// touchscreen or tablet), so the cursor lands exactly where clicked.
    /// uinput works the same under X11 and Wayland.
    pub struct Injector {
        kbd: Device,
        ptr: Device,
        held: HashSet<(bool, u16)>,
    }

    impl Injector {
        pub fn new() -> Result<Injector> {
            let kbd = Device::create(
                "OpenDirectory remote keyboard",
                |f| {
                    ioctl(f, UI_SET_EVBIT, EV_KEY as u64)?;
                    for c in keys::ALL_EVDEV {
                        ioctl(f, UI_SET_KEYBIT, c as u64)?;
                    }
                    Ok(())
                },
                false,
            )?;
            let ptr = Device::create(
                "OpenDirectory remote pointer",
                |f| {
                    ioctl(f, UI_SET_EVBIT, EV_KEY as u64)?;
                    ioctl(f, UI_SET_EVBIT, EV_ABS as u64)?;
                    ioctl(f, UI_SET_EVBIT, EV_REL as u64)?;
                    for b in [BTN_LEFT, BTN_RIGHT, BTN_MIDDLE, BTN_SIDE, BTN_EXTRA] {
                        ioctl(f, UI_SET_KEYBIT, b as u64)?;
                    }
                    ioctl(f, UI_SET_ABSBIT, ABS_X as u64)?;
                    ioctl(f, UI_SET_ABSBIT, ABS_Y as u64)?;
                    ioctl(f, UI_SET_RELBIT, REL_WHEEL as u64)?;
                    ioctl(f, UI_SET_RELBIT, REL_HWHEEL as u64)?;
                    ioctl(f, UI_SET_PROPBIT, INPUT_PROP_DIRECT as u64)?;
                    Ok(())
                },
                true,
            )?;
            Ok(Injector { kbd, ptr, held: HashSet::new() })
        }

        pub fn apply(&mut self, ev: &InputEvent) {
            match ev {
                InputEvent::Move { x, y } => {
                    self.ptr.emit(EV_ABS, ABS_X, (x.clamp(0.0, 1.0) * ABS_MAX as f64) as i32);
                    self.ptr.emit(EV_ABS, ABS_Y, (y.clamp(0.0, 1.0) * ABS_MAX as f64) as i32);
                    self.ptr.sync();
                }
                InputEvent::Button { b, d } => {
                    let code = match b {
                        0 => BTN_LEFT,
                        1 => BTN_MIDDLE,
                        2 => BTN_RIGHT,
                        3 => BTN_SIDE,
                        4 => BTN_EXTRA,
                        _ => return,
                    };
                    self.track(true, code, *d);
                    self.ptr.emit(EV_KEY, code, *d as i32);
                    self.ptr.sync();
                }
                InputEvent::Wheel { dx, dy } => {
                    if *dy != 0.0 {
                        self.ptr.emit(EV_REL, REL_WHEEL, -dy.signum() as i32 * dy.abs().ceil().min(10.0) as i32);
                    }
                    if *dx != 0.0 {
                        self.ptr.emit(EV_REL, REL_HWHEEL, dx.signum() as i32 * dx.abs().ceil().min(10.0) as i32);
                    }
                    self.ptr.sync();
                }
                InputEvent::Key { code, d } => {
                    if let Some(k) = keys::lookup(code) {
                        self.track(false, k.evdev, *d);
                        self.kbd.emit(EV_KEY, k.evdev, *d as i32);
                        self.kbd.sync();
                    }
                }
                InputEvent::ReleaseAll => self.release_all(),
            }
        }

        fn track(&mut self, ptr: bool, code: u16, down: bool) {
            if down {
                self.held.insert((ptr, code));
            } else {
                self.held.remove(&(ptr, code));
            }
        }

        fn release_all(&mut self) {
            for (ptr, code) in std::mem::take(&mut self.held) {
                let dev = if ptr { &self.ptr } else { &self.kbd };
                dev.emit(EV_KEY, code, 0);
                dev.sync();
            }
        }
    }

    impl Drop for Injector {
        fn drop(&mut self) {
            self.release_all();
        }
    }
}

// ------------------------------------------------------------ Windows (SendInput)

#[cfg(windows)]
pub use win::Injector;

#[cfg(windows)]
mod win {
    use super::*;
    use std::collections::HashSet;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;

    pub struct Injector {
        held: HashSet<(bool, u16, bool)>,
    }

    fn send(inputs: &[INPUT]) {
        unsafe {
            SendInput(inputs.len() as u32, inputs.as_ptr(), std::mem::size_of::<INPUT>() as i32);
        }
    }

    fn mouse(dx: i32, dy: i32, data: i32, flags: u32) -> INPUT {
        INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: INPUT_0 { mi: MOUSEINPUT { dx, dy, mouseData: data as u32, dwFlags: flags, time: 0, dwExtraInfo: 0 } },
        }
    }

    fn key(scan: u16, ext: bool, down: bool) -> INPUT {
        let mut flags = KEYEVENTF_SCANCODE;
        if ext {
            flags |= KEYEVENTF_EXTENDEDKEY;
        }
        if !down {
            flags |= KEYEVENTF_KEYUP;
        }
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: 0, wScan: scan, dwFlags: flags, time: 0, dwExtraInfo: 0 } },
        }
    }

    impl Injector {
        pub fn new() -> anyhow::Result<Injector> {
            Ok(Injector { held: HashSet::new() })
        }

        pub fn apply(&mut self, ev: &InputEvent) {
            match ev {
                InputEvent::Move { x, y } => {
                    // Absolute coordinates on the primary monitor (the one captured).
                    let ax = (x.clamp(0.0, 1.0) * 65535.0) as i32;
                    let ay = (y.clamp(0.0, 1.0) * 65535.0) as i32;
                    send(&[mouse(ax, ay, 0, MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE)]);
                }
                InputEvent::Button { b, d } => {
                    let (flag, data) = match (b, d) {
                        (0, true) => (MOUSEEVENTF_LEFTDOWN, 0),
                        (0, false) => (MOUSEEVENTF_LEFTUP, 0),
                        (1, true) => (MOUSEEVENTF_MIDDLEDOWN, 0),
                        (1, false) => (MOUSEEVENTF_MIDDLEUP, 0),
                        (2, true) => (MOUSEEVENTF_RIGHTDOWN, 0),
                        (2, false) => (MOUSEEVENTF_RIGHTUP, 0),
                        (3, true) => (MOUSEEVENTF_XDOWN, 1),
                        (3, false) => (MOUSEEVENTF_XUP, 1),
                        (4, true) => (MOUSEEVENTF_XDOWN, 2),
                        (4, false) => (MOUSEEVENTF_XUP, 2),
                        _ => return,
                    };
                    if *d {
                        self.held.insert((true, *b as u16, false));
                    } else {
                        self.held.remove(&(true, *b as u16, false));
                    }
                    send(&[mouse(0, 0, data, flag)]);
                }
                InputEvent::Wheel { dx, dy } => {
                    if *dy != 0.0 {
                        send(&[mouse(0, 0, (-dy * 120.0) as i32, MOUSEEVENTF_WHEEL)]);
                    }
                    if *dx != 0.0 {
                        send(&[mouse(0, 0, (dx * 120.0) as i32, MOUSEEVENTF_HWHEEL)]);
                    }
                }
                InputEvent::Key { code, d } => {
                    if let Some(k) = keys::lookup(code) {
                        if *d {
                            self.held.insert((false, k.scan, k.ext));
                        } else {
                            self.held.remove(&(false, k.scan, k.ext));
                        }
                        send(&[key(k.scan, k.ext, *d)]);
                    }
                }
                InputEvent::ReleaseAll => self.release_all(),
            }
        }

        fn release_all(&mut self) {
            for (is_mouse, code, ext) in std::mem::take(&mut self.held) {
                if is_mouse {
                    self.apply(&InputEvent::Button { b: code as u8, d: false });
                } else {
                    send(&[key(code, ext, false)]);
                }
            }
        }
    }

    impl Drop for Injector {
        fn drop(&mut self) {
            self.release_all();
        }
    }
}

// ------------------------------------------------------------ others

#[cfg(not(any(target_os = "linux", windows)))]
pub struct Injector;

#[cfg(not(any(target_os = "linux", windows)))]
impl Injector {
    pub fn new() -> anyhow::Result<Injector> {
        anyhow::bail!("{UNAVAILABLE}")
    }
    pub fn apply(&mut self, _ev: &InputEvent) {}
}
