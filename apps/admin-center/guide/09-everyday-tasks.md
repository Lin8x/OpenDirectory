# 9. Everyday tasks and fixing common problems

[Guide home](README.md) · Previous: [Walkthrough: your first computer](08-walkthrough-first-endpoint.md) · Next: [Glossary](glossary.md)

Short recipes for the things you'll do most often, then the problems people hit most and how to fix them.

---

## Everyday tasks

### Someone new joins

1. **Users > New user.** Put them in the right OU and tick their security groups (for example *Staff* and their department).
2. That's it. Every computer whose GPOs give that group sign-in accounts creates their account at its next check-in.

### Someone leaves

1. Open the user and click **Disable**. They can no longer sign in anywhere, and their account is disabled on every computer.
2. Later, if you're sure, delete them. They stay in the **Recycle Bin** for 180 days, with their groups and password, in case you need them back.

### Someone forgot their password

Open the user and click **Reset password**. You can set a new one, or give them a one-time code they redeem with **Change password** on a sign-in screen. The dialog shows which password policy applies to them.

### Give a team admin rights on their own computers

Create a GPO with a **Sign-in accounts** setting for that team's group, tick **Make them local administrators**, and link it to the OU their computers are in.

### Move a computer to a different OU

Open the endpoint and change **Organizational unit** in the **Placement** card. Its GPOs change at the next check-in.

### Apply different settings to some computers

1. Create an OU for them (for example *Kiosks*) and move the computers into it.
2. Create a GPO with the settings, link it to that OU, and give it a lower priority number than the GPOs it should beat.
3. Check one computer's **Policy & compliance** tab to confirm it got what you expected.

### Run something on many computers

**Script Library:** pick or write a script, click **Run…**, and target computers, a security group or an OU. To run it automatically, add a **Scripts (startup, sign-in, scheduled)** setting to a GPO with a trigger: at startup, at sign-in, on a schedule, or once.

### Install operating system updates

**Patch Management** lists pending updates on every computer, including security updates and whether a restart is needed. Select computers, a group or an OU and install them.

### Back up everything

**Backup & Restore** saves the whole directory, settings and keys into one encrypted `.odbackup` file, on a schedule, to a folder, network share or cloud storage. Set one up as soon as you depend on OpenDirectory, and keep the passphrase somewhere safe: without it, the backup can't be opened.

### Find anything fast

Press **Ctrl K** (or **Cmd K** on a Mac) to search users, groups, computers, GPOs and pages from anywhere.

---

## Fixing common problems

### A computer doesn't appear after enrolling

- **Check the command ran as administrator.** On Windows, the PowerShell window title must say *Administrator*. On Mac and Linux, the command uses `sudo`.
- **Check the computer can reach the server.** It needs the address shown under **This Management Server** on the Enrollment page. Try opening that address in a browser on the computer: a certificate warning is fine and means it's reachable; a timeout means it isn't.
- **Check the firewall on the server computer** allows incoming connections on port **8443**.
- **Check the token** is still **Active** (not expired or used up) in the **Tokens** list.

### A computer shows as offline

- It may simply be off or asleep. Agents check in every 15 seconds while the computer is on.
- If it's a laptop that left the office, set up the [Internet Gateway](05-gateway-and-updates.md).
- On the computer, check the service is running: **Services** on Windows (*OpenDirectory Endpoint Agent*), `sudo launchctl list | grep opendirectory` on macOS, `systemctl status opendirectory-agent` on Linux.

### A setting says Non-compliant or Failed

Read the message on the line in **Policy & compliance**. It says what the agent found or what went wrong. Common cases:

- **Disk isn't encrypted:** turn on BitLocker or FileVault yourself. OpenDirectory reports encryption but doesn't enable it.
- **Unsupported:** that operating system has no equivalent. Use the platform filter on the GPO if you don't want to see it.
- **Pending:** the agent hasn't reported on it yet, usually because it's offline or older than the setting needs.

### A user can't sign in to a computer

- Is the user **enabled**, and in the group the **Sign-in accounts** setting uses?
- Is that GPO **enabled** and **linked** to the computer's OU (or to all endpoints)? Check the computer's **Policy & compliance** tab for a *Sign-in accounts* line.
- Has the computer checked in since you made the change? It needs to be online.
- On Windows, the username can't be longer than 20 characters.

### The Admin Center says it can't reach the Management Server

If the directory is hosted on this computer, the Admin Center may have been closed from the tray, or the computer restarted without **Launch at startup** on. Open the Admin Center again. If the server is on another machine, check that machine is on and reachable, then click **Try again**.

### I forgot my Domain Admin password

On the sign-in screen, click **Forgot password?**. Your computer asks for its own administrator password, then you choose the account and set a new password. On a dedicated server, run `sudo opendirectory-server admin reset-password <username>` on the server itself. The [README](../../../README.md#features) has details under *Sign-in and break-glass recovery*.

---

## Where to go next

- The **Learn** page in the Admin Center and the [glossary](glossary.md) explain every term.
- [docs/ARCHITECTURE.md](../../../docs/ARCHITECTURE.md) explains how it all works under the hood.
- [docs/ROADMAP.md](../../../docs/ROADMAP.md) lists what's coming and what's still experimental.
