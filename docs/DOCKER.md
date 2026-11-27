# Running OpenDirectory in Docker

The Docker image runs the **Management Server** and serves the **Admin Center in your browser** (the web console). The Compose file adds **guacd**, so Windows Remote Desktop (RDP) also works in the browser. It's the easiest way to run OpenDirectory on a NAS, a home server, or any Linux host with Docker, and it works with Docker Compose and Portainer.

> The desktop Admin Center and the `od` CLI can connect to the same server. Endpoint Agents enroll against it exactly as they do with any other Management Server.

## Quick start

```bash
git clone https://github.com/Lin8x/OpenDirectory && cd OpenDirectory
docker build -t opendirectory .
docker run -d --name opendirectory --restart unless-stopped \
  -p 8443:8443 \
  -e OD_PUBLIC_HOST=od.example.lan \
  -v opendirectory-data:/var/lib/opendirectory \
  opendirectory
```

> A prebuilt image (`ghcr.io/lin8x/opendirectory`) will be published from GitHub Releases once the Docker workflow is enabled; until then, build it as above.

1. Open `https://<host>:8443`. The server makes its own certificate, so your browser warns the first time; continue, or supply your own certificate (below).
2. The setup wizard creates your domain, the first Domain Admin and a security baseline.
3. Go to **Enrollment** for the command to run on each computer.

To see the TLS fingerprint the agents and CLI pin:

```bash
docker logs opendirectory | grep fingerprint
```

## Docker Compose (recommended)

The repository has a ready [`docker-compose.yml`](../docker-compose.yml) with two services: `opendirectory` (the server and web console) and `guacd` (the RDP helper, reachable only inside the stack):

```bash
OD_PUBLIC_HOST=192.168.1.10 docker compose up -d
```

Use the address other computers will use for `OD_PUBLIC_HOST`. The compose file builds the image from the repository the first time (`docker compose build` to rebuild after pulling changes).

## Reaching the web console from another computer

1. On any computer on the same network, open `https://<OD_PUBLIC_HOST>:8443/` in a browser.
2. Accept the certificate warning the first time, or give the server your own certificate (see Settings below).
3. Sign in. Domain Admins get the full console. Accounts you've delegated devices to (**Delegation of Control**) get a console with only those devices.

From outside your network, use a VPN or a private network tool such as Tailscale or WireGuard, and open the same address. Forwarding port 8443 on your router works too, but turn on **two-factor sign-in** for every account first (**Settings > Two-factor sign-in**) and use a trusted certificate. Port 8444 (the Internet Gateway) only serves Endpoint Agents and never the web console.

## Remote desktop in the browser

Open an endpoint and pick **Low-latency stream**, **Remote Desktop (RDP)** or **VNC**. Everything travels through the agent's outbound connection, so endpoints open no inbound ports.

- **RDP** goes through `guacd`. The server connects to it with `OD_GUACD`, and guacd connects back to the server for the RDP leg at `OD_GUACD_CALLBACK`. Compose sets both. Without guacd, the RDP option is greyed out and the other two still work.
- **The low-latency stream** needs FFmpeg on the endpoint. It encodes on the endpoint's GPU (NVENC, AMF, Quick Sync, VAAPI or VideoToolbox) and falls back to software.
- **VNC** needs a VNC server on the endpoint, or Screen Sharing on a Mac.

Running the server without Docker? Run guacd next to it and point the server at it:

```bash
docker run -d --name guacd --network host guacamole/guacd:1.6.0
OD_GUACD=127.0.0.1:4822 OD_GUACD_CALLBACK=127.0.0.1 opendirectory-server run
```

## Portainer

1. In Portainer, open **Stacks** and click **Add stack**.
2. Name it `opendirectory`, choose **Repository**, and point it at this repository's URL with `docker-compose.yml` as the compose path, so Portainer can build the image. (The **Web editor** works once a prebuilt image is published.)
3. Under **Environment variables**, add `OD_PUBLIC_HOST` with the name or IP your endpoints will use.
4. Click **Deploy the stack**, then open `https://<host>:8443`.

## Settings

| Variable | Default | What it does |
| --- | --- | --- |
| `OD_PUBLIC_HOST` | detected | Host name or IP put into enrollment commands. Set it in containers, where the detected address is the container's own. |
| `OD_LISTEN` | `0.0.0.0:8443` | Address and port inside the container. |
| `OD_DATA_DIR` | `/var/lib/opendirectory` | Directory database, keys and certificate. **Keep this on a volume.** |
| `OD_TLS_CERT`, `OD_TLS_KEY` | unset | PEM certificate and key to use instead of the self-signed certificate. Mount them read-only. |
| `OD_WEB_UI` | `/usr/share/opendirectory/web` | The web Admin Center. Unset it to run API-only. |
| `OD_GUACD` | unset (`guacd:4822` in Compose) | Where guacd listens. Turns on browser RDP. |
| `OD_GUACD_CALLBACK` | `127.0.0.1` (`opendirectory` in Compose) | The name or IP guacd uses to reach this server for the RDP connection. |

## Behind a reverse proxy

Traefik, Caddy, nginx and others work. Proxy HTTPS to `https://opendirectory:8443` and either trust its certificate or give the server your own with `OD_TLS_CERT`/`OD_TLS_KEY`. Endpoint Agents pin the certificate fingerprint they see, so point agents at a name whose certificate won't change, or let them connect to port 8443 directly.

## Backups and upgrades

- Use **Backup & Restore** in the Admin Center for scheduled, encrypted backups to SFTP, S3, WebDAV and more. Backing up the `opendirectory-data` volume also works.
- To upgrade, `git pull` and `docker compose up -d --build`. The database is migrated automatically.

## Security notes

- The web console uses short sessions, locks out repeated failed sign-ins, supports two-factor sign-in (TOTP authenticator apps, optionally required by policy), and sends strict security headers (CSP, no framing).
- Delegated operators only ever see the endpoints, OUs, groups and users delegated to them, and only with the permissions you ticked. Domain Admin accounts are never in a delegated scope.
- guacd has no published port, and the one-shot connection it makes back to the server is only accepted from guacd's own address.
- Don't publish port 8443 to the internet unless you need endpoints to check in from outside. Use a VPN or restrict it at your firewall.
