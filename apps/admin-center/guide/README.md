# The OpenDirectory Beginner's Guide

This guide teaches you how OpenDirectory works from the ground up. It's built into the Admin Center: click **Guide** at the bottom of the sidebar, or a **Guide** link at the top of most pages, to open the chapter about what you're looking at. It works offline. You don't need any IT background. Every term is explained the first time it appears, and each chapter ends with a short "In Active Directory terms" box, so if you ever read Microsoft documentation or talk to a Windows admin, you'll know what they mean.

The first half explains the ideas. The second half walks you through doing it, click by click, from installing the app to managing your first computer.

> **Looking for how the code is built?** That's a different document: [docs/GUIDE.md](../../../docs/GUIDE.md) explains the programming side (Rust, Tauri, the repository layout). This guide is about *using* OpenDirectory.

---

## Part 1: The ideas

Read these in order. Each one builds on the one before, and each takes about five minutes.

1. [The big picture](01-big-picture.md): what OpenDirectory is for, and the four programs that make it up.
2. [The directory: domains, OUs, users and groups](02-directory.md): how you describe your organization.
3. [Group Policy: settings for many computers at once](03-group-policy.md): how you tell computers what to do.
4. [Endpoints, agents and enrollment](04-endpoints-and-agents.md): how computers join and stay in touch.
5. [Working from anywhere: the Internet Gateway and automatic updates](05-gateway-and-updates.md): laptops that leave the office, and keeping everything up to date.
6. [The person at the computer: notices, consent and the help desk](06-end-user-experience.md): what the people you manage see, and how they reach you.

## Part 2: Doing it

7. [Walkthrough: install and create your domain](07-walkthrough-setup.md)
8. [Walkthrough: enroll and manage your first computer](08-walkthrough-first-endpoint.md)
9. [Everyday tasks and fixing common problems](09-everyday-tasks.md)

## Reference

- [Glossary](glossary.md): every term in one place, with its industry name and its Active Directory equivalent.

---

## Three tips before you start

- **Turn on Learn mode.** In the Admin Center, open the menu at the bottom of the sidebar and switch on **Learn mode**. Small explanations then appear next to every technical word on screen. The **Learn** page has the full list.
- **Nothing happens to a computer until it's enrolled.** You can click around, create users and build policies without any risk. Settings only reach a computer after you install the agent on it (chapter 4).
- **Almost everything can be undone.** Deleted users, groups, OUs, policies and scripts go to the **Recycle Bin** for 180 days, and every change is written to the **Audit Log** with who made it.
