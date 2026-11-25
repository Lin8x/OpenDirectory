# Running OpenDirectory in Docker

The Docker image runs the **Management Server** and serves the **Admin Center in your browser** (web mode). It's the easiest way to run OpenDirectory on a NAS, a home server, or any Linux host with Docker, and it works with Docker Compose and Portainer.

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

## Docker Compose

The repository has a ready [`docker-compose.yml`](../docker-compose.yml):

```bash
OD_PUBLIC_HOST=od.example.lan docker compose up -d
```

The compose file builds the image from the repository the first time (`docker compose build` to rebuild after pulling changes).

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

## Behind a reverse proxy

Traefik, Caddy, nginx and others work. Proxy HTTPS to `https://opendirectory:8443` and either trust its certificate or give the server your own with `OD_TLS_CERT`/`OD_TLS_KEY`. Endpoint Agents pin the certificate fingerprint they see, so point agents at a name whose certificate won't change, or let them connect to port 8443 directly.

## Backups and upgrades

- Use **Backup & Restore** in the Admin Center for scheduled, encrypted backups to SFTP, S3, WebDAV and more. Backing up the `opendirectory-data` volume also works.
- To upgrade, `git pull` and `docker compose up -d --build`. The database is migrated automatically.

## Security notes

- The web Admin Center only accepts Domain Admins, uses short sessions, locks out repeated failed sign-ins, and sends strict security headers (CSP, no framing).
- Don't publish port 8443 to the internet unless you need endpoints to check in from outside. Use a VPN or restrict it at your firewall.
