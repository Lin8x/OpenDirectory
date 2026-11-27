# 6. The person at the computer: notices, consent and the help desk

[Guide home](README.md) · Previous: [The Internet Gateway and updates](05-gateway-and-updates.md) · Next: [Walkthrough: install and create your domain](07-walkthrough-setup.md)

Everything so far has been from your side, as the admin. But every endpoint has a person using it, called the **end user** in IT. Good IT is honest with them about what's managed, asks before looking at their screen, and gives them an easy way to ask for help.

OpenDirectory handles this with four GPO settings in the **End-user experience** category. Because they're GPO settings, they follow the usual rules from chapter 3: link them to the OUs you want, and use priority if two GPOs disagree. You might use stricter consent for staff laptops and none for unattended servers.

These settings need agent version 0.13.0 or newer. Older agents skip them until they update themselves (chapter 5).

---

## 1. Managed-device notice

**What it does:** shows each person who signs in a message saying the organization manages this device, and what IT can and can't see. It can ask them to press **I understand**, and records who did and when.

**Why you'd use it:** people have a right to know a computer is managed, and in many workplaces and countries you're required to tell them. It also builds trust: the default text explains that IT can see hardware, software, settings and security status, but can't read files, messages or browsing unless a remote session is opened, and that they'll be told when that happens.

**Options:**

- **Title** and **Message**: what the notice says. The default is a good starting point; edit it to match your organization.
- **Users must acknowledge**: when on, each person must press *I understand*. The endpoint's compliance shows who has acknowledged the current version of the notice. If you change the text, everyone is asked again.

**How it appears:** a message box on Windows, a dialog on macOS, a dialog on Linux desktops, and a notification plus an in-app notice on Android.

---

## 2. Remote session consent

**What it does:** decides what the signed-in person sees before you open a **remote shell** (a command window on their computer) or **remote desktop** (seeing and controlling their screen).

**Options for Mode:**

| Mode | What the person sees |
| --- | --- |
| **Ask the user to allow it** (default) | A prompt with Allow and Deny. The session only starts if they allow it. |
| **Tell the user it started** | A notification that IT has connected. No prompt. |
| **No prompt or notice** | Nothing. Use this only for computers nobody sits at, such as servers. |

**Allow when nobody is signed in** decides what happens when there's no one to ask, for example a server, or a desktop at night. When it's on, you can connect anyway. When it's off, sessions are refused until someone signs in and allows it.

---

## 3. Help desk chat

**What it does:** lets people message IT from their computer and get replies, like a small chat app.

**How it works for them:** on Windows a **Contact IT** shortcut appears in the Start menu, on macOS in Applications, and on Linux in the applications menu. It opens a chat page on their own computer. On Android it's the Help screen in the app. Nothing extra runs in the background.

**How it works for you:** conversations show up on the **Help Desk** page in the Admin Center, and on the **Help desk** tab of each endpoint. You reply from there, and can open a remote session from the same endpoint page if you need to look.

**Options:** **Other ways to reach IT** lets you add a phone number or email shown alongside the chat.

You can also send a one-way pop-up to a computer at any time with **Message user** on its endpoint page, for example "Please save your work, this PC restarts at 6 PM."

---

## 4. Remote tools

**What it does:** turns the remote shell and remote desktop on or off for the computers the GPO applies to. The agent enforces it too, so a computer with remote desktop off refuses remote desktop sessions even if someone tries.

**Why you'd use it:** to switch remote control off entirely for sensitive computers, such as a manager's laptop or a computer that handles payments, while keeping it on everywhere else.

**Options:** **Remote shell** and **Remote desktop**, each on or off.

---

## A sensible starting point

For a typical office:

- **Default Endpoint Policy** (linked to the whole domain): managed-device notice with *Users must acknowledge* on, help desk chat on, remote session consent set to *Ask the user*.
- **Servers** OU: remote session consent set to *No prompt or notice*, with *Allow when nobody is signed in* on, at a lower priority number so it wins on servers.

---

> **In Active Directory terms**
>
> | OpenDirectory setting | Closest Microsoft equivalent |
> | --- | --- |
> | Managed-device notice | Interactive logon message; the Intune enrollment privacy notice |
> | Remote session consent | "Set rules for remote control of Remote Desktop Services user sessions" (Full Control with user's permission) |
> | Help desk chat | Company Portal's Contact IT / help desk page |
> | Remote tools | "Allow users to connect remotely using Remote Desktop Services" and the Remote Assistance policies |
