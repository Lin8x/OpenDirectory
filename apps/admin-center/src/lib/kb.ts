// Knowledge Base: one entry per service or concept, in three views:
// the open standard (what it is), how OpenDirectory implements it, and the
// Microsoft / Active Directory product that does the same job. Every link and
// video was checked when it was added (videos through YouTube's oEmbed API).

export interface KbLink { label: string; url: string }
export interface KbVideo { title: string; channel: string; url: string }
export interface KbEntry {
  key: string;
  title: string;
  category: "Directory & identity" | "Network services" | "Security" | "Endpoint management";
  summary: string;
  /** The technology itself, independent of any product. */
  standard: { text: string[]; wikipedia: KbLink; video: KbVideo; links: KbLink[] };
  /** How OpenDirectory provides it. */
  ours: { text: string[]; to?: string; links: KbLink[] };
  /** How Windows Server does it differently: Microsoft Learn docs and a Windows how-to video. */
  microsoft?: { name: string; text: string[]; learn: KbLink; video: KbVideo };
}

const yt = (id: string) => `https://www.youtube.com/watch?v=${id}`;
const wiki = (title: string): KbLink => ({ label: title.replace(/_/g, " "), url: `https://en.wikipedia.org/wiki/${title}` });
const ms = (label: string, path: string): KbLink => ({ label, url: `https://learn.microsoft.com/en-us/${path}` });
const rfc = (n: number, label: string): KbLink => ({ label: `RFC ${n}: ${label}`, url: `https://www.rfc-editor.org/rfc/rfc${n}` });

export const KB: KbEntry[] = [
  {
    key: "directory", title: "Directory services", category: "Directory & identity",
    summary: "A central database of users, groups and computers that everything else trusts.",
    standard: {
      text: [
        "A directory service stores identities (people, groups, computers) and their attributes in one place, so you create an account once and use it everywhere: sign-in, Wi-Fi, VPN, file shares and apps.",
        "Objects live in a tree. Organizational units (OUs) are folders in that tree; they mirror your organization and are where you link policy.",
        "Security groups collect users so you grant access to the group instead of to each person. Good practice: grant access to groups only, never to individual users.",
      ],
      wikipedia: wiki("Directory_service"),
      video: { title: "Active Directory Explained in 5 minutes", channel: "Connected Cookie", url: yt("OfXJlmuoc20") },
      links: [{ label: "X.500 directory standards (Wikipedia)", url: "https://en.wikipedia.org/wiki/X.500" }],
    },
    ours: {
      text: [
        "Users, Security Groups and Organizational Units are the directory. GPOs, scripts, RADIUS, LDAP and the Samba domain controller all read from it.",
        "Deleted objects go to the Recycle Bin for 180 days with their passwords and memberships, and fine-grained password policies apply per group.",
      ],
      to: "/users",
      links: [],
    },
    microsoft: {
      name: "Active Directory Domain Services (AD DS)",
      text: [
        "AD DS is the directory in Windows Server. A domain is a security boundary with its own database (ntds.dit) held by every domain controller; domains form trees and forests that trust each other.",
        "Domain controllers replicate changes to each other (multi-master), using sites and site links to plan traffic over WAN links. Five FSMO roles, such as the PDC emulator and RID master, are held by one DC each.",
        "Admins work in Active Directory Users and Computers (ADUC), Active Directory Administrative Center and the ActiveDirectory PowerShell module.",
      ],
      video: { title: "Install Active Directory & Promote Windows Server 2022 to Domain Controller", channel: "HKTECH-HUB", url: yt("6BZBqb-5tLI") },
      learn: ms("AD DS overview (Microsoft Learn)", "windows-server/identity/ad-ds/get-started/virtual-dc/active-directory-domain-services-overview"),
    },
  },
  {
    key: "dns", title: "DNS", category: "Network services",
    summary: "Turns names like od.corp.example.com into IP addresses.",
    standard: {
      text: [
        "DNS (Domain Name System) is the internet's phone book. A client asks a resolver for a name, and the resolver walks the hierarchy (root, then .com, then example.com) until an authoritative server answers.",
        "Record types to know: A (IPv4 address), AAAA (IPv6), CNAME (alias), MX (mail), TXT (verification and policy), SRV (where a service runs), PTR (reverse lookup).",
        "Inside a company, an internal DNS server is authoritative for the domain (corp.example.com) and forwards everything else to an upstream resolver. Clients learn the DNS server from DHCP.",
      ],
      wikipedia: wiki("Domain_Name_System"),
      video: { title: "How a DNS Server (Domain Name System) works", channel: "PowerCert Animated Videos", url: yt("mpQZVYPuDGU") },
      links: [rfc(1034, "Domain names, concepts and facilities"), { label: "Cloudflare Learning: What is DNS?", url: "https://www.cloudflare.com/learning/dns/what-is-dns/" }],
    },
    ours: {
      text: [
        "Network Services > DNS is an authoritative server for your domain. It adds records for the Management Server, every endpoint and every DHCP client automatically, and forwards other lookups upstream over UDP and TCP.",
        "When the Domain Controller role is on, Samba runs the domain's DNS instead, so turn OpenDirectory's DNS off there.",
      ],
      to: "/network-services",
      links: [],
    },
    microsoft: {
      name: "Windows Server DNS",
      text: [
        "The DNS Server role is normally installed on every domain controller. Zones can be AD-integrated, which stores them in the directory so they replicate with it and accept secure dynamic updates from domain members.",
        "Active Directory depends on DNS: computers find domain controllers through SRV records such as _ldap._tcp.dc._msdcs.<domain>, so broken DNS means broken sign-in and Group Policy.",
        "It's managed in DNS Manager or with the DnsServer PowerShell module, and supports conditional forwarders, scavenging of stale records and DNSSEC.",
      ],
      video: { title: "Install and Configure DNS Server in Windows Server 2022", channel: "IT-Tutorials", url: yt("9qd_3Oniowk") },
      learn: ms("DNS in Windows Server (Microsoft Learn)", "windows-server/networking/dns/dns-overview"),
    },
  },
  {
    key: "dhcp", title: "DHCP", category: "Network services",
    summary: "Hands out IP addresses and network settings automatically.",
    standard: {
      text: [
        "DHCP (Dynamic Host Configuration Protocol) gives a device its IP address, subnet mask, default gateway and DNS servers when it joins the network. The exchange is Discover, Offer, Request, Acknowledge (DORA).",
        "A scope is the range of addresses a server hands out on one subnet. A reservation always gives the same address to one MAC address, useful for printers and servers.",
        "DHCP uses broadcasts, which don't cross routers. To serve several VLANs from one server, turn on DHCP relay (\"ip helper-address\") on the router.",
      ],
      wikipedia: wiki("Dynamic_Host_Configuration_Protocol"),
      video: { title: "DHCP Explained - Dynamic Host Configuration Protocol", channel: "PowerCert Animated Videos", url: yt("e6-TaH5bkjo") },
      links: [rfc(2131, "Dynamic Host Configuration Protocol")],
    },
    ours: {
      text: [
        "Network Services > DHCP has scopes, reservations, relay support and a lease table, and registers each client's name in OpenDirectory's DNS.",
        "It's off by default so it never clashes with the DHCP server in your router. Only run one DHCP server per subnet.",
      ],
      to: "/network-services",
      links: [],
    },
    microsoft: {
      name: "Windows Server DHCP",
      text: [
        "The DHCP Server role hands out leases from scopes with options such as 003 (router), 006 (DNS servers) and 015 (DNS domain name). In a domain, a DHCP server must be authorized in Active Directory before it will serve clients, which stops rogue servers.",
        "Two servers can share scopes with DHCP failover (load balance or hot standby). The server can also register clients in DNS on their behalf.",
      ],
      video: { title: "Windows Server 2025: Deploy DHCP Server, Configure DHCP Scope & Options", channel: "ITOpsTalk (Microsoft)", url: yt("cZSJlE4V1pI") },
      learn: ms("DHCP in Windows Server (Microsoft Learn)", "windows-server/networking/technologies/dhcp/dhcp-top"),
    },
  },
  {
    key: "ldap", title: "LDAP", category: "Directory & identity",
    summary: "The standard protocol apps use to read and search a directory.",
    standard: {
      text: [
        "LDAP (Lightweight Directory Access Protocol) lets an application look up users and groups, and check a password by binding as the user. Many apps, NAS boxes, printers and firewalls support it.",
        "Every object has a distinguished name (DN) such as CN=Alice,OU=Staff,DC=corp,DC=example,DC=com. Searches start from a base DN and use filters like (&(objectClass=user)(sAMAccountName=alice)).",
        "Plain LDAP (port 389) sends passwords in clear text. Use LDAPS (port 636) or StartTLS so binds are encrypted.",
      ],
      wikipedia: wiki("Lightweight_Directory_Access_Protocol"),
      video: { title: "What is LDAP (Lightweight Directory Access Protocol)?", channel: "CBT Nuggets", url: yt("vy3e6ekuqqg") },
      links: [rfc(4511, "LDAP: The Protocol")],
    },
    ours: {
      text: [
        "Network Services > LDAP is a read-only LDAP and LDAPS server with an Active Directory-style schema (sAMAccountName, memberOf, userPrincipalName), so apps that support AD can bind to OpenDirectory without changes.",
        "It's read-only on purpose: manage objects in the Admin Center, and let apps only look them up.",
      ],
      to: "/network-services",
      links: [],
    },
    microsoft: {
      name: "AD DS over LDAP",
      text: [
        "Every Active Directory domain controller is an LDAP server, and the global catalog (ports 3268/3269) answers forest-wide searches. Tools like ADSI Edit and ldp.exe talk to it directly.",
        "Microsoft recommends requiring LDAP signing and channel binding, and newer Windows versions sign or encrypt LDAP by default.",
      ],
      video: { title: "Configure Secure LDAP (LDAPS) Using AD CS Certificates", channel: "Labs Hands On", url: yt("yc_ROKH89Xo") },
      learn: ms("LDAP signing for Active Directory (Microsoft Learn)", "windows-server/identity/ad-ds/ldap-signing"),
    },
  },
  {
    key: "kerberos", title: "Kerberos", category: "Directory & identity",
    summary: "Ticket-based single sign-on.",
    standard: {
      text: [
        "Kerberos lets a user prove who they are once and then reach many services without sending their password again. A trusted Key Distribution Center (KDC) issues tickets.",
        "At sign-in the user gets a Ticket Granting Ticket (TGT). To reach a service, the client trades the TGT for a service ticket for that service's principal name (SPN).",
        "Kerberos needs clocks within about 5 minutes of each other and working DNS.",
      ],
      wikipedia: wiki("Kerberos_(protocol)"),
      video: { title: "Kerberos Explained (In 3 Levels Of Detail)", channel: "VbScrub", url: yt("snGeZlDQL2Q") },
      links: [rfc(4120, "The Kerberos Network Authentication Service (V5)"), { label: "MIT Kerberos documentation", url: "https://web.mit.edu/kerberos/krb5-latest/doc/" }],
    },
    ours: {
      text: [
        "The Domain Controller role runs Samba's KDC, so domain-joined Windows, macOS and Linux computers get Kerberos tickets for directory accounts.",
        "Without the DC role, endpoints use local accounts that the agent keeps in sync with the directory instead.",
      ],
      to: "/domain-controller",
      links: [],
    },
    microsoft: {
      name: "Kerberos in Windows",
      text: [
        "Every AD domain controller runs the KDC service, and the krbtgt account's key signs every TGT. Windows prefers Kerberos and falls back to NTLM when it can't get a ticket.",
        "Misconfigured service accounts (weak passwords on accounts with SPNs, unconstrained delegation) are a common attack path, so audit them and prefer group managed service accounts.",
      ],
      video: { title: "Configure Kerberos Policy Settings in Active Directory", channel: "MSFT WebCast", url: yt("mBjvUsFiXUs") },
      learn: ms("Kerberos authentication overview (Microsoft Learn)", "windows-server/security/kerberos/kerberos-authentication-overview"),
    },
  },
  {
    key: "gpo", title: "Group Policy", category: "Endpoint management",
    summary: "Central configuration for every computer, linked to OUs.",
    standard: {
      text: [
        "Central configuration management describes how computers should be configured (password rules, screen lock, firewall, software, scripts) and applies it automatically, again and again, so settings don't drift.",
        "Policies are targeted at groups of machines. More specific policies override general ones, and the merged result on one machine is what you check when something doesn't apply.",
      ],
      wikipedia: wiki("Configuration_management"),
      video: { title: "Active Directory GPO Tutorial for Beginners", channel: "The Cloud Threat Defender", url: yt("AX9TX7c90MA") },
      links: [{ label: "CIS Benchmarks (hardening baselines)", url: "https://www.cisecurity.org/cis-benchmarks" }],
    },
    ours: {
      text: [
        "OpenDirectory GPOs describe intents, such as locking the screen after 10 minutes. The agent turns each one into the native setting on Windows (including Home), macOS and Linux.",
        "GPOs link to OUs, child OUs inherit, the lower priority number wins, and each endpoint shows its Resultant Set of Policy with compliance per setting. GPOs can also run scripts at startup, at sign-in or on a schedule.",
      ],
      to: "/gpos",
      links: [],
    },
    microsoft: {
      name: "Group Policy (GPMC)",
      text: [
        "GPOs are stored in AD and in SYSVOL on every domain controller, and linked to sites, domains and OUs in the Group Policy Management Console. Processing order is Local, Site, Domain, OU (LSDOU), with Enforced and Block Inheritance as overrides.",
        "Clients refresh policy every 90 minutes or so (gpupdate forces it) and gpresult shows the result. Windows Home can't join a domain, so it gets no domain Group Policy.",
      ],
      video: { title: "How to Create and Configure OUs & GPOs in Active Directory", channel: "Love Story With Technology", url: yt("lvyOHAVOmEg") },
      learn: ms("Group Policy overview (Microsoft Learn)", "windows-server/identity/ad-ds/manage/group-policy/group-policy-overview"),
    },
  },
  {
    key: "pki", title: "Certificates and PKI", category: "Security",
    summary: "A certificate authority vouches for servers, users and devices.",
    standard: {
      text: [
        "Public key infrastructure (PKI) binds a public key to a name with a certificate signed by a certificate authority (CA). Anyone who trusts the CA's root certificate trusts what it signs.",
        "Internal certificates secure intranet sites, LDAPS, RADIUS (EAP-TLS) and device identity. Keep the root CA key safe, keep certificate lifetimes short, and publish a revocation list (CRL).",
        "Auto-enrollment means each device creates its own key, sends a certificate signing request (CSR) and renews before expiry, so nobody copies private keys around.",
      ],
      wikipedia: wiki("Public_key_infrastructure"),
      video: { title: "PKI Hierarchy", channel: "ITFreeTraining", url: yt("n_2RvlSpWEU") },
      links: [rfc(5280, "X.509 certificate and CRL profile"), { label: "Cloudflare Learning: What is an SSL certificate?", url: "https://www.cloudflare.com/learning/ssl/what-is-an-ssl-certificate/" }],
    },
    ours: {
      text: [
        "Network Services > Certificate Authority creates a root CA for your domain, issues server and client certificates or signs CSRs, revokes them and publishes a CRL.",
        "The Trusted root certificate GPO setting makes endpoints trust it, and Machine certificate auto-enrollment has each endpoint request and renew its own certificate. On Windows the certificate is saved as files and not yet imported into the certificate store.",
      ],
      to: "/network-services",
      links: [],
    },
    microsoft: {
      name: "Active Directory Certificate Services (AD CS)",
      text: [
        "AD CS runs enterprise CAs that publish into Active Directory. Certificate templates define what each certificate is for and who may enroll, and a Group Policy setting turns on auto-enrollment for users and computers.",
        "A typical design has an offline root CA and an online issuing CA. Overly permissive templates are a well-known attack path, so review template permissions.",
      ],
      video: { title: "Install & Configure AD CS on Windows Server 2022", channel: "Digital IT Lab", url: yt("C_yyFuRSey0") },
      learn: ms("AD CS overview (Microsoft Learn)", "windows-server/identity/ad-cs/active-directory-certificate-services-overview"),
    },
  },
  {
    key: "radius", title: "RADIUS and 802.1X", category: "Network services",
    summary: "One sign-in for Wi-Fi, VPN and switch ports.",
    standard: {
      text: [
        "RADIUS is how network devices (Wi-Fi controllers, VPN gateways, switches) ask a central server \"is this user allowed in?\". The device is the RADIUS client and shares a secret with the server.",
        "802.1X is port-based access control: nobody gets on the wired or wireless network until RADIUS says yes. WPA2/WPA3-Enterprise Wi-Fi is 802.1X over Wi-Fi.",
        "The EAP method decides how the user proves themselves: PEAP/MS-CHAPv2 uses a password, EAP-TLS uses a certificate on the device (the strongest option).",
      ],
      wikipedia: wiki("RADIUS"),
      video: { title: "How 802.1X works", channel: "CBT Nuggets", url: yt("SdC3ppnanAE") },
      links: [rfc(2865, "RADIUS"), wiki("IEEE_802.1X"), { label: "FreeRADIUS documentation", url: "https://www.freeradius.org/documentation/" }],
    },
    ours: {
      text: [
        "Network Services > RADIUS supports PAP and MS-CHAPv2 for VPNs and network devices. Each RADIUS client can be limited to members of a security group.",
        "EAP (needed for WPA2/3-Enterprise Wi-Fi) isn't supported yet; use it for VPNs and switch or firewall admin sign-in today.",
      ],
      to: "/network-services",
      links: [],
    },
    microsoft: {
      name: "Network Policy Server (NPS)",
      text: [
        "NPS is Microsoft's RADIUS server. Connection request policies decide which requests it handles; network policies check conditions such as AD group membership and choose the EAP method.",
        "It's the usual backend for 802.1X Wi-Fi (with PEAP or EAP-TLS using AD CS certificates) and for Always On VPN.",
      ],
      video: { title: "How to Configure RADIUS Server with NPS on Windows Server 2022", channel: "MSFT WebCast", url: yt("_WmPofLXM6c") },
      learn: ms("Network Policy Server (Microsoft Learn)", "windows-server/networking/technologies/nps/nps-top"),
    },
  },
  {
    key: "vlan", title: "VLANs and subnets", category: "Network services",
    summary: "Split one physical network into separate, routable segments.",
    standard: {
      text: [
        "A subnet is a range of IP addresses written in CIDR form, like 10.0.20.0/24 (256 addresses). Devices in the same subnet talk directly; reaching another subnet goes through a router.",
        "A VLAN separates traffic on the same switches into isolated broadcast domains, tagged with an ID (802.1Q). Usually each VLAN gets its own subnet.",
        "Segmenting servers, staff, students, guests and management into separate VLANs, with a firewall between them, limits how far an attacker can move.",
      ],
      wikipedia: wiki("VLAN"),
      video: { title: "VLAN Explained", channel: "PowerCert Animated Videos", url: yt("jC6MJTh9fRE") },
      links: [wiki("Subnet")],
    },
    ours: {
      text: ["The Network Lab models subnets, VLANs, routers with DHCP relay and firewalls. Press Run to see which traffic works, or load a reference design such as Campus or Enterprise."],
      to: "/lab",
      links: [],
    },
  },
  {
    key: "vpn", title: "VPN", category: "Network services",
    summary: "An encrypted tunnel into the network from anywhere.",
    standard: {
      text: [
        "A remote-access VPN gives a laptop an encrypted tunnel to the office network, as if it were plugged in. Site-to-site VPNs join whole networks.",
        "Common protocols: WireGuard (simple and fast), IPsec/IKEv2 (built into most OSes) and SSL VPNs. Authenticate users against the directory through RADIUS, ideally with MFA.",
      ],
      wikipedia: wiki("Virtual_private_network"),
      video: { title: "VPN (Virtual Private Network) Explained", channel: "PowerCert Animated Videos", url: yt("R-JUOpCgTZc") },
      links: [{ label: "WireGuard", url: "https://www.wireguard.com/" }],
    },
    ours: {
      text: [
        "OpenDirectory agents don't need a VPN: they connect out to the Management Server over TLS from anywhere. Use a VPN to reach other internal resources.",
        "Point your VPN gateway at OpenDirectory's RADIUS server so users sign in with their directory account.",
      ],
      to: "/network-services",
      links: [],
    },
    microsoft: {
      name: "Always On VPN and RRAS",
      text: [
        "Routing and Remote Access Service (RRAS) is Windows Server's VPN server. Always On VPN is the Windows client feature that connects automatically, with a device tunnel before sign-in and a user tunnel after.",
        "It usually authenticates through NPS with certificates from AD CS, and replaced the older DirectAccess.",
      ],
      video: { title: "Client-to-Site VPN with RRAS and NPS on Windows Server", channel: "Troy Berg", url: yt("q_aaU_AqF20") },
      learn: ms("Always On VPN (Microsoft Learn)", "windows-server/remote/remote-access/overview-always-on-vpn"),
    },
  },
  {
    key: "laps", title: "Local admin password rotation", category: "Security",
    summary: "A unique, rotating password for every computer's local admin account.",
    standard: {
      text: [
        "If every computer shares one local administrator password, stealing it from one machine unlocks them all. That's how attackers move sideways (\"pass the hash\").",
        "Rotation gives each computer its own random password, changes it on a schedule, and stores it centrally so admins can look it up, with an audit record, when they need it.",
      ],
      wikipedia: wiki("Pass_the_hash"),
      video: { title: "What is Microsoft (LAPS) Local Administrator Password Solution?", channel: "Cloud Scholars", url: yt("H5Bx2_9JgHc") },
      links: [],
    },
    ours: {
      text: [
        "The Local admin password rotation GPO setting keeps one local admin account on each endpoint (Windows, macOS and Linux) with its own random password.",
        "The agent stores each new password on the Management Server, encrypted, before setting it. Admins see it, with an audit entry, on the endpoint's page and can rotate it now.",
      ],
      to: "/gpos",
      links: [],
    },
    microsoft: {
      name: "Windows LAPS",
      text: [
        "Windows LAPS is built into current Windows versions. It backs the password up to Active Directory (encrypted, readable only by authorized groups) or to Microsoft Entra ID, configured with Group Policy or Intune.",
        "Admins read the password with Get-LapsADPassword or in ADUC, and can force rotation after use.",
      ],
      video: { title: "Setup Windows LAPS in just 5 Minutes", channel: "CloudManagement.Community", url: yt("leo_rQXegtU") },
      learn: ms("Windows LAPS overview (Microsoft Learn)", "windows-server/identity/laps/laps-overview"),
    },
  },
  {
    key: "patch", title: "Patch management", category: "Endpoint management",
    summary: "Find and install updates on every computer, on your schedule.",
    standard: {
      text: [
        "Most breaches use known vulnerabilities that already had a fix. Patch management is the routine of scanning for missing updates, testing them, and installing them within a set time.",
        "Use rings: a small pilot group first, then everyone else a few days later. Track which machines need a reboot.",
      ],
      wikipedia: wiki("Patch_(computing)"),
      video: { title: "What Is Patch Management?", channel: "JumpCloud", url: yt("NvOwsO-GtiM") },
      links: [{ label: "CISA Known Exploited Vulnerabilities catalog", url: "https://www.cisa.gov/known-exploited-vulnerabilities-catalog" }],
    },
    ours: {
      text: ["Patch Management scans Windows Update, apt, dnf and softwareupdate on each endpoint, lists pending updates and reboot status, and installs them on demand for endpoints, security groups or OUs."],
      to: "/patches",
      links: [],
    },
    microsoft: {
      name: "WSUS",
      text: [
        "Windows Server Update Services downloads updates from Microsoft once and serves them inside the network. Admins approve updates per computer group, and a Group Policy setting points clients at the WSUS server.",
        "Microsoft has deprecated WSUS in favor of cloud services such as Windows Autopatch and Intune update rings.",
      ],
      video: { title: "How to Install and Configure WSUS on Windows Server", channel: "Digital IT Lab", url: yt("teL2Sw7vDLw") },
      learn: ms("WSUS overview (Microsoft Learn)", "windows-server/administration/windows-server-update-services/get-started/windows-server-update-services-wsus"),
    },
  },
  {
    key: "remote", title: "Remote shell and desktop", category: "Endpoint management",
    summary: "Fix a computer without walking over to it.",
    standard: {
      text: [
        "A remote shell (like SSH) gives a command line on another machine; remote desktop (RDP, VNC) shows its screen.",
        "Never expose RDP or SSH directly to the internet. Reach them through a VPN or a broker, require strong authentication, and log every session.",
      ],
      wikipedia: wiki("Secure_Shell"),
      video: { title: "Learn SSH In 6 Minutes", channel: "Code With Bubb", url: yt("v45p_kJV9i4") },
      links: [wiki("Remote_Desktop_Protocol")],
    },
    ours: {
      text: ["Each endpoint's page has Remote Shell. Remote Desktop tunnels RDP or VNC through the agent's existing outbound connection, so endpoints need no inbound ports. Sessions are audited."],
      to: "/endpoints",
      links: [],
    },
    microsoft: {
      name: "Remote Desktop Services",
      text: [
        "RDP is built into Windows Pro and Server. Remote Desktop Services adds session hosts for many users, a connection broker, a web client and an RD Gateway that publishes RDP safely over HTTPS.",
        "PowerShell remoting (WinRM) is the Windows remote shell.",
      ],
      video: { title: "How to Deploy Remote Desktop Services RDS on Windows Server 2022", channel: "MSFT WebCast", url: yt("p_OOmOe7E1Y") },
      learn: ms("Remote Desktop Services (Microsoft Learn)", "windows-server/remote/remote-desktop-services/remote-desktop-services-overview"),
    },
  },
  {
    key: "mdm", title: "Mobile device management", category: "Endpoint management",
    summary: "Inventory and configuration for phones and tablets.",
    standard: {
      text: [
        "MDM tracks which phones and tablets belong to the organization and pushes settings such as Wi-Fi, passcode rules and certificates.",
        "Apple devices use configuration profiles (.mobileconfig). Full over-the-air management needs Apple's MDM protocol (with an APNs certificate) or Android Enterprise.",
      ],
      wikipedia: wiki("Mobile_device_management"),
      video: { title: "What Is Mobile Device Management (MDM)?", channel: "JumpCloud", url: yt("5Btm7jvBO2s") },
      links: [{ label: "Apple Platform Deployment guide", url: "https://support.apple.com/guide/deployment/welcome/web" }],
    },
    ours: {
      text: ["Mobile Devices keeps an inventory by serial, IMEI, owner and OS, and builds Apple configuration profiles for Wi-Fi, passcode rules and your root CA. Over-the-air enrollment isn't supported yet."],
      to: "/mobile",
      links: [],
    },
    microsoft: {
      name: "Microsoft Intune",
      text: [
        "Intune is Microsoft's cloud MDM for Windows, iOS, Android and macOS. Devices enroll through Entra ID, receive configuration and compliance policies, and Conditional Access can block non-compliant devices from company apps.",
      ],
      video: { title: "Enroll Windows Devices in Intune", channel: "BTNHD", url: yt("wqNc4weNB-k") },
      learn: ms("Microsoft Intune overview (Microsoft Learn)", "mem/intune/fundamentals/what-is-intune"),
    },
  },
  {
    key: "firewall", title: "Firewalls and segmentation", category: "Security",
    summary: "Decide which traffic may pass between networks.",
    standard: {
      text: [
        "A firewall allows or blocks traffic by source, destination, protocol and port. Start from default deny and open only what's needed.",
        "Put firewalls between zones of different trust (internet, DMZ, servers, users, management), not only at the edge.",
      ],
      wikipedia: wiki("Firewall_(computing)"),
      video: { title: "Firewalls Explained in 4 Minutes", channel: "Turtle Code", url: yt("mPrHLTouP6Y") },
      links: [],
    },
    ours: {
      text: ["The Firewall GPO setting turns on each endpoint's own firewall on every OS. The Network Lab checks that your network firewalls allow the ports OpenDirectory needs, such as 8443/tcp for agent check-in."],
      to: "/lab",
      links: [],
    },
    microsoft: {
      name: "Windows Defender Firewall",
      text: ["The host firewall built into Windows has Domain, Private and Public profiles and is managed centrally with Group Policy (Windows Defender Firewall with Advanced Security) or Intune."],
      video: { title: "Configure Windows Firewall using Group Policy in Active Directory", channel: "MSFT WebCast", url: yt("lffLuaY1wLE") },
      learn: ms("Windows Firewall overview (Microsoft Learn)", "windows/security/operating-system-security/network-security/windows-firewall/"),
    },
  },
  {
    key: "zerotrust", title: "Zero trust", category: "Security",
    summary: "Never trust the network; verify every user, device and request.",
    standard: {
      text: [
        "Zero trust assumes the network is already hostile. Every access is checked against identity, device health and policy, with the least privilege needed.",
        "Practical steps: MFA everywhere, managed and compliant devices only, segmentation, and separate admin accounts used only from privileged access workstations.",
      ],
      wikipedia: wiki("Zero_trust_architecture"),
      video: { title: "Zero Trust Explained in 7 Minutes", channel: "3V Learning Centre", url: yt("h0lAV97lwPI") },
      links: [{ label: "NIST SP 800-207: Zero Trust Architecture", url: "https://csrc.nist.gov/pubs/sp/800/207/final" }],
    },
    ours: {
      text: ["Compliance status per endpoint, the High security onboarding baseline, and the High security reference design in the Network Lab are good starting points."],
      to: "/lab",
      links: [],
    },
    microsoft: {
      name: "Microsoft Zero Trust",
      text: ["Microsoft's approach combines Entra ID Conditional Access, Intune device compliance, Defender and network segmentation, built around verify explicitly, use least privilege and assume breach."],
      video: { title: "Introduction to Zero Trust", channel: "Microsoft Developer", url: yt("Vqd0rFZlU68") },
      learn: { label: "Zero Trust overview (Microsoft Learn)", url: "https://learn.microsoft.com/en-us/security/zero-trust/zero-trust-overview" },
    },
  },
  {
    key: "backup", title: "Backups and recovery", category: "Security",
    summary: "Copies you can restore when something goes wrong.",
    standard: {
      text: [
        "The 3-2-1 rule: three copies of your data, on two kinds of storage, with one copy off site. Keep at least one copy offline or immutable so ransomware can't reach it.",
        "A backup you haven't restored is a guess. Test restores on a schedule.",
      ],
      wikipedia: wiki("Backup"),
      video: { title: "Data Backup: The 3-2-1 Rule", channel: "ExplainingComputers", url: yt("rFO6NyLIP7M") },
      links: [],
    },
    ours: {
      text: [
        "Backup & Restore makes encrypted, scheduled backups of the whole directory to local, SFTP, FTP, WebDAV, S3 and cloud drives, with retention and a verify step.",
        "The Recycle Bin restores single deleted objects with their passwords and memberships for 180 days.",
      ],
      to: "/backups",
      links: [],
    },
    microsoft: {
      name: "AD Recycle Bin and system state backup",
      text: [
        "The AD Recycle Bin (turned on once per forest) keeps deleted objects with all their attributes so they can be restored with Restore-ADObject or in Active Directory Administrative Center.",
        "For disasters, Windows Server Backup takes system state backups of domain controllers for authoritative or non-authoritative restore.",
      ],
      video: { title: "How to Enable Active Directory Recycle Bin and Restore Deleted Objects", channel: "TheWhyNet", url: yt("dZNZNH1lVQI") },
      learn: ms("AD Recycle Bin (Microsoft Learn)", "windows-server/identity/ad-ds/get-started/adac/active-directory-recycle-bin"),
    },
  },
];
