// Mirrors od-core's Rust types.
export type Uuid = string;
export type OsFamily = "windows" | "macos" | "linux";
export type Role = "user" | "admin";

export interface OrgUnit { id: Uuid; name: string; description: string; parent_id: Uuid | null; created_at: string }
export interface User {
  id: Uuid; username: string; display_name: string; email: string; title: string; department: string;
  ou_id: Uuid | null; enabled: boolean; role: Role; must_change_password: boolean;
  password_set_at: string | null; last_login_at: string | null; created_at: string; group_ids: Uuid[];
}
export interface Group { id: Uuid; name: string; description: string; ou_id: Uuid | null; member_ids: Uuid[]; endpoint_ids: Uuid[]; created_at: string }
export interface Disk { mount: string; total_bytes: number; free_bytes: number; file_system: string }
export interface NetInterface { name: string; mac: string; ips: string[] }
export interface Inventory {
  hostname: string; os_name: string; os_version: string; os_edition: string; kernel_version: string; arch: string;
  manufacturer: string; model: string; serial_number: string; cpu_model: string; cpu_cores: number; memory_bytes: number;
  uptime_secs: number; disks: Disk[]; interfaces: NetInterface[]; logged_in_users: string[];
  disk_encrypted: boolean | null; firewall_enabled: boolean | null;
  patches?: PatchReport | null;
}
export interface Patch { title: string; id: string; security: boolean; severity: string }
export interface PatchReport { scanned_at: string | null; pending: Patch[]; reboot_required: boolean; source: string; error: string }
export type ComplianceState = "compliant" | "applied" | "non_compliant" | "failed" | "unsupported";
export interface ComplianceItem { setting: string; state: ComplianceState; message: string }
export interface Endpoint {
  id: Uuid; hostname: string; os: OsFamily; ou_id: Uuid | null; notes: string; agent_version: string;
  enrolled_at: string; last_seen_at: string | null; online: boolean; inventory: Inventory; compliance: ComplianceItem[]; public_ip: string;
}
export interface EnrollmentToken { token: string; description: string; ou_id: Uuid | null; uses: number; max_uses: number | null; expires_at: string | null; created_at: string; created_by: string }
export type Shell = "default" | "powershell" | "bash" | "zsh" | "cmd";
export type JobKind = { type: "script"; shell: Shell; script: string; env?: Record<string, string> } | { type: "refresh_inventory" } | { type: "restart_agent" } | { type: "scan_updates" } | { type: "install_updates"; security_only: boolean; reboot: boolean };
export type JobStatus = "queued" | "running" | "succeeded" | "failed" | "cancelled";
export interface Job {
  id: Uuid; endpoint_id: Uuid; kind: JobKind; status: JobStatus; script_id: Uuid | null; batch_id: Uuid | null; exit_code: number | null; output: string;
  created_by: string; created_at: string; started_at: string | null; finished_at: string | null;
}
export interface AuditEntry { id: number; at: string; actor: string; action: string; target: string; details: string }

export type Setting =
  | { kind: "password_min_length"; length: number }
  | { kind: "screen_lock"; minutes: number }
  | { kind: "firewall"; enabled: boolean }
  | { kind: "disk_encryption"; required: boolean }
  | { kind: "automatic_updates"; enabled: boolean }
  | { kind: "local_accounts"; group_id: Uuid; admin: boolean }
  | { kind: "login_banner"; title: string; text: string }
  | { kind: "install_packages"; packages: string[] }
  | { kind: "run_script"; script_id: Uuid; trigger: ScriptTrigger; params: Record<string, string> }
  | { kind: "account_lockout"; threshold: number; duration_minutes: number }
  | { kind: "password_max_age"; days: number }
  | { kind: "password_history"; count: number }
  | { kind: "password_complexity"; required: boolean }
  | { kind: "usb_storage"; mode: "allow" | "read_only" | "block" }
  | { kind: "guest_account"; enabled: boolean }
  | { kind: "remote_desktop"; enabled: boolean }
  | { kind: "ssh_server"; enabled: boolean }
  | { kind: "auto_login"; allowed: boolean }
  | { kind: "time_server"; server: string }
  | { kind: "hide_last_username"; enabled: boolean }
  | { kind: "disable_autorun"; enabled: boolean }
  | { kind: "smb1"; enabled: boolean }
  | { kind: "llmnr"; enabled: boolean }
  | { kind: "antivirus"; required: boolean }
  | { kind: "app_reputation"; enabled: boolean }
  | { kind: "security_auditing"; enabled: boolean }
  | { kind: "update_deferral"; quality_days: number; feature_days: number }
  | { kind: "local_admin_password"; account: string; length: number; rotate_days: number }
  | { kind: "trusted_root_certificate"; name: string; pem: string };
export type SettingKind = Setting["kind"];

export interface Policy {
  id: Uuid; name: string; description: string; enabled: boolean; priority: number;
  targets: { all_endpoints: boolean; ou_ids: Uuid[] }; settings: Setting[]; created_at: string; updated_at: string;
}
export type SupportLevel = "enforced" | "report_only" | "unsupported";
export interface PlatformSupport { os: OsFamily; level: SupportLevel; mechanism: string }
export interface CatalogEntry {
  kind: SettingKind; title: string; category: string; summary: string; ad_equivalent: string;
  support: PlatformSupport[]; default: Setting; choices?: { field: string; options: [string, string][] }[];
}
export interface RsopEntry {
  setting: Setting; policy_id: Uuid; policy_name: string; overridden: string[]; description: string;
  support: PlatformSupport | null; compliance: ComplianceItem | null;
}
export interface ServerInfo { version: string; endpoint_url: string; tls_fingerprint: string | null; data_dir: string }
export interface Overview {
  organization: string | null; domain: string | null;
  counts: Record<string, number>;
  endpoints_by_os: Record<string, number>;
  getting_started: Record<string, boolean>;
  recent_activity: AuditEntry[];
  server: ServerInfo;
}
export interface SearchHit { type: "user" | "group" | "endpoint" | "ou" | "policy"; id: Uuid; title: string; subtitle: string }

export interface ScriptParam { name: string; label: string; default: string; required: boolean; secret?: boolean }
export interface Script {
  id: Uuid; name: string; description: string; category: string; shell: Shell; body: string; os: OsFamily[];
  params: ScriptParam[]; template: string | null; created_by: string; created_at: string; updated_at: string;
}
export interface ScriptTemplate { key: string; name: string; category: string; description: string; os: OsFamily[]; shell: Shell; body: string; params: ScriptParam[] }
export type ScriptTrigger = { type: "startup" } | { type: "logon" } | { type: "interval"; minutes: number } | { type: "once" };
export interface RunTargets { all_endpoints: boolean; endpoint_ids: Uuid[]; group_ids: Uuid[]; ou_ids: Uuid[] }
export interface TargetPreview { id: Uuid; hostname: string; os: OsFamily; online: boolean }
export interface RunResponse { batch_id: Uuid | null; targets: TargetPreview[]; skipped: TargetPreview[]; jobs: Job[] }
