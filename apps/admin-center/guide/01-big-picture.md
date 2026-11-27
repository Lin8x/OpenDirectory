# 1. The big picture

[Guide home](README.md) · Next: [The directory](02-directory.md)

## The problem OpenDirectory solves

Imagine you look after ten computers: a few Windows laptops, a Mac, and a Linux server. Each one has its own list of people who can sign in, its own password rules, its own firewall setting and its own updates. When someone joins, you walk to every computer and create their account. When someone leaves, you have to remember every computer they could use. When you want screens to lock after ten minutes, you change that setting ten times.

That works for three computers. It falls apart at ten, and it's impossible at a hundred.

The solution IT departments use is called **centralized management**: you describe the people, the computers and the rules *once*, in one place, and the computers fetch their instructions from there and apply them on their own. In the Windows world, the product that does this is **Active Directory** (AD), which runs on Windows Server. OpenDirectory does the same everyday job, but it also works on Windows Home, macOS, Linux and Android, and it doesn't need Windows Server.

## The four programs

OpenDirectory is made of four programs that work together. Picture an office building:

| Program | What it is | The office analogy |
| --- | --- | --- |
| **Admin Center** | The app you click around in. You create users, write rules and look at your computers here. | Your desk and the forms you fill in. |
| **Management Server** | The program that stores everything (people, computers, rules) and hands out instructions. | The filing cabinet and the mailroom. |
| **Endpoint Agent** | A small background program installed on every computer you manage. It collects instructions and carries them out. | A staff member in each room who reads the memos and acts on them. |
| **`od` command** (CLI) | A text command that does everything the Admin Center does, for people who prefer typing or want to automate things. | Sending memos by typing instead of filling in forms. |

**Endpoint** is just the industry word for "a computer or phone that's being managed". A laptop, a desktop, a server and an Android phone are all endpoints.

**CLI** stands for *command-line interface*: a program you use by typing commands into a terminal window.

### Where each program runs

```
        ┌──────────────────────┐
        │     Admin Center     │   you make changes here
        └──────────┬───────────┘
                   ▼
        ┌──────────────────────┐
        │  Management Server   │   stores everything
        └──────────────────────┘
          ▲        ▲        ▲      agents fetch instructions
          │        │        │      and report back
   ┌──────┴───┐ ┌──┴─────┐ ┌┴──────────┐
   │  Agent   │ │ Agent  │ │   Agent   │
   │ Windows  │ │ Mac    │ │   Linux   │
   │ laptop   │ │        │ │   server  │
   └──────────┘ └────────┘ └───────────┘
```

The simplest setup, and the one this guide uses, puts the Admin Center and the Management Server on the **same computer**. When you install the Admin Center and choose **Host the directory on this computer**, the Management Server runs quietly inside it. You don't install anything else.

Bigger organizations run the Management Server on its own dedicated machine (a *server*, which is just a computer that stays on and serves other computers), and connect to it from the Admin Center on their own laptop. Both setups work exactly the same way once they're running.

## What happens, in one paragraph

You create a **user** for each person and a **security group** for each team. You write **Group Policy Objects** (GPOs), which are named bundles of rules like "lock the screen after 10 minutes" or "give everyone in the Staff group an account on this computer". You attach each GPO to a folder of computers called an **OU**. Then you install the **agent** on each computer. Every 15 seconds the agent asks the Management Server, "What should I be doing?", gets back the rules that apply to it, applies them using that operating system's own tools, and reports whether it worked. You see the results in the Admin Center.

The next chapters take each of those words one at a time.

## Why the agent always calls home

One detail matters a lot for security and for home networks: **the agent always starts the conversation**. The Management Server never reaches out to a computer. This means managed computers don't need any open ports (no holes in their firewall), and they work behind a home router, on hotel Wi-Fi, or anywhere else that can reach the server.

The connection is encrypted, and the agent checks it's talking to *your* server and not an impostor. Chapter 4 explains how.

> **In Active Directory terms**
>
> - The **Management Server** plays the role of a **domain controller** (DC): the Windows Server that holds the directory.
> - The **Admin Center** combines **Active Directory Users and Computers** (ADUC) and the **Group Policy Management Console** (GPMC).
> - The **Endpoint Agent** does the job of the **Group Policy client** and its **client-side extensions** built into Windows, plus some of what an **RMM** tool (remote monitoring and management) does.
> - The **`od` CLI** is like the **ActiveDirectory PowerShell module**.
