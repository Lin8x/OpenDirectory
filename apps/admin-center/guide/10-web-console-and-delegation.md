# 10. The web console, delegated access and remote desktop

[Guide home](README.md) · Previous: [Everyday tasks](09-everyday-tasks.md) · Next: [Glossary](glossary.md)

This chapter covers three things that go together:

1. **The web console**: the Admin Center in any web browser, on any computer, without installing anything.
2. **Delegation of Control**: accounts that can manage only the devices you give them, and nothing else.
3. **Remote desktop in the browser**: see and use a computer's screen from a browser tab. That includes a low-latency mode fast enough for games and apps like Photoshop.

---

## The web console

Your Management Server serves the Admin Center to web browsers as well as to the desktop app. It's the same screens, the same sign-in and the same rules.

**Where is it?** Use the address your endpoints already use, with a slash on the end. It's shown in **Settings > Connect another Admin Center** as **Web console**. For example:

```
https://192.168.1.10:8443/
```

This works wherever the server runs:

- **In the desktop Admin Center**, if you chose **Host on this computer**. Leave the app running (it can sit in the system tray), and other computers on your network can open the address above.
- **As a service** (`opendirectory-server install-service`), when the `web` folder sits next to the program.
- **In Docker**, with the included `docker-compose.yml` (see [docs/DOCKER.md](../../../docs/DOCKER.md)).

**The certificate warning.** The server makes its own certificate, so the first time you open the page your browser warns you. Check that the address is right and continue. To get rid of the warning for good, give the server a certificate your browsers trust (`--tls-cert` and `--tls-key`, or `OD_TLS_CERT` and `OD_TLS_KEY` in Docker).

**From outside your network.** The safest way is a VPN, or a private network tool such as Tailscale or WireGuard. Then you open the same address as you would at home. You *can* forward port 8443 on your router, but if you do, turn on two-factor sign-in for everyone first (below) and use a real certificate. The **Internet Gateway** (port 8444, chapter 5) is a separate door that only serves Endpoint Agents. It never serves the web console, so opening it doesn't expose sign-in.

## Two-factor sign-in

Two-factor sign-in (also called MFA, for *multi-factor authentication*) asks for a six-digit code from an app on your phone, as well as your password. Someone who steals your password still can't sign in.

- **Turn it on for yourself:** **Settings > Two-factor sign-in > Set up an authenticator app**. Scan the QR code with Microsoft Authenticator, Google Authenticator, 1Password, Bitwarden, Aegis or any other code app, then type the code it shows.
- **Require it:** in the same card, Domain Admins choose who must use it: **Delegated operators** or **Every account**. Anyone who hasn't set it up is asked to do so at their next sign-in, before they can do anything else.
- **Lost phone:** a Domain Admin opens the user and clicks **Reset two-factor**. If it's your own account and you can't sign in at all, run `opendirectory-server admin reset-mfa <user>` on the server itself.

## Delegation of Control

Not everyone who needs to help should be a Domain Admin. Delegation gives an account (or every member of a group) **some permissions over some objects**, for example:

- *Alex can use remote desktop on the gaming PC.*
- *A technician can reset passwords and chat with users in the Branch Office OU, for one week.*

Open **Delegation of Control** (under Directory) and click **Delegate control**:

1. **Delegate to:** a user or a security group. Domain Admins already have full control, so they aren't offered.
2. **Over these objects:** single endpoints, OUs or security groups. An OU covers the computers and user accounts in it, and in its child OUs if you leave **child OUs** ticked. A group covers its member computers and users.
3. **Permissions:** tick what they may do, or start from a template (**Remote desktop user**, **Help desk operator**, **Endpoint operator**, **Read-only**).
4. **Access ends:** **Never**, or after an hour, a day, a week and so on. That makes it a temporary admin account. It switches itself off at that time.

### What a delegated operator sees

They sign in at the same web console (or desktop app) with their own username and password. They get a simpler console with **My devices** and, if you allowed it, **Users** and **Help Desk**. Everything outside their scope is hidden. They can't see other computers, other users, Group Policy, the audit log or settings. Even if they type an address by hand, the server answers as if the object doesn't exist. Accounts of Domain Admins are never in their scope, so they can't reset an admin's password.

Turning a delegation off, deleting it or letting it expire ends access at once. The next thing they click signs them out. Everything they do is in the **Audit Log** under their own name.

| Permission | What it allows |
| --- | --- |
| View endpoints | See the device, its details and its job history |
| Remote desktop | Open a remote desktop session (stream, RDP or VNC) |
| Remote shell | Open a command line on the device |
| Endpoint actions | Refresh inventory, restart the agent, updates, messages |
| Run scripts | Run scripts on the device |
| Read LAPS password | See and rotate the local admin password |
| Help desk chat | Chat with the person at the device |
| View users / Reset passwords | See accounts in scope, and reset their passwords |
| Manage services, software, processes, containers | The matching tabs on the device |

## Remote desktop in the browser

Open a device (or **My devices** for delegated operators) and choose a way to connect:

| Option | Best for | What the endpoint needs |
| --- | --- | --- |
| **Low-latency stream** | Games, video, Photoshop and other creative apps | FFmpeg installed. A GPU encoder is used automatically when there is one. |
| **Remote Desktop (RDP)** | Everyday Windows work, a separate sign-in session | Remote Desktop turned on (use the Remote Desktop GPO setting). The server needs the `guacd` helper, which the Docker Compose file includes. |
| **VNC** | Macs (Screen Sharing) and Linux desktops | A VNC server, or Screen Sharing on a Mac |

All three travel through the agent's own outbound connection, so the endpoint never opens a port to the network. The **Remote tools** and **consent** GPO settings from chapter 6 still apply. If the person at the computer must approve sessions, the picture appears once they click **Allow**.

### The low-latency stream

The agent captures the screen and compresses it into video on the computer's graphics card. That's the same idea as game-streaming tools. The browser then plays that video and sends your keyboard and mouse back.

- **Encoders:** NVIDIA (NVENC), AMD (AMF on Windows, VAAPI on Linux), Intel (Quick Sync or VAAPI) and Apple (VideoToolbox), with a software encoder as the fallback. **Automatic** picks the first one that actually works on that computer.
- **Quality:** pick a preset, from *Office work* (4 Mbit/s) to *Competitive games* (native resolution at 120 frames per second). The bar above the picture shows the encoder, frame rate and bandwidth live.
- **Game mode** locks the mouse to the stream so games that turn the camera with the mouse work. Press **Esc** to get your mouse back. **Full screen** also captures keys like Esc and the Windows key in Chrome and Edge.
- **Install FFmpeg on the endpoint first.** On Windows: `winget install --id Gyan.FFmpeg -e --scope machine`. On Linux, install the `ffmpeg` package. On a Mac, `brew install ffmpeg`.
- **Limits today:** no sound yet. On Windows and Linux the main screen is streamed, and you control it with keyboard and mouse. On a Mac, the stream is view only for now. Use **VNC** to control a Mac.

> **In Active Directory terms:** the web console is like Windows Admin Center. Delegation of Control is the same idea as AD's *Delegation of Control Wizard*, with OU-scoped permissions, plus time-limited access like Privileged Access Management. Never including Domain Admins in a delegated scope matches AdminSDHolder. Browser RDP works like Remote Desktop Web Access, through a gateway.
