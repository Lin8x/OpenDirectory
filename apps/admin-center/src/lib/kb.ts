// Knowledge Base: one entry per service or concept, with a short guide, how
// OpenDirectory implements it, the Microsoft / Active Directory counterpart,
// and links to learn more. Every link was checked when it was added.

export interface KbLink { label: string; url: string }
export interface KbVideo { title: string; channel: string; url: string }
export interface KbEntry {
  key: string;
  title: string;
  category: "Directory & identity" | "Network services" | "Security" | "Endpoint management";
  summary: string;
  /** Plain-language guide, one paragraph per item. */
  guide: string[];
  /** Where it lives in OpenDirectory. */
  inOpenDirectory: { text: string; to?: string };
  wikipedia: string;
  video: KbVideo;
  links: KbLink[];
  microsoft?: { name: string; text: string[]; links: KbLink[] };
}

const yt = (id: string) => `https://www.youtube.com/watch?v=${id}`;
const wiki = (t: string) => `https://en.wikipedia.org/wiki/${t}`;
const ms = (p: string) => `https://learn.microsoft.com/en-us/${p}`;

export const KB: KbEntry[] = [
  {
    key: "directory", title: "Directory services", category: "Directory & identity",
    summary: "A central database of users, groups and computers that everything else trusts.",
    guide: [
      "A directory service stores identities (people, groups, computers) and their attributes in one place, so you create an account once and use it everywhere: sign-in, Wi-Fi, VPN, file shares and apps.",
      "Objects are organized in a tree. Organizational units (OUs) are folders in that tree. They mirror your organization and are where you link policy.",
      "Security groups collect users so you grant access to the group instead of to each person. Good practice: grant access to groups only, never to individual users.",
    ],
    inOpenDirectory: { text: "Users, Security Groups and Organizational Units are the directory. Everything else (GPOs, scripts, RADIUS, LDAP) reads from it.", to: "/users" },
    wikipedia: wiki("Directory_service"),
    video: { title: "Active Directory Explained in 5 minutes", channel: "Connected Cookie", url: yt("OfXJlmuoc20") },
    links: [{ label: "Samba: Active Directory Domain Controller", url: "https://wiki.samba.org/index.php/Setting_up_Samba_as_an_Active_Directory_Domain_Controller" }],
    microsoft: {
      name: "Active Directory Domain Services (AD DS)",
      text: ["AD DS is Microsoft's directory: domains, forests, OUs and domain controllers that replicate with each other. Active Directory Users and Computers (ADUC) is its main console.", "OpenDirectory's Users, Groups and OUs pages correspond to ADUC, and the Domain Controller role can run a real AD-compatible DC with Samba."],
      links: [{ label: "AD DS overview (Microsoft Learn)", url: ms("windows-server/identity/ad-ds/get-started/virtual-dc/active-directory-domain-services-overview") }],
    },
  },
  {
    key: "dns", title: "DNS", category: "Network services",
    summary: "Turns names like od.corp.example.com into IP addresses.",
    guide: [
      "DNS (Domain Name System) is the internet's phone book. A client asks a resolver for a name, and the resolver walks the hierarchy (root, then .com, then example.com) until an authoritative server answers.",
      "Record types to know: A (IPv4 address), AAAA (IPv6), CNAME (alias), MX (mail), TXT (verification and policy), SRV (where a service runs, used heavily by Active Directory), PTR (reverse lookup).",
      "Inside a company, an internal DNS server is authoritative for the domain (corp.example.com) and forwards everything else to an upstream resolver. Clients get the DNS server address from DHCP.",
    ],
    inOpenDirectory: { text: "Network Services > DNS: an authoritative zone for your domain with automatic records for every endpoint and DHCP client, plus forwarding.", to: "/network-services" },
    wikipedia: wiki("Domain_Name_System"),
    video: { title: "How a DNS Server (Domain Name System) works", channel: "PowerCert Animated Videos", url: yt("mpQZVYPuDGU") },
    links: [
      { label: "RFC 1034: Domain names, concepts and facilities", url: "https://www.rfc-editor.org/rfc/rfc1034" },
      { label: "Cloudflare Learning: What is DNS?", url: "https://www.cloudflare.com/learning/dns/what-is-dns/" },
    ],
    microsoft: {
      name: "Windows Server DNS",
      text: ["The DNS Server role on Windows Server, usually installed on every domain controller with AD-integrated zones that replicate with the directory. Managed in DNS Manager.", "Active Directory depends on DNS: clients find domain controllers through SRV records such as _ldap._tcp.dc._msdcs.<domain>."],
      links: [{ label: "DNS in Windows Server (Microsoft Learn)", url: ms("windows-server/networking/dns/dns-overview") }],
    },
  },
  {
    key: "dhcp", title: "DHCP", category: "Network services",
    summary: "Hands out IP addresses and network settings automatically.",
    guide: [
      "DHCP (Dynamic Host Configuration Protocol) gives a device its IP address, subnet mask, default gateway and DNS servers when it joins the network. The exchange is Discover, Offer, Request, Acknowledge (DORA).",
      "A scope is the range of addresses a server hands out on one subnet. A reservation always gives the same address to one MAC address, useful for printers and servers.",
      "DHCP uses broadcasts, which don't cross routers. To serve several VLANs from one server, turn on DHCP relay (\"ip helper-address\") on the router.",
    ],
    inOpenDirectory: { text: "Network Services > DHCP: scopes, reservations, relay support and a lease table. Off by default so it never clashes with your router.", to: "/network-services" },
    wikipedia: wiki("Dynamic_Host_Configuration_Protocol"),
    video: { title: "DHCP Explained - Dynamic Host Configuration Protocol", channel: "PowerCert Animated Videos", url: yt("e6-TaH5bkjo") },
    links: [{ label: "RFC 2131: Dynamic Host Configuration Protocol", url: "https://www.rfc-editor.org/rfc/rfc2131" }],
    microsoft: {
      name: "Windows Server DHCP",
      text: ["The DHCP Server role, managed in the DHCP console. In a domain it must be authorized in Active Directory before it hands out addresses, and it can update DNS on behalf of clients."],
      links: [{ label: "DHCP in Windows Server (Microsoft Learn)", url: ms("windows-server/networking/technologies/dhcp/dhcp-top") }],
    },
  },
  {
    key: "ldap", title: "LDAP", category: "Directory & identity",
    summary: "The standard protocol apps use to read and search a directory.",
    guide: [
      "LDAP (Lightweight Directory Access Protocol) lets an application look up users and groups, and check a password by binding as the user. Many apps, NAS boxes, printers and firewalls support it.",
      "Every object has a distinguished name (DN) such as CN=Alice,OU=Staff,DC=corp,DC=example,DC=com. Searches start from a base DN and use filters like (&(objectClass=user)(sAMAccountName=alice)).",
      "Plain LDAP (port 389) sends passwords in clear text. Use LDAPS (port 636) or StartTLS so binds are encrypted.",
    ],
    inOpenDirectory: { text: "Network Services > LDAP: a read-only LDAP and LDAPS server with an Active Directory-style schema, so apps that support AD can use OpenDirectory.", to: "/network-services" },
    wikipedia: wiki("Lightweight_Directory_Access_Protocol"),
    video: { title: "What is LDAP (Lightweight Directory Access Protocol)?", channel: "CBT Nuggets", url: yt("vy3e6ekuqqg") },
    links: [{ label: "RFC 4511: LDAP protocol", url: "https://www.rfc-editor.org/rfc/rfc4511" }],
    microsoft: {
      name: "AD DS over LDAP",
      text: ["Every Active Directory domain controller is an LDAP server. Tools like ADSI Edit and ldp.exe talk to it directly, and Microsoft recommends requiring LDAP signing and channel binding."],
      links: [{ label: "LDAP signing for Active Directory (Microsoft Learn)", url: ms("windows-server/identity/ad-ds/ldap-signing") }],
    },
  },
  {
    key: "kerberos", title: "Kerberos", category: "Directory & identity",
    summary: "Ticket-based single sign-on used by Active Directory.",
    guide: [
      "Kerberos lets a user prove who they are once and then reach many services without sending their password again. A trusted Key Distribution Center (KDC) issues tickets.",
      "At sign-in the user gets a Ticket Granting Ticket (TGT). To reach a service, the client trades the TGT for a service ticket for that service's principal name (SPN).",
      "Kerberos needs clocks within about 5 minutes of each other and working DNS, which is why domain controllers also run time and DNS services.",
    ],
    inOpenDirectory: { text: "The Domain Controller role (Samba) provides a Kerberos KDC for domain-joined Windows, macOS and Linux computers.", to: "/domain-controller" },
    wikipedia: wiki("Kerberos_(protocol)"),
    video: { title: "Kerberos Explained (In 3 Levels Of Detail)", channel: "VbScrub", url: yt("snGeZlDQL2Q") },
    links: [
      { label: "MIT Kerberos documentation", url: "https://web.mit.edu/kerberos/krb5-latest/doc/" },
      { label: "RFC 4120: The Kerberos Network Authentication Service (V5)", url: "https://www.rfc-editor.org/rfc/rfc4120" },
    ],
    microsoft: {
      name: "Kerberos in Windows",
      text: ["Every AD domain controller runs the KDC service. Windows prefers Kerberos and falls back to NTLM, which security baselines increasingly restrict."],
      links: [{ label: "Kerberos authentication overview (Microsoft Learn)", url: ms("windows-server/security/kerberos/kerberos-authentication-overview") }],
    },
  },
  {
    key: "gpo", title: "Group Policy", category: "Endpoint management",
    summary: "Central configuration for every computer, linked to OUs.",
    guide: [
      "Group Policy describes how computers should be configured (password rules, screen lock, firewall, software, scripts) and applies it automatically, again and again, so settings don't drift.",
      "Policies are linked to OUs. Child OUs inherit from their parents, and when two policies set the same thing, the higher-priority one wins.",
      "Resultant Set of Policy (RSoP) is the final merged result on one computer, and the first thing to check when a setting doesn't apply.",
    ],
    inOpenDirectory: { text: "Group Policy: cross-platform GPOs that the agent applies natively on Windows (including Home), macOS and Linux, with RSoP on each endpoint.", to: "/gpos" },
    wikipedia: wiki("Group_Policy"),
    video: { title: "Active Directory GPO Tutorial for Beginners", channel: "The Cloud Threat Defender", url: yt("AX9TX7c90MA") },
    links: [{ label: "CIS Benchmarks (hardening baselines)", url: "https://www.cisecurity.org/cis-benchmarks" }],
    microsoft: {
      name: "Group Policy (GPMC)",
      text: ["Group Policy Management Console links GPOs to sites, domains and OUs. Processing order is Local, Site, Domain, OU (LSDOU). Windows Home editions can't join a domain, so they don't get domain GPOs."],
      links: [{ label: "Group Policy overview (Microsoft Learn)", url: ms("windows-server/identity/ad-ds/manage/group-policy/group-policy-overview") }],
    },
  },
  {
    key: "pki", title: "Certificates and PKI", category: "Security",
    summary: "A certificate authority vouches for servers, users and devices.",
    guide: [
      "Public key infrastructure (PKI) binds a public key to a name with a certificate signed by a certificate authority (CA). Anyone who trusts the CA's root certificate trusts what it signs.",
      "Use internal certificates for intranet sites, LDAPS, RADIUS (EAP-TLS) and device identity. Keep the root CA's key safe, keep certificate lifetimes short, and publish a revocation list (CRL).",
      "Auto-enrollment means each device creates its own key, sends a certificate signing request (CSR), and renews before expiry, so nobody copies private keys around.",
    ],
    inOpenDirectory: { text: "Network Services > Certificate Authority, plus the Trusted root certificate and Machine certificate auto-enrollment GPO settings.", to: "/network-services" },
    wikipedia: wiki("Public_key_infrastructure"),
    video: { title: "PKI Hierarchy", channel: "ITFreeTraining", url: yt("n_2RvlSpWEU") },
    links: [
      { label: "RFC 5280: X.509 certificate and CRL profile", url: "https://www.rfc-editor.org/rfc/rfc5280" },
      { label: "Cloudflare Learning: What is a TLS certificate?", url: "https://www.cloudflare.com/learning/ssl/what-is-an-ssl-certificate/" },
    ],
    microsoft: {
      name: "Active Directory Certificate Services (AD CS)",
      text: ["AD CS provides enterprise CAs with certificate templates and Group Policy auto-enrollment for users and computers. Misconfigured templates are a common attack path, so review them carefully."],
      links: [{ label: "AD CS overview (Microsoft Learn)", url: ms("windows-server/identity/ad-cs/active-directory-certificate-services-overview") }],
    },
  },
  {
    key: "radius", title: "RADIUS and 802.1X", category: "Network services",
    summary: "One sign-in for Wi-Fi, VPN and switch ports.",
    guide: [
      "RADIUS is how network devices (Wi-Fi controllers, VPN gateways, switches) ask a central server \"is this user allowed in?\". The device is the RADIUS client and shares a secret with the server.",
      "802.1X is port-based access control: nobody gets on the wired or wireless network until RADIUS says yes. WPA2/WPA3-Enterprise Wi-Fi is 802.1X over Wi-Fi.",
      "The EAP method decides how the user proves themselves: PEAP/MS-CHAPv2 uses a password, EAP-TLS uses a certificate on the device (the strongest option).",
    ],
    inOpenDirectory: { text: "Network Services > RADIUS: PAP and MS-CHAPv2 for VPNs and network devices, limited per client by security group. EAP (WPA2/3-Enterprise) isn't supported yet.", to: "/network-services" },
    wikipedia: wiki("RADIUS"),
    video: { title: "How 802.1X works", channel: "CBT Nuggets", url: yt("SdC3ppnanAE") },
    links: [
      { label: "RFC 2865: RADIUS", url: "https://www.rfc-editor.org/rfc/rfc2865" },
      { label: "Wikipedia: IEEE 802.1X", url: wiki("IEEE_802.1X") },
      { label: "FreeRADIUS documentation", url: "https://www.freeradius.org/documentation/" },
    ],
    microsoft: {
      name: "Network Policy Server (NPS)",
      text: ["NPS is Microsoft's RADIUS server. It checks AD group membership in network policies and is the usual backend for 802.1X Wi-Fi and Always On VPN."],
      links: [{ label: "Network Policy Server (Microsoft Learn)", url: ms("windows-server/networking/technologies/nps/nps-top") }],
    },
  },
  {
    key: "vlan", title: "VLANs and subnets", category: "Network services",
    summary: "Split one physical network into separate, routable segments.",
    guide: [
      "A subnet is a range of IP addresses written in CIDR form, like 10.0.20.0/24 (256 addresses). Devices in the same subnet talk directly; reaching another subnet goes through a router.",
      "A VLAN separates traffic on the same switches into isolated broadcast domains, tagged with an ID (802.1Q). Usually each VLAN gets its own subnet.",
      "Segmenting servers, staff, students, guests and management into separate VLANs, with a firewall between them, limits how far an attacker can move.",
    ],
    inOpenDirectory: { text: "Network Lab: model subnets, VLANs, routers with DHCP relay and firewalls, then press Run to see what works.", to: "/lab" },
    wikipedia: wiki("VLAN"),
    video: { title: "VLAN Explained", channel: "PowerCert Animated Videos", url: yt("jC6MJTh9fRE") },
    links: [{ label: "Wikipedia: Subnetwork", url: wiki("Subnet") }],
  },
  {
    key: "vpn", title: "VPN", category: "Network services",
    summary: "An encrypted tunnel into the network from anywhere.",
    guide: [
      "A remote-access VPN gives a laptop an encrypted tunnel to the office network, as if it were plugged in. Site-to-site VPNs join whole networks.",
      "Common protocols: WireGuard (simple and fast), IPsec/IKEv2 (built into most OSes) and SSL VPNs. Authenticate users against the directory through RADIUS, ideally with MFA.",
      "OpenDirectory agents don't need a VPN: they connect out to the Management Server over TLS. A VPN is for reaching other internal resources.",
    ],
    inOpenDirectory: { text: "Point your VPN gateway at OpenDirectory's RADIUS server for directory sign-in, and model remote users in the Network Lab.", to: "/network-services" },
    wikipedia: wiki("Virtual_private_network"),
    video: { title: "VPN (Virtual Private Network) Explained", channel: "PowerCert Animated Videos", url: yt("R-JUOpCgTZc") },
    links: [{ label: "WireGuard", url: "https://www.wireguard.com/" }],
    microsoft: {
      name: "Always On VPN and RRAS",
      text: ["Windows Server's Routing and Remote Access Service (RRAS) is a VPN server; Always On VPN connects automatically before sign-in. Both usually authenticate through NPS."],
      links: [{ label: "Always On VPN (Microsoft Learn)", url: ms("windows-server/remote/remote-access/overview-always-on-vpn") }],
    },
  },
  {
    key: "laps", title: "Local admin password rotation", category: "Security",
    summary: "A unique, rotating password for every computer's local admin account.",
    guide: [
      "If every computer shares one local administrator password, stealing it from one machine unlocks them all. That's how attackers move sideways (\"pass the hash\").",
      "Rotation gives each computer its own random password, changes it on a schedule, and stores it centrally so admins can look it up (with an audit record) when they need it.",
    ],
    inOpenDirectory: { text: "The Local admin password rotation (LAPS) GPO setting, with the password shown (and audited) on each endpoint's page.", to: "/gpos" },
    wikipedia: wiki("Pass_the_hash"),
    video: { title: "What is Microsoft (LAPS) Local Administrator Password Solution?", channel: "Cloud Scholars", url: yt("H5Bx2_9JgHc") },
    links: [],
    microsoft: {
      name: "Windows LAPS",
      text: ["Windows LAPS is built into current Windows versions and backs passwords up to Active Directory or Microsoft Entra ID, configured with Group Policy or Intune."],
      links: [{ label: "Windows LAPS overview (Microsoft Learn)", url: ms("windows-server/identity/laps/laps-overview") }],
    },
  },
  {
    key: "patch", title: "Patch management", category: "Endpoint management",
    summary: "Find and install updates on every computer, on your schedule.",
    guide: [
      "Most breaches use known vulnerabilities that already had a fix. Patch management is the routine of scanning for missing updates, testing them, and installing them within a set time.",
      "Use rings: a small pilot group first, then everyone else a few days later. Track which machines need a reboot.",
    ],
    inOpenDirectory: { text: "Patch Management: scan and install OS updates on Windows, macOS and Linux endpoints, with reboot status.", to: "/patches" },
    wikipedia: wiki("Patch_(computing)"),
    video: { title: "What Is Patch Management?", channel: "JumpCloud", url: yt("NvOwsO-GtiM") },
    links: [{ label: "CISA Known Exploited Vulnerabilities catalog", url: "https://www.cisa.gov/known-exploited-vulnerabilities-catalog" }],
    microsoft: {
      name: "WSUS",
      text: ["Windows Server Update Services lets admins approve Windows updates and serve them from inside the network. Microsoft has deprecated WSUS in favor of cloud services such as Windows Autopatch."],
      links: [{ label: "WSUS overview (Microsoft Learn)", url: ms("windows-server/administration/windows-server-update-services/get-started/windows-server-update-services-wsus") }],
    },
  },
  {
    key: "remote", title: "Remote shell and desktop", category: "Endpoint management",
    summary: "Fix a computer without walking over to it.",
    guide: [
      "A remote shell (like SSH or PowerShell remoting) gives a command line on another machine; remote desktop (RDP, VNC) shows its screen.",
      "Never expose RDP or SSH directly to the internet. Reach them through a VPN or a broker, require strong authentication, and log every session.",
    ],
    inOpenDirectory: { text: "Each endpoint's page has Remote Shell, and Remote Desktop tunnels RDP or VNC through the agent's existing connection, so no inbound ports are needed.", to: "/endpoints" },
    wikipedia: wiki("Secure_Shell"),
    video: { title: "Learn SSH In 6 Minutes", channel: "Code With Bubb", url: yt("v45p_kJV9i4") },
    links: [{ label: "Wikipedia: Remote Desktop Protocol", url: wiki("Remote_Desktop_Protocol") }],
    microsoft: {
      name: "Remote Desktop Services and PowerShell remoting",
      text: ["RDP is built into Windows Pro and Server. Remote Desktop Services adds session hosts and a gateway; PowerShell remoting (WinRM) gives a remote shell."],
      links: [{ label: "Remote Desktop Services (Microsoft Learn)", url: ms("windows-server/remote/remote-desktop-services/remote-desktop-services-overview") }],
    },
  },
  {
    key: "mdm", title: "Mobile device management", category: "Endpoint management",
    summary: "Inventory and configuration for phones and tablets.",
    guide: [
      "MDM tracks which phones and tablets belong to the organization and pushes settings such as Wi-Fi, passcode rules and certificates.",
      "Apple devices use configuration profiles (.mobileconfig). Full over-the-air management needs Apple's MDM protocol (with an APNs certificate) or Android Enterprise.",
    ],
    inOpenDirectory: { text: "Mobile Devices: inventory by serial, IMEI and owner, and downloadable Apple configuration profiles. Over-the-air enrollment isn't supported yet.", to: "/mobile" },
    wikipedia: wiki("Mobile_device_management"),
    video: { title: "What Is Mobile Device Management (MDM)?", channel: "JumpCloud", url: yt("5Btm7jvBO2s") },
    links: [{ label: "Apple Platform Deployment guide", url: "https://support.apple.com/guide/deployment/welcome/web" }],
    microsoft: {
      name: "Microsoft Intune",
      text: ["Intune is Microsoft's cloud MDM for Windows, iOS, Android and macOS, tied to Entra ID."],
      links: [{ label: "Microsoft Intune overview (Microsoft Learn)", url: ms("mem/intune/fundamentals/what-is-intune") }],
    },
  },
  {
    key: "firewall", title: "Firewalls and segmentation", category: "Security",
    summary: "Decide which traffic may pass between networks.",
    guide: [
      "A firewall allows or blocks traffic by source, destination, protocol and port. Start from default deny and open only what's needed, for example 8443/tcp for agent check-in.",
      "Put firewalls between zones of different trust (internet, DMZ, servers, users, management), not only at the edge.",
    ],
    inOpenDirectory: { text: "The Firewall GPO setting turns on each endpoint's own firewall; the Network Lab checks your network firewalls allow the right ports.", to: "/lab" },
    wikipedia: wiki("Firewall_(computing)"),
    video: { title: "Firewalls Explained in 4 Minutes", channel: "Turtle Code", url: yt("mPrHLTouP6Y") },
    links: [],
    microsoft: {
      name: "Windows Defender Firewall",
      text: ["The host firewall built into Windows, configured with Group Policy (Windows Defender Firewall with Advanced Security) or Intune."],
      links: [{ label: "Windows Firewall overview (Microsoft Learn)", url: ms("windows/security/operating-system-security/network-security/windows-firewall/") }],
    },
  },
  {
    key: "zerotrust", title: "Zero trust", category: "Security",
    summary: "Never trust the network; verify every user, device and request.",
    guide: [
      "Zero trust assumes the network is already hostile. Every access is checked against identity, device health and policy, with the least privilege needed.",
      "Practical steps: MFA everywhere, managed and compliant devices only, segmentation, and separate admin accounts used only from privileged access workstations.",
    ],
    inOpenDirectory: { text: "Compliance status per endpoint, the High security baseline, and the Hardened reference design in the Network Lab.", to: "/lab" },
    wikipedia: wiki("Zero_trust_architecture"),
    video: { title: "Introduction to Zero Trust", channel: "Microsoft Developer", url: yt("Vqd0rFZlU68") },
    links: [{ label: "NIST SP 800-207: Zero Trust Architecture", url: "https://csrc.nist.gov/pubs/sp/800/207/final" }],
  },
  {
    key: "backup", title: "Backups", category: "Security",
    summary: "Copies you can restore when something goes wrong.",
    guide: [
      "The 3-2-1 rule: three copies of your data, on two kinds of storage, with one copy off site. Keep at least one copy offline or immutable so ransomware can't reach it.",
      "A backup you haven't restored is a guess. Test restores on a schedule.",
    ],
    inOpenDirectory: { text: "Backup & Restore: encrypted, scheduled backups of the whole directory to local, SFTP, S3 and cloud destinations, with retention.", to: "/backups" },
    wikipedia: wiki("Backup"),
    video: { title: "Data Backup: The 3-2-1 Rule", channel: "ExplainingComputers", url: yt("rFO6NyLIP7M") },
    links: [],
    microsoft: {
      name: "Windows Server Backup and AD Recycle Bin",
      text: ["Windows Server Backup takes system state backups of domain controllers. The AD Recycle Bin restores deleted objects with their attributes, like OpenDirectory's Recycle Bin."],
      links: [{ label: "AD Recycle Bin (Microsoft Learn)", url: ms("windows-server/identity/ad-ds/get-started/adac/active-directory-recycle-bin") }],
    },
  },
];
