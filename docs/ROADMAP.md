# Roadmap

## Done (v0.1)

- Directory: users, security groups, organizational units, Domain Admin role, audit log, search.
- Group Policy: cross-platform GPOs with priority, OU links with inheritance, Resultant Set of Policy per endpoint.
- Settings: password minimum length, screen lock, firewall, disk encryption (report), automatic updates, login banner, required software, sign-in accounts.
- Endpoint Agent as a native background service on Windows, macOS and Linux, with enrollment tokens, hardware/OS/network/storage inventory, compliance reporting, remote scripts, inventory refresh and agent restart.
- Admin Center desktop app with embedded server, onboarding wizard, Learn mode and glossary, command palette, tray icon, light/dark theme.
- `od` CLI covering the same features.

## Next

1. **Real-hardware validation** on Windows 10/11 Home and Pro and on macOS 14+. Ship signed installers (MSI, pkg, deb/rpm).
2. **Revert removed settings.** When a setting leaves a GPO, return the endpoint to its previous value instead of leaving it as last applied.
3. **Live remote session.** Interactive shell first, then remote desktop (screen streaming through the server, with user consent prompts).
4. **Mobile device tracking (basic MDM).** Inventory for iPhone and Android by serial number, IMEI, owner and IP. Later, real enrollment through Apple MDM (needs an APNs certificate) and Android Enterprise.
5. **Software deployment and patch reporting.** Package catalog, per-endpoint installed software list.
6. **Domain Controller role.** LDAP and Kerberos endpoints so AD-aware apps and Windows Pro machines can authenticate against OpenDirectory, plus sync from and to an existing Active Directory or Samba domain.
7. **Delegated administration.** Roles narrower than Domain Admin (help desk, read-only), scoped to OUs.
8. **Agent self-update** from GitHub Releases, staged by OU.
9. **High availability.** Server replication and backup/restore from the Admin Center.
