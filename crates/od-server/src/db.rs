//! SQLite-backed directory store. One file, no external database.

use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use od_core::policy::{Policy, PolicyTargets, Setting};
use od_core::*;
use rusqlite::{params, Connection, OptionalExtension, Row};
use uuid::Uuid;

const SCHEMA: &str = r#"
PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;
CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS ous (
  id TEXT PRIMARY KEY, name TEXT NOT NULL, description TEXT NOT NULL DEFAULT '',
  parent_id TEXT REFERENCES ous(id) ON DELETE RESTRICT, created_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS users (
  id TEXT PRIMARY KEY, username TEXT NOT NULL UNIQUE COLLATE NOCASE,
  display_name TEXT NOT NULL, email TEXT NOT NULL DEFAULT '', title TEXT NOT NULL DEFAULT '',
  department TEXT NOT NULL DEFAULT '', ou_id TEXT REFERENCES ous(id) ON DELETE SET NULL,
  enabled INTEGER NOT NULL DEFAULT 1, role TEXT NOT NULL DEFAULT 'user',
  must_change_password INTEGER NOT NULL DEFAULT 0,
  password_hash TEXT, password_sealed BLOB, password_version INTEGER NOT NULL DEFAULT 0,
  password_set_at TEXT, last_login_at TEXT, created_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS groups (
  id TEXT PRIMARY KEY, name TEXT NOT NULL UNIQUE COLLATE NOCASE, description TEXT NOT NULL DEFAULT '',
  ou_id TEXT REFERENCES ous(id) ON DELETE SET NULL, created_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS group_members (
  group_id TEXT NOT NULL REFERENCES groups(id) ON DELETE CASCADE,
  user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  PRIMARY KEY (group_id, user_id)
);
CREATE TABLE IF NOT EXISTS endpoints (
  id TEXT PRIMARY KEY, hostname TEXT NOT NULL, os TEXT NOT NULL,
  ou_id TEXT REFERENCES ous(id) ON DELETE SET NULL, notes TEXT NOT NULL DEFAULT '',
  agent_version TEXT NOT NULL DEFAULT '', secret_hash TEXT NOT NULL,
  enrolled_at TEXT NOT NULL, last_seen_at TEXT, inventory TEXT NOT NULL DEFAULT '{}',
  compliance TEXT NOT NULL DEFAULT '[]', public_ip TEXT NOT NULL DEFAULT '',
  want_inventory INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS enrollment_tokens (
  token TEXT PRIMARY KEY, description TEXT NOT NULL DEFAULT '',
  ou_id TEXT REFERENCES ous(id) ON DELETE SET NULL, uses INTEGER NOT NULL DEFAULT 0,
  max_uses INTEGER, expires_at TEXT, created_at TEXT NOT NULL, created_by TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS jobs (
  id TEXT PRIMARY KEY, endpoint_id TEXT NOT NULL REFERENCES endpoints(id) ON DELETE CASCADE,
  kind TEXT NOT NULL, status TEXT NOT NULL, exit_code INTEGER, output TEXT NOT NULL DEFAULT '',
  created_by TEXT NOT NULL, created_at TEXT NOT NULL, started_at TEXT, finished_at TEXT
);
CREATE INDEX IF NOT EXISTS jobs_endpoint ON jobs(endpoint_id, created_at);
CREATE TABLE IF NOT EXISTS policies (
  id TEXT PRIMARY KEY, name TEXT NOT NULL, description TEXT NOT NULL DEFAULT '',
  enabled INTEGER NOT NULL DEFAULT 1, priority INTEGER NOT NULL DEFAULT 100,
  targets TEXT NOT NULL, settings TEXT NOT NULL, created_at TEXT NOT NULL, updated_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS sessions (
  token_hash TEXT PRIMARY KEY, user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  created_at TEXT NOT NULL, expires_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS group_endpoints (
  group_id TEXT NOT NULL REFERENCES groups(id) ON DELETE CASCADE,
  endpoint_id TEXT NOT NULL REFERENCES endpoints(id) ON DELETE CASCADE,
  PRIMARY KEY (group_id, endpoint_id)
);
CREATE TABLE IF NOT EXISTS scripts (
  id TEXT PRIMARY KEY, name TEXT NOT NULL UNIQUE COLLATE NOCASE, description TEXT NOT NULL DEFAULT '',
  category TEXT NOT NULL DEFAULT '', shell TEXT NOT NULL, body TEXT NOT NULL, os TEXT NOT NULL DEFAULT '[]',
  params TEXT NOT NULL DEFAULT '[]', template TEXT, created_by TEXT NOT NULL,
  created_at TEXT NOT NULL, updated_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS backup_plans (
  id TEXT PRIMARY KEY, name TEXT NOT NULL, sealed BLOB NOT NULL, updated_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS integrations (
  id TEXT PRIMARY KEY, kind TEXT NOT NULL, sealed BLOB NOT NULL, updated_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS recycle_bin (
  id TEXT PRIMARY KEY, kind TEXT NOT NULL, name TEXT NOT NULL, data TEXT NOT NULL,
  deleted_by TEXT NOT NULL, deleted_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS password_history (
  user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE, hash TEXT NOT NULL, set_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS reset_codes (
  user_id TEXT PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE, code_hash TEXT NOT NULL, expires_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS laps (
  endpoint_id TEXT PRIMARY KEY REFERENCES endpoints(id) ON DELETE CASCADE, account TEXT NOT NULL,
  password_sealed BLOB NOT NULL, previous_sealed BLOB, rotated_at TEXT NOT NULL, rotate_requested INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS helpdesk_messages (
  id INTEGER PRIMARY KEY AUTOINCREMENT, endpoint_id TEXT NOT NULL REFERENCES endpoints(id) ON DELETE CASCADE,
  from_it INTEGER NOT NULL, author TEXT NOT NULL, body TEXT NOT NULL, at TEXT NOT NULL, read INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS helpdesk_by_endpoint ON helpdesk_messages(endpoint_id, id);
CREATE TABLE IF NOT EXISTS device_certs (
  fingerprint TEXT PRIMARY KEY, endpoint_id TEXT NOT NULL REFERENCES endpoints(id) ON DELETE CASCADE,
  not_after TEXT NOT NULL, issued_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS audit (
  id INTEGER PRIMARY KEY AUTOINCREMENT, at TEXT NOT NULL, actor TEXT NOT NULL,
  action TEXT NOT NULL, target TEXT NOT NULL, details TEXT NOT NULL DEFAULT ''
);
"#;

/// Seconds without a check-in before an endpoint shows as offline.
pub const ONLINE_WINDOW_SECS: i64 = 90;

#[derive(Clone)]
pub struct Db {
    conn: Arc<Mutex<Connection>>,
    /// Receives every audit entry, for webhooks and chat alerts.
    events: Arc<std::sync::OnceLock<tokio::sync::mpsc::UnboundedSender<AuditEntry>>>,
}

fn ts(t: DateTime<Utc>) -> String {
    t.to_rfc3339()
}
fn parse_ts(s: Option<String>) -> Option<DateTime<Utc>> {
    s.and_then(|s| DateTime::parse_from_rfc3339(&s).ok().map(|d| d.with_timezone(&Utc)))
}
fn req_ts(s: String) -> DateTime<Utc> {
    parse_ts(Some(s)).unwrap_or_else(Utc::now)
}
fn uid(s: String) -> Uuid {
    Uuid::parse_str(&s).unwrap_or_default()
}
fn opt_uid(s: Option<String>) -> Option<Uuid> {
    s.and_then(|s| Uuid::parse_str(&s).ok())
}
fn opt_str(u: Option<Uuid>) -> Option<String> {
    u.map(|u| u.to_string())
}

/// Secrets the server keeps for a user. Never serialized to clients.
pub struct UserSecrets {
    pub password_hash: Option<String>,
    pub password_sealed: Option<Vec<u8>>,
    pub password_version: i64,
}

pub struct Recycled {
    pub id: Uuid,
    pub kind: String,
    pub name: String,
    pub data: String,
    pub deleted_by: String,
    pub deleted_at: DateTime<Utc>,
}

pub struct EndpointAuth {
    pub id: Uuid,
    pub secret_hash: String,
    pub ou_id: Option<Uuid>,
    pub want_inventory: bool,
}

/// Columns added after v0.2. `CREATE TABLE IF NOT EXISTS` doesn't add
/// columns to existing databases, so each is added here when missing.
const ADDED_COLUMNS: &[(&str, &str, &str)] = &[("jobs", "script_id", "TEXT"), ("jobs", "batch_id", "TEXT")];

fn migrate(conn: &Connection) -> Result<()> {
    conn.execute_batch(SCHEMA)?;
    for (table, col, def) in ADDED_COLUMNS {
        let exists: bool = conn.prepare(&format!("SELECT 1 FROM pragma_table_info('{table}') WHERE name = ?"))?.exists([col])?;
        if !exists {
            conn.execute_batch(&format!("ALTER TABLE {table} ADD COLUMN {col} {def}"))?;
        }
    }
    Ok(())
}

impl Db {
    pub fn open(path: &Path) -> Result<Db> {
        let conn = Connection::open(path).with_context(|| format!("open database {}", path.display()))?;
        migrate(&conn)?;
        Ok(Db { conn: Arc::new(Mutex::new(conn)), events: Default::default() })
    }

    pub fn open_in_memory() -> Result<Db> {
        let conn = Connection::open_in_memory()?;
        migrate(&conn)?;
        Ok(Db { conn: Arc::new(Mutex::new(conn)), events: Default::default() })
    }

    pub fn lock(&self) -> MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Replace the whole database with `bytes` (a snapshot from another
    /// server), keeping this server's own rows in `keep` tables.
    pub fn restore_from(&self, bytes: &[u8], keep: &[&str]) -> Result<()> {
        let tmp = std::env::temp_dir().join(format!("od-restore-{}.db", Uuid::new_v4()));
        crate::crypto::write_private(&tmp, bytes)?;
        let res = (|| -> Result<()> {
            let mut conn = self.lock();
            let mut saved: Vec<(String, Vec<Vec<rusqlite::types::Value>>, usize)> = vec![];
            for t in keep {
                let mut st = conn.prepare(&format!("SELECT * FROM {t}"))?;
                let n = st.column_count();
                let rows = st
                    .query_map([], |r| (0..n).map(|i| r.get::<_, rusqlite::types::Value>(i)).collect::<rusqlite::Result<Vec<_>>>())?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                saved.push((t.to_string(), rows, n));
            }
            conn.restore(rusqlite::DatabaseName::Main, &tmp, None::<fn(rusqlite::backup::Progress)>)?;
            migrate(&conn)?;
            for (t, rows, n) in saved {
                let sql = format!("INSERT OR IGNORE INTO {t} VALUES ({})", vec!["?"; n].join(","));
                for row in rows {
                    let _ = conn.execute(&sql, rusqlite::params_from_iter(row));
                }
            }
            Ok(())
        })();
        let _ = std::fs::remove_file(&tmp);
        res
    }

    /// A consistent copy of the whole database, taken while it's in use.
    pub fn snapshot(&self) -> Result<Vec<u8>> {
        let tmp = std::env::temp_dir().join(format!("od-snapshot-{}.db", Uuid::new_v4()));
        self.lock().execute("VACUUM INTO ?", [tmp.to_string_lossy().to_string()])?;
        let bytes = std::fs::read(&tmp);
        let _ = std::fs::remove_file(&tmp);
        Ok(bytes?)
    }

    // ---------- meta ----------
    pub fn meta_get(&self, key: &str) -> Result<Option<String>> {
        Ok(self.lock().query_row("SELECT value FROM meta WHERE key = ?", [key], |r| r.get(0)).optional()?)
    }
    pub fn meta_set(&self, key: &str, value: &str) -> Result<()> {
        self.lock()
            .execute("INSERT INTO meta(key, value) VALUES(?, ?) ON CONFLICT(key) DO UPDATE SET value = excluded.value", [key, value])?;
        Ok(())
    }

    // ---------- audit ----------
    pub fn audit(&self, actor: &str, action: &str, target: &str, details: &str) {
        let at = Utc::now();
        let id = {
            let c = self.lock();
            let _ = c.execute(
                "INSERT INTO audit(at, actor, action, target, details) VALUES (?, ?, ?, ?, ?)",
                params![ts(at), actor, action, target, details],
            );
            c.last_insert_rowid()
        };
        if let Some(tx) = self.events.get() {
            let _ =
                tx.send(AuditEntry { id, at, actor: actor.into(), action: action.into(), target: target.into(), details: details.into() });
        }
    }
    /// Subscribe to audit entries as they're written (one subscriber).
    pub fn subscribe(&self) -> Option<tokio::sync::mpsc::UnboundedReceiver<AuditEntry>> {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        self.events.set(tx).ok().map(|_| rx)
    }
    /// Delete audit entries older than `days`. Returns how many were removed.
    pub fn prune_audit(&self, days: u32) -> Result<usize> {
        let cutoff = ts(Utc::now() - chrono::Duration::days(days as i64));
        Ok(self.lock().execute("DELETE FROM audit WHERE at < ?", [cutoff])?)
    }

    pub fn list_audit(&self, limit: u32, query: Option<&str>) -> Result<Vec<AuditEntry>> {
        let c = self.lock();
        let like = format!("%{}%", query.unwrap_or(""));
        let mut st = c.prepare(
            "SELECT id, at, actor, action, target, details FROM audit
             WHERE actor LIKE ?1 OR action LIKE ?1 OR target LIKE ?1 OR details LIKE ?1
             ORDER BY id DESC LIMIT ?2",
        )?;
        let rows = st.query_map(params![like, limit], |r| {
            Ok(AuditEntry {
                id: r.get(0)?,
                at: req_ts(r.get(1)?),
                actor: r.get(2)?,
                action: r.get(3)?,
                target: r.get(4)?,
                details: r.get(5)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    // ---------- OUs ----------
    fn map_ou(r: &Row) -> rusqlite::Result<OrgUnit> {
        Ok(OrgUnit {
            id: uid(r.get(0)?),
            name: r.get(1)?,
            description: r.get(2)?,
            parent_id: opt_uid(r.get(3)?),
            created_at: req_ts(r.get(4)?),
        })
    }
    pub fn list_ous(&self) -> Result<Vec<OrgUnit>> {
        let c = self.lock();
        let mut st = c.prepare("SELECT id, name, description, parent_id, created_at FROM ous ORDER BY name COLLATE NOCASE")?;
        let rows = st.query_map([], Self::map_ou)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }
    pub fn get_ou(&self, id: Uuid) -> Result<Option<OrgUnit>> {
        Ok(self
            .lock()
            .query_row("SELECT id, name, description, parent_id, created_at FROM ous WHERE id = ?", [id.to_string()], Self::map_ou)
            .optional()?)
    }
    pub fn insert_ou(&self, ou: &OrgUnit) -> Result<()> {
        self.lock().execute(
            "INSERT INTO ous(id, name, description, parent_id, created_at) VALUES (?, ?, ?, ?, ?)",
            params![ou.id.to_string(), ou.name, ou.description, opt_str(ou.parent_id), ts(ou.created_at)],
        )?;
        Ok(())
    }
    pub fn update_ou(&self, ou: &OrgUnit) -> Result<()> {
        self.lock().execute(
            "UPDATE ous SET name = ?, description = ?, parent_id = ? WHERE id = ?",
            params![ou.name, ou.description, opt_str(ou.parent_id), ou.id.to_string()],
        )?;
        Ok(())
    }
    /// Counts of objects that would block deleting an OU.
    pub fn ou_usage(&self, id: Uuid) -> Result<(i64, i64, i64, i64)> {
        let c = self.lock();
        let id = id.to_string();
        let q = |sql: &str| -> rusqlite::Result<i64> { c.query_row(sql, [&id], |r| r.get(0)) };
        Ok((
            q("SELECT COUNT(*) FROM ous WHERE parent_id = ?")?,
            q("SELECT COUNT(*) FROM users WHERE ou_id = ?")?,
            q("SELECT COUNT(*) FROM groups WHERE ou_id = ?")?,
            q("SELECT COUNT(*) FROM endpoints WHERE ou_id = ?")?,
        ))
    }
    pub fn delete_ou(&self, id: Uuid) -> Result<bool> {
        Ok(self.lock().execute("DELETE FROM ous WHERE id = ?", [id.to_string()])? > 0)
    }

    // ---------- users ----------
    const USER_COLS: &'static str = "id, username, display_name, email, title, department, ou_id, enabled, role,
        must_change_password, password_set_at, last_login_at, created_at";

    fn map_user(r: &Row) -> rusqlite::Result<User> {
        let role: String = r.get(8)?;
        Ok(User {
            id: uid(r.get(0)?),
            username: r.get(1)?,
            display_name: r.get(2)?,
            email: r.get(3)?,
            title: r.get(4)?,
            department: r.get(5)?,
            ou_id: opt_uid(r.get(6)?),
            enabled: r.get::<_, i64>(7)? != 0,
            role: if role == "admin" { Role::Admin } else { Role::User },
            must_change_password: r.get::<_, i64>(9)? != 0,
            password_set_at: parse_ts(r.get(10)?),
            last_login_at: parse_ts(r.get(11)?),
            created_at: req_ts(r.get(12)?),
            group_ids: vec![],
        })
    }

    fn fill_user_groups(c: &Connection, users: &mut [User]) -> rusqlite::Result<()> {
        let mut st = c.prepare("SELECT group_id FROM group_members WHERE user_id = ?")?;
        for u in users.iter_mut() {
            u.group_ids = st.query_map([u.id.to_string()], |r| r.get::<_, String>(0))?.filter_map(|g| g.ok().map(uid)).collect();
        }
        Ok(())
    }

    pub fn count_users(&self) -> Result<i64> {
        Ok(self.lock().query_row("SELECT COUNT(*) FROM users", [], |r| r.get(0))?)
    }

    pub fn list_users(&self) -> Result<Vec<User>> {
        let c = self.lock();
        let mut st = c.prepare(&format!("SELECT {} FROM users ORDER BY display_name COLLATE NOCASE", Self::USER_COLS))?;
        let mut users: Vec<User> = st.query_map([], Self::map_user)?.collect::<rusqlite::Result<_>>()?;
        Self::fill_user_groups(&c, &mut users)?;
        Ok(users)
    }

    pub fn get_user(&self, id: Uuid) -> Result<Option<User>> {
        let c = self.lock();
        let u = c.query_row(&format!("SELECT {} FROM users WHERE id = ?", Self::USER_COLS), [id.to_string()], Self::map_user).optional()?;
        Ok(match u {
            Some(u) => {
                let mut v = [u];
                Self::fill_user_groups(&c, &mut v)?;
                let [u] = v;
                Some(u)
            }
            None => None,
        })
    }

    pub fn get_user_by_username(&self, username: &str) -> Result<Option<User>> {
        let id: Option<String> = self.lock().query_row("SELECT id FROM users WHERE username = ?", [username], |r| r.get(0)).optional()?;
        match id {
            Some(id) => self.get_user(uid(id)),
            None => Ok(None),
        }
    }

    pub fn user_secrets(&self, id: Uuid) -> Result<UserSecrets> {
        Ok(self.lock().query_row(
            "SELECT password_hash, password_sealed, password_version FROM users WHERE id = ?",
            [id.to_string()],
            |r| Ok(UserSecrets { password_hash: r.get(0)?, password_sealed: r.get(1)?, password_version: r.get(2)? }),
        )?)
    }

    pub fn insert_user(&self, u: &User) -> Result<()> {
        self.lock().execute(
            "INSERT INTO users(id, username, display_name, email, title, department, ou_id, enabled, role, must_change_password, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            params![
                u.id.to_string(), u.username, u.display_name, u.email, u.title, u.department, opt_str(u.ou_id),
                u.enabled as i64, role_str(u.role), u.must_change_password as i64, ts(u.created_at)
            ],
        )?;
        Ok(())
    }

    pub fn update_user(&self, u: &User) -> Result<()> {
        self.lock().execute(
            "UPDATE users SET username = ?, display_name = ?, email = ?, title = ?, department = ?, ou_id = ?, enabled = ?, role = ?,
             must_change_password = ? WHERE id = ?",
            params![
                u.username,
                u.display_name,
                u.email,
                u.title,
                u.department,
                opt_str(u.ou_id),
                u.enabled as i64,
                role_str(u.role),
                u.must_change_password as i64,
                u.id.to_string()
            ],
        )?;
        Ok(())
    }

    pub fn set_user_password(&self, id: Uuid, hash: &str, sealed: &[u8]) -> Result<()> {
        self.lock().execute(
            "UPDATE users SET password_hash = ?, password_sealed = ?, password_version = password_version + 1, password_set_at = ? WHERE id = ?",
            params![hash, sealed, ts(Utc::now()), id.to_string()],
        )?;
        Ok(())
    }

    pub fn push_password_history(&self, id: Uuid, hash: &str) -> Result<()> {
        let c = self.lock();
        c.execute("INSERT INTO password_history(user_id, hash, set_at) VALUES (?, ?, ?)", params![id.to_string(), hash, ts(Utc::now())])?;
        // Keep at most 24, the most any policy remembers.
        c.execute(
            "DELETE FROM password_history WHERE user_id = ?1 AND rowid NOT IN (SELECT rowid FROM password_history WHERE user_id = ?1 ORDER BY set_at DESC LIMIT 24)",
            [id.to_string()],
        )?;
        Ok(())
    }
    pub fn password_history(&self, id: Uuid, n: u32) -> Result<Vec<String>> {
        let c = self.lock();
        let mut st = c.prepare("SELECT hash FROM password_history WHERE user_id = ? ORDER BY set_at DESC LIMIT ?")?;
        let rows = st.query_map(params![id.to_string(), n], |r| r.get(0))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }
    pub fn set_reset_code(&self, id: Uuid, code_hash: &str, expires: DateTime<Utc>) -> Result<()> {
        self.lock().execute(
            "INSERT INTO reset_codes(user_id, code_hash, expires_at) VALUES (?1, ?2, ?3) ON CONFLICT(user_id) DO UPDATE SET code_hash = ?2, expires_at = ?3",
            params![id.to_string(), code_hash, ts(expires)],
        )?;
        Ok(())
    }
    /// Use a reset code: true if it matched and hadn't expired. Codes work once.
    pub fn take_reset_code(&self, id: Uuid, code_hash: &str) -> Result<bool> {
        let c = self.lock();
        let row: Option<(String, String)> = c
            .query_row("SELECT code_hash, expires_at FROM reset_codes WHERE user_id = ?", [id.to_string()], |r| Ok((r.get(0)?, r.get(1)?)))
            .optional()?;
        let ok = row.map(|(h, exp)| crate::crypto::ct_eq(&h, code_hash) && req_ts(exp) > Utc::now()).unwrap_or(false);
        if ok {
            c.execute("DELETE FROM reset_codes WHERE user_id = ?", [id.to_string()])?;
        }
        Ok(ok)
    }

    pub fn touch_login(&self, id: Uuid) -> Result<()> {
        self.lock().execute("UPDATE users SET last_login_at = ? WHERE id = ?", params![ts(Utc::now()), id.to_string()])?;
        Ok(())
    }

    pub fn delete_user(&self, id: Uuid) -> Result<bool> {
        Ok(self.lock().execute("DELETE FROM users WHERE id = ?", [id.to_string()])? > 0)
    }

    pub fn count_enabled_admins(&self) -> Result<i64> {
        Ok(self.lock().query_row("SELECT COUNT(*) FROM users WHERE role = 'admin' AND enabled = 1", [], |r| r.get(0))?)
    }

    // ---------- groups ----------
    fn map_group(r: &Row) -> rusqlite::Result<Group> {
        Ok(Group {
            id: uid(r.get(0)?),
            name: r.get(1)?,
            description: r.get(2)?,
            ou_id: opt_uid(r.get(3)?),
            member_ids: vec![],
            endpoint_ids: vec![],
            created_at: req_ts(r.get(4)?),
        })
    }
    fn fill_members(c: &Connection, groups: &mut [Group]) -> rusqlite::Result<()> {
        let mut st = c.prepare("SELECT user_id FROM group_members WHERE group_id = ?")?;
        for g in groups.iter_mut() {
            g.member_ids = st.query_map([g.id.to_string()], |r| r.get::<_, String>(0))?.filter_map(|u| u.ok().map(uid)).collect();
        }
        let mut st = c.prepare("SELECT endpoint_id FROM group_endpoints WHERE group_id = ?")?;
        for g in groups.iter_mut() {
            g.endpoint_ids = st.query_map([g.id.to_string()], |r| r.get::<_, String>(0))?.filter_map(|u| u.ok().map(uid)).collect();
        }
        Ok(())
    }
    pub fn list_groups(&self) -> Result<Vec<Group>> {
        let c = self.lock();
        let mut st = c.prepare("SELECT id, name, description, ou_id, created_at FROM groups ORDER BY name COLLATE NOCASE")?;
        let mut gs: Vec<Group> = st.query_map([], Self::map_group)?.collect::<rusqlite::Result<_>>()?;
        Self::fill_members(&c, &mut gs)?;
        Ok(gs)
    }
    pub fn get_group(&self, id: Uuid) -> Result<Option<Group>> {
        let c = self.lock();
        let g = c
            .query_row("SELECT id, name, description, ou_id, created_at FROM groups WHERE id = ?", [id.to_string()], Self::map_group)
            .optional()?;
        Ok(match g {
            Some(g) => {
                let mut v = [g];
                Self::fill_members(&c, &mut v)?;
                let [g] = v;
                Some(g)
            }
            None => None,
        })
    }
    pub fn insert_group(&self, g: &Group) -> Result<()> {
        self.lock().execute(
            "INSERT INTO groups(id, name, description, ou_id, created_at) VALUES (?, ?, ?, ?, ?)",
            params![g.id.to_string(), g.name, g.description, opt_str(g.ou_id), ts(g.created_at)],
        )?;
        Ok(())
    }
    pub fn update_group(&self, g: &Group) -> Result<()> {
        self.lock().execute(
            "UPDATE groups SET name = ?, description = ?, ou_id = ? WHERE id = ?",
            params![g.name, g.description, opt_str(g.ou_id), g.id.to_string()],
        )?;
        Ok(())
    }
    pub fn set_group_members(&self, group: Uuid, members: &[Uuid]) -> Result<()> {
        let mut c = self.lock();
        let tx = c.transaction()?;
        tx.execute("DELETE FROM group_members WHERE group_id = ?", [group.to_string()])?;
        for m in members {
            tx.execute("INSERT OR IGNORE INTO group_members(group_id, user_id) VALUES (?, ?)", [group.to_string(), m.to_string()])?;
        }
        tx.commit()?;
        Ok(())
    }
    pub fn set_group_endpoints(&self, group: Uuid, endpoints: &[Uuid]) -> Result<()> {
        let mut c = self.lock();
        let tx = c.transaction()?;
        tx.execute("DELETE FROM group_endpoints WHERE group_id = ?", [group.to_string()])?;
        for e in endpoints {
            tx.execute("INSERT OR IGNORE INTO group_endpoints(group_id, endpoint_id) VALUES (?, ?)", [group.to_string(), e.to_string()])?;
        }
        tx.commit()?;
        Ok(())
    }
    pub fn set_user_groups(&self, user: Uuid, groups: &[Uuid]) -> Result<()> {
        let mut c = self.lock();
        let tx = c.transaction()?;
        tx.execute("DELETE FROM group_members WHERE user_id = ?", [user.to_string()])?;
        for g in groups {
            tx.execute("INSERT OR IGNORE INTO group_members(group_id, user_id) VALUES (?, ?)", [g.to_string(), user.to_string()])?;
        }
        tx.commit()?;
        Ok(())
    }
    pub fn delete_group(&self, id: Uuid) -> Result<bool> {
        Ok(self.lock().execute("DELETE FROM groups WHERE id = ?", [id.to_string()])? > 0)
    }

    // ---------- endpoints ----------
    const DEVICE_COLS: &'static str =
        "id, hostname, os, ou_id, notes, agent_version, enrolled_at, last_seen_at, inventory, compliance, public_ip";

    fn map_endpoint(r: &Row) -> rusqlite::Result<Endpoint> {
        let os: String = r.get(2)?;
        let last_seen = parse_ts(r.get(7)?);
        let inv: String = r.get(8)?;
        let comp: String = r.get(9)?;
        Ok(Endpoint {
            id: uid(r.get(0)?),
            hostname: r.get(1)?,
            os: serde_json::from_value(serde_json::Value::String(os)).unwrap_or(OsFamily::Linux),
            ou_id: opt_uid(r.get(3)?),
            notes: r.get(4)?,
            agent_version: r.get(5)?,
            enrolled_at: req_ts(r.get(6)?),
            online: last_seen.map(|t| (Utc::now() - t).num_seconds() < ONLINE_WINDOW_SECS).unwrap_or(false),
            last_seen_at: last_seen,
            inventory: serde_json::from_str(&inv).unwrap_or_default(),
            compliance: serde_json::from_str(&comp).unwrap_or_default(),
            public_ip: r.get(10)?,
        })
    }
    pub fn list_endpoints(&self) -> Result<Vec<Endpoint>> {
        let c = self.lock();
        let mut st = c.prepare(&format!("SELECT {} FROM endpoints ORDER BY hostname COLLATE NOCASE", Self::DEVICE_COLS))?;
        let rows = st.query_map([], Self::map_endpoint)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }
    pub fn get_endpoint(&self, id: Uuid) -> Result<Option<Endpoint>> {
        Ok(self
            .lock()
            .query_row(&format!("SELECT {} FROM endpoints WHERE id = ?", Self::DEVICE_COLS), [id.to_string()], Self::map_endpoint)
            .optional()?)
    }
    pub fn endpoint_auth(&self, id: Uuid) -> Result<Option<EndpointAuth>> {
        Ok(self
            .lock()
            .query_row("SELECT id, secret_hash, ou_id, want_inventory FROM endpoints WHERE id = ?", [id.to_string()], |r| {
                Ok(EndpointAuth {
                    id: uid(r.get(0)?),
                    secret_hash: r.get(1)?,
                    ou_id: opt_uid(r.get(2)?),
                    want_inventory: r.get::<_, i64>(3)? != 0,
                })
            })
            .optional()?)
    }
    pub fn insert_endpoint(&self, d: &Endpoint, secret_hash: &str) -> Result<()> {
        self.lock().execute(
            "INSERT INTO endpoints(id, hostname, os, ou_id, notes, agent_version, secret_hash, enrolled_at, last_seen_at, inventory, compliance, public_ip)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, '[]', ?)",
            params![
                d.id.to_string(), d.hostname, os_str(d.os), opt_str(d.ou_id), d.notes, d.agent_version, secret_hash,
                ts(d.enrolled_at), d.last_seen_at.map(ts), serde_json::to_string(&d.inventory)?, d.public_ip
            ],
        )?;
        Ok(())
    }
    pub fn update_endpoint_admin(&self, id: Uuid, ou_id: Option<Uuid>, notes: &str) -> Result<()> {
        self.lock().execute("UPDATE endpoints SET ou_id = ?, notes = ? WHERE id = ?", params![opt_str(ou_id), notes, id.to_string()])?;
        Ok(())
    }
    pub fn record_checkin(
        &self,
        id: Uuid,
        agent_version: &str,
        inventory: Option<&Inventory>,
        compliance: &[ComplianceItem],
        public_ip: &str,
    ) -> Result<()> {
        let c = self.lock();
        let now = ts(Utc::now());
        match inventory {
            Some(inv) => c.execute(
                "UPDATE endpoints SET agent_version = ?, last_seen_at = ?, inventory = ?, hostname = ?, compliance = ?, public_ip = ?, want_inventory = 0 WHERE id = ?",
                params![agent_version, now, serde_json::to_string(inv)?, inv.hostname, serde_json::to_string(compliance)?, public_ip, id.to_string()],
            )?,
            None => c.execute(
                "UPDATE endpoints SET agent_version = ?, last_seen_at = ?, compliance = ?, public_ip = ? WHERE id = ?",
                params![agent_version, now, serde_json::to_string(compliance)?, public_ip, id.to_string()],
            )?,
        };
        Ok(())
    }
    pub fn request_inventory(&self, id: Uuid) -> Result<()> {
        self.lock().execute("UPDATE endpoints SET want_inventory = 1 WHERE id = ?", [id.to_string()])?;
        Ok(())
    }
    pub fn delete_endpoint(&self, id: Uuid) -> Result<bool> {
        Ok(self.lock().execute("DELETE FROM endpoints WHERE id = ?", [id.to_string()])? > 0)
    }

    // ---------- Internet Gateway device certificates ----------
    /// Register a newly issued device certificate, keeping only it and the
    /// one before (so a renewal can't lock a device out mid-way).
    pub fn add_device_cert(&self, endpoint: Uuid, fingerprint: &str, not_after: DateTime<Utc>) -> Result<()> {
        let conn = self.lock();
        conn.execute(
            "DELETE FROM device_certs WHERE endpoint_id = ?1 AND fingerprint NOT IN
               (SELECT fingerprint FROM device_certs WHERE endpoint_id = ?1 ORDER BY issued_at DESC LIMIT 1)",
            [endpoint.to_string()],
        )?;
        conn.execute(
            "INSERT OR REPLACE INTO device_certs(fingerprint, endpoint_id, not_after, issued_at) VALUES(?, ?, ?, ?)",
            rusqlite::params![fingerprint, endpoint.to_string(), ts(not_after), ts(Utc::now())],
        )?;
        Ok(())
    }
    pub fn device_cert_owner(&self, fingerprint: &str) -> Result<Option<Uuid>> {
        let id: Option<String> = self
            .lock()
            .query_row(
                "SELECT endpoint_id FROM device_certs WHERE fingerprint = ? AND not_after > ?",
                [fingerprint, &ts(Utc::now())],
                |r| r.get(0),
            )
            .optional()?;
        Ok(id.map(uid))
    }
    pub fn device_cert_count(&self, endpoint: Uuid) -> Result<i64> {
        Ok(self.lock().query_row("SELECT COUNT(*) FROM device_certs WHERE endpoint_id = ?", [endpoint.to_string()], |r| r.get(0))?)
    }
    pub fn device_cert_endpoints(&self) -> Result<i64> {
        Ok(self.lock().query_row("SELECT COUNT(DISTINCT endpoint_id) FROM device_certs", [], |r| r.get(0))?)
    }

    // ---------- help desk ----------
    pub fn add_chat(&self, endpoint: Uuid, from_it: bool, author: &str, text: &str) -> Result<od_core::protocol::ChatMessage> {
        let at = Utc::now();
        let conn = self.lock();
        // Messages from IT start out read; the user's start out unread.
        conn.execute(
            "INSERT INTO helpdesk_messages(endpoint_id, from_it, author, body, at, read) VALUES(?, ?, ?, ?, ?, ?)",
            rusqlite::params![endpoint.to_string(), from_it, author, text, ts(at), from_it],
        )?;
        Ok(od_core::protocol::ChatMessage { id: conn.last_insert_rowid(), from_it, author: author.into(), text: text.into(), at })
    }
    /// Messages for one endpoint after `since`, oldest first, at most `limit`
    /// (the newest ones when there are more).
    pub fn chat_after(&self, endpoint: Uuid, since: i64, limit: u32) -> Result<Vec<od_core::protocol::ChatMessage>> {
        let conn = self.lock();
        let mut st = conn.prepare(
            "SELECT id, from_it, author, body, at FROM helpdesk_messages WHERE endpoint_id = ? AND id > ? ORDER BY id DESC LIMIT ?",
        )?;
        let mut v = st
            .query_map(rusqlite::params![endpoint.to_string(), since, limit], |r| {
                Ok(od_core::protocol::ChatMessage {
                    id: r.get(0)?,
                    from_it: r.get(1)?,
                    author: r.get(2)?,
                    text: r.get(3)?,
                    at: req_ts(r.get(4)?),
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        v.reverse();
        Ok(v)
    }
    pub fn mark_chat_read(&self, endpoint: Uuid) -> Result<()> {
        self.lock().execute("UPDATE helpdesk_messages SET read = 1 WHERE endpoint_id = ? AND read = 0", [endpoint.to_string()])?;
        Ok(())
    }
    /// One row per endpoint with messages: (endpoint, last message, unread count).
    pub fn chat_threads(&self) -> Result<Vec<(Uuid, od_core::protocol::ChatMessage, i64)>> {
        let conn = self.lock();
        let mut st = conn.prepare(
            "SELECT m.endpoint_id, m.id, m.from_it, m.author, m.body, m.at,
                    (SELECT COUNT(*) FROM helpdesk_messages u WHERE u.endpoint_id = m.endpoint_id AND u.read = 0)
             FROM helpdesk_messages m
             WHERE m.id = (SELECT MAX(id) FROM helpdesk_messages x WHERE x.endpoint_id = m.endpoint_id)
             ORDER BY m.id DESC",
        )?;
        let v = st
            .query_map([], |r| {
                Ok((
                    uid(r.get(0)?),
                    od_core::protocol::ChatMessage {
                        id: r.get(1)?,
                        from_it: r.get(2)?,
                        author: r.get(3)?,
                        text: r.get(4)?,
                        at: req_ts(r.get(5)?),
                    },
                    r.get(6)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(v)
    }

    // ---------- enrollment tokens ----------
    fn map_token(r: &Row) -> rusqlite::Result<EnrollmentToken> {
        Ok(EnrollmentToken {
            token: r.get(0)?,
            description: r.get(1)?,
            ou_id: opt_uid(r.get(2)?),
            uses: r.get::<_, i64>(3)? as u32,
            max_uses: r.get::<_, Option<i64>>(4)?.map(|v| v as u32),
            expires_at: parse_ts(r.get(5)?),
            created_at: req_ts(r.get(6)?),
            created_by: r.get(7)?,
        })
    }
    pub fn list_tokens(&self) -> Result<Vec<EnrollmentToken>> {
        let c = self.lock();
        let mut st = c.prepare(
            "SELECT token, description, ou_id, uses, max_uses, expires_at, created_at, created_by FROM enrollment_tokens ORDER BY created_at DESC",
        )?;
        let rows = st.query_map([], Self::map_token)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }
    pub fn insert_token(&self, t: &EnrollmentToken) -> Result<()> {
        self.lock().execute(
            "INSERT INTO enrollment_tokens(token, description, ou_id, uses, max_uses, expires_at, created_at, created_by) VALUES (?, ?, ?, 0, ?, ?, ?, ?)",
            params![t.token, t.description, opt_str(t.ou_id), t.max_uses.map(|v| v as i64), t.expires_at.map(ts), ts(t.created_at), t.created_by],
        )?;
        Ok(())
    }
    /// The token if it's still valid, without using it up.
    pub fn valid_token(&self, token: &str) -> Result<Option<EnrollmentToken>> {
        let t = self
            .lock()
            .query_row(
                "SELECT token, description, ou_id, uses, max_uses, expires_at, created_at, created_by FROM enrollment_tokens WHERE token = ?",
                [token],
                Self::map_token,
            )
            .optional()?;
        Ok(t.filter(|t| !(t.expires_at.map(|e| e < Utc::now()).unwrap_or(false) || t.max_uses.map(|m| t.uses >= m).unwrap_or(false))))
    }
    /// Atomically validate and consume one use of a token.
    pub fn consume_token(&self, token: &str) -> Result<Option<EnrollmentToken>> {
        let c = self.lock();
        let t = c
            .query_row(
                "SELECT token, description, ou_id, uses, max_uses, expires_at, created_at, created_by FROM enrollment_tokens WHERE token = ?",
                [token],
                Self::map_token,
            )
            .optional()?;
        let Some(t) = t else { return Ok(None) };
        if t.expires_at.map(|e| e < Utc::now()).unwrap_or(false) || t.max_uses.map(|m| t.uses >= m).unwrap_or(false) {
            return Ok(None);
        }
        c.execute("UPDATE enrollment_tokens SET uses = uses + 1 WHERE token = ?", [token])?;
        Ok(Some(t))
    }
    pub fn delete_token(&self, token: &str) -> Result<bool> {
        Ok(self.lock().execute("DELETE FROM enrollment_tokens WHERE token = ?", [token])? > 0)
    }

    // ---------- jobs ----------
    fn map_job(r: &Row) -> rusqlite::Result<Job> {
        let kind: String = r.get(2)?;
        let status: String = r.get(3)?;
        Ok(Job {
            id: uid(r.get(0)?),
            endpoint_id: uid(r.get(1)?),
            kind: serde_json::from_str(&kind).unwrap_or(JobKind::RefreshInventory),
            status: serde_json::from_value(serde_json::Value::String(status)).unwrap_or(JobStatus::Failed),
            exit_code: r.get(4)?,
            output: r.get(5)?,
            created_by: r.get(6)?,
            created_at: req_ts(r.get(7)?),
            started_at: parse_ts(r.get(8)?),
            finished_at: parse_ts(r.get(9)?),
            script_id: opt_uid(r.get(10)?),
            batch_id: opt_uid(r.get(11)?),
        })
    }
    const JOB_COLS: &'static str =
        "id, endpoint_id, kind, status, exit_code, output, created_by, created_at, started_at, finished_at, script_id, batch_id";

    pub fn insert_job(&self, j: &Job) -> Result<()> {
        self.lock().execute(
            "INSERT INTO jobs(id, endpoint_id, kind, status, created_by, created_at, script_id, batch_id) VALUES (?, ?, ?, 'queued', ?, ?, ?, ?)",
            params![
                j.id.to_string(), j.endpoint_id.to_string(), serde_json::to_string(&j.kind)?, j.created_by, ts(j.created_at),
                opt_str(j.script_id), opt_str(j.batch_id)
            ],
        )?;
        Ok(())
    }
    /// Record a run that already happened on the endpoint (GPO scripts).
    pub fn insert_finished_job(&self, j: &Job) -> Result<()> {
        self.lock().execute(
            "INSERT INTO jobs(id, endpoint_id, kind, status, exit_code, output, created_by, created_at, started_at, finished_at, script_id)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            params![
                j.id.to_string(),
                j.endpoint_id.to_string(),
                serde_json::to_string(&j.kind)?,
                if j.status == JobStatus::Succeeded { "succeeded" } else { "failed" },
                j.exit_code,
                j.output,
                j.created_by,
                ts(j.created_at),
                j.started_at.map(ts),
                j.finished_at.map(ts),
                opt_str(j.script_id)
            ],
        )?;
        Ok(())
    }
    pub fn list_batch_jobs(&self, batch: Uuid) -> Result<Vec<Job>> {
        let c = self.lock();
        let mut st = c.prepare(&format!("SELECT {} FROM jobs WHERE batch_id = ? ORDER BY created_at", Self::JOB_COLS))?;
        let rows = st.query_map([batch.to_string()], Self::map_job)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }
    pub fn list_script_jobs(&self, script: Uuid, limit: u32) -> Result<Vec<Job>> {
        let c = self.lock();
        let mut st = c.prepare(&format!("SELECT {} FROM jobs WHERE script_id = ? ORDER BY created_at DESC LIMIT ?", Self::JOB_COLS))?;
        let rows = st.query_map(params![script.to_string(), limit], Self::map_job)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }
    pub fn list_jobs(&self, endpoint: Option<Uuid>, limit: u32) -> Result<Vec<Job>> {
        let c = self.lock();
        let (sql, dev) = match endpoint {
            Some(d) => {
                (format!("SELECT {} FROM jobs WHERE endpoint_id = ?1 ORDER BY created_at DESC LIMIT ?2", Self::JOB_COLS), d.to_string())
            }
            None => (format!("SELECT {} FROM jobs WHERE ?1 = ?1 ORDER BY created_at DESC LIMIT ?2", Self::JOB_COLS), String::new()),
        };
        let mut st = c.prepare(&sql)?;
        let rows = st.query_map(params![dev, limit], Self::map_job)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }
    pub fn get_job(&self, id: Uuid) -> Result<Option<Job>> {
        Ok(self
            .lock()
            .query_row(&format!("SELECT {} FROM jobs WHERE id = ?", Self::JOB_COLS), [id.to_string()], Self::map_job)
            .optional()?)
    }
    /// Hand queued jobs to the agent and mark them running.
    pub fn claim_jobs(&self, endpoint: Uuid) -> Result<Vec<Job>> {
        let mut c = self.lock();
        let tx = c.transaction()?;
        let jobs: Vec<Job> = {
            let mut st = tx.prepare(&format!(
                "SELECT {} FROM jobs WHERE endpoint_id = ? AND status = 'queued' ORDER BY created_at LIMIT 20",
                Self::JOB_COLS
            ))?;
            let rows = st.query_map([endpoint.to_string()], Self::map_job)?;
            rows.collect::<rusqlite::Result<_>>()?
        };
        let now = ts(Utc::now());
        for j in &jobs {
            tx.execute("UPDATE jobs SET status = 'running', started_at = ? WHERE id = ?", params![now, j.id.to_string()])?;
        }
        tx.commit()?;
        Ok(jobs)
    }
    pub fn finish_job(&self, id: Uuid, endpoint: Uuid, success: bool, exit_code: Option<i32>, output: &str) -> Result<bool> {
        let n = self.lock().execute(
            "UPDATE jobs SET status = ?, exit_code = ?, output = ?, finished_at = ? WHERE id = ? AND endpoint_id = ? AND status = 'running'",
            params![if success { "succeeded" } else { "failed" }, exit_code, output, ts(Utc::now()), id.to_string(), endpoint.to_string()],
        )?;
        Ok(n > 0)
    }
    pub fn cancel_job(&self, id: Uuid) -> Result<bool> {
        Ok(self.lock().execute(
            "UPDATE jobs SET status = 'cancelled', finished_at = ? WHERE id = ? AND status = 'queued'",
            params![ts(Utc::now()), id.to_string()],
        )? > 0)
    }

    // ---------- backup plans (sealed: destination credentials inside) ----------
    pub fn list_backup_plans(&self) -> Result<Vec<(Uuid, Vec<u8>)>> {
        let c = self.lock();
        let mut st = c.prepare("SELECT id, sealed FROM backup_plans")?;
        let rows = st.query_map([], |r| Ok((uid(r.get(0)?), r.get::<_, Vec<u8>>(1)?)))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }
    pub fn get_backup_plan(&self, id: Uuid) -> Result<Option<Vec<u8>>> {
        Ok(self.lock().query_row("SELECT sealed FROM backup_plans WHERE id = ?", [id.to_string()], |r| r.get(0)).optional()?)
    }
    pub fn upsert_backup_plan(&self, id: Uuid, name: &str, sealed: &[u8]) -> Result<()> {
        self.lock().execute(
            "INSERT INTO backup_plans(id, name, sealed, updated_at) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(id) DO UPDATE SET name = ?2, sealed = ?3, updated_at = ?4",
            params![id.to_string(), name, sealed, ts(Utc::now())],
        )?;
        Ok(())
    }
    pub fn delete_backup_plan(&self, id: Uuid) -> Result<bool> {
        Ok(self.lock().execute("DELETE FROM backup_plans WHERE id = ?", [id.to_string()])? > 0)
    }

    // ---------- integrations (sealed: credentials inside) ----------
    pub fn list_integrations(&self) -> Result<Vec<(Uuid, Vec<u8>)>> {
        let c = self.lock();
        let mut st = c.prepare("SELECT id, sealed FROM integrations")?;
        let rows = st.query_map([], |r| Ok((uid(r.get(0)?), r.get::<_, Vec<u8>>(1)?)))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }
    pub fn upsert_integration(&self, id: Uuid, kind: &str, sealed: &[u8]) -> Result<()> {
        self.lock().execute(
            "INSERT INTO integrations(id, kind, sealed, updated_at) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(id) DO UPDATE SET kind = ?2, sealed = ?3, updated_at = ?4",
            params![id.to_string(), kind, sealed, ts(Utc::now())],
        )?;
        Ok(())
    }
    pub fn delete_integration(&self, id: Uuid) -> Result<bool> {
        Ok(self.lock().execute("DELETE FROM integrations WHERE id = ?", [id.to_string()])? > 0)
    }

    // ---------- LAPS ----------
    pub fn store_laps(&self, endpoint: Uuid, account: &str, sealed: &[u8]) -> Result<()> {
        self.lock().execute(
            "INSERT INTO laps(endpoint_id, account, password_sealed, previous_sealed, rotated_at, rotate_requested) VALUES (?1, ?2, ?3, NULL, ?4, 0)
             ON CONFLICT(endpoint_id) DO UPDATE SET previous_sealed = password_sealed, account = ?2, password_sealed = ?3, rotated_at = ?4, rotate_requested = 0",
            params![endpoint.to_string(), account, sealed, ts(Utc::now())],
        )?;
        Ok(())
    }
    /// (account, current sealed, previous sealed, rotated_at, rotate_requested)
    #[allow(clippy::type_complexity)]
    pub fn get_laps(&self, endpoint: Uuid) -> Result<Option<(String, Vec<u8>, Option<Vec<u8>>, DateTime<Utc>, bool)>> {
        Ok(self
            .lock()
            .query_row(
                "SELECT account, password_sealed, previous_sealed, rotated_at, rotate_requested FROM laps WHERE endpoint_id = ?",
                [endpoint.to_string()],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, req_ts(r.get(3)?), r.get::<_, i64>(4)? != 0)),
            )
            .optional()?)
    }
    pub fn request_laps_rotation(&self, endpoint: Uuid, on: bool) -> Result<bool> {
        Ok(self.lock().execute("UPDATE laps SET rotate_requested = ? WHERE endpoint_id = ?", params![on as i64, endpoint.to_string()])? > 0)
    }

    // ---------- recycle bin ----------
    pub fn insert_recycled(&self, id: Uuid, kind: &str, name: &str, data: &str, by: &str) -> Result<()> {
        self.lock().execute(
            "INSERT INTO recycle_bin(id, kind, name, data, deleted_by, deleted_at) VALUES (?, ?, ?, ?, ?, ?)",
            params![id.to_string(), kind, name, data, by, ts(Utc::now())],
        )?;
        Ok(())
    }
    pub fn list_recycled(&self) -> Result<Vec<Recycled>> {
        let c = self.lock();
        let mut st = c.prepare("SELECT id, kind, name, data, deleted_by, deleted_at FROM recycle_bin ORDER BY deleted_at DESC")?;
        let rows = st.query_map([], Self::map_recycled)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }
    pub fn get_recycled(&self, id: Uuid) -> Result<Option<Recycled>> {
        Ok(self
            .lock()
            .query_row(
                "SELECT id, kind, name, data, deleted_by, deleted_at FROM recycle_bin WHERE id = ?",
                [id.to_string()],
                Self::map_recycled,
            )
            .optional()?)
    }
    fn map_recycled(r: &Row) -> rusqlite::Result<Recycled> {
        Ok(Recycled {
            id: uid(r.get(0)?),
            kind: r.get(1)?,
            name: r.get(2)?,
            data: r.get(3)?,
            deleted_by: r.get(4)?,
            deleted_at: req_ts(r.get(5)?),
        })
    }
    pub fn delete_recycled(&self, id: Uuid) -> Result<bool> {
        Ok(self.lock().execute("DELETE FROM recycle_bin WHERE id = ?", [id.to_string()])? > 0)
    }
    pub fn purge_recycled_before(&self, before: DateTime<Utc>) -> Result<usize> {
        Ok(self.lock().execute("DELETE FROM recycle_bin WHERE deleted_at < ?", [ts(before)])?)
    }

    // ---------- scripts ----------
    fn map_script(r: &Row) -> rusqlite::Result<od_core::scripts::Script> {
        let shell: String = r.get(4)?;
        let os: String = r.get(6)?;
        let ps: String = r.get(7)?;
        Ok(od_core::scripts::Script {
            id: uid(r.get(0)?),
            name: r.get(1)?,
            description: r.get(2)?,
            category: r.get(3)?,
            shell: serde_json::from_value(serde_json::Value::String(shell)).unwrap_or(Shell::Default),
            body: r.get(5)?,
            os: serde_json::from_str(&os).unwrap_or_default(),
            params: serde_json::from_str(&ps).unwrap_or_default(),
            template: r.get(8)?,
            created_by: r.get(9)?,
            created_at: req_ts(r.get(10)?),
            updated_at: req_ts(r.get(11)?),
        })
    }
    const SCRIPT_COLS: &'static str =
        "id, name, description, category, shell, body, os, params, template, created_by, created_at, updated_at";
    pub fn list_scripts(&self) -> Result<Vec<od_core::scripts::Script>> {
        let c = self.lock();
        let mut st =
            c.prepare(&format!("SELECT {} FROM scripts ORDER BY category COLLATE NOCASE, name COLLATE NOCASE", Self::SCRIPT_COLS))?;
        let rows = st.query_map([], Self::map_script)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }
    pub fn get_script(&self, id: Uuid) -> Result<Option<od_core::scripts::Script>> {
        Ok(self
            .lock()
            .query_row(&format!("SELECT {} FROM scripts WHERE id = ?", Self::SCRIPT_COLS), [id.to_string()], Self::map_script)
            .optional()?)
    }
    pub fn upsert_script(&self, sc: &od_core::scripts::Script) -> Result<()> {
        let shell = serde_json::to_value(&sc.shell)?.as_str().unwrap_or("default").to_string();
        self.lock().execute(
            "INSERT INTO scripts(id, name, description, category, shell, body, os, params, template, created_by, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
             ON CONFLICT(id) DO UPDATE SET name = ?2, description = ?3, category = ?4, shell = ?5, body = ?6, os = ?7, params = ?8, updated_at = ?12",
            params![
                sc.id.to_string(), sc.name, sc.description, sc.category, shell, sc.body, serde_json::to_string(&sc.os)?,
                serde_json::to_string(&sc.params)?, sc.template, sc.created_by, ts(sc.created_at), ts(sc.updated_at)
            ],
        )?;
        Ok(())
    }
    pub fn delete_script(&self, id: Uuid) -> Result<bool> {
        Ok(self.lock().execute("DELETE FROM scripts WHERE id = ?", [id.to_string()])? > 0)
    }

    // ---------- policies ----------
    fn map_policy(r: &Row) -> rusqlite::Result<Policy> {
        let targets: String = r.get(5)?;
        let settings: String = r.get(6)?;
        Ok(Policy {
            id: uid(r.get(0)?),
            name: r.get(1)?,
            description: r.get(2)?,
            enabled: r.get::<_, i64>(3)? != 0,
            priority: r.get::<_, i64>(4)? as i32,
            targets: serde_json::from_str::<PolicyTargets>(&targets).unwrap_or_default(),
            settings: serde_json::from_str::<Vec<Setting>>(&settings).unwrap_or_default(),
            created_at: req_ts(r.get(7)?),
            updated_at: req_ts(r.get(8)?),
        })
    }
    const POLICY_COLS: &'static str = "id, name, description, enabled, priority, targets, settings, created_at, updated_at";
    pub fn list_policies(&self) -> Result<Vec<Policy>> {
        let c = self.lock();
        let mut st = c.prepare(&format!("SELECT {} FROM policies ORDER BY priority, name COLLATE NOCASE", Self::POLICY_COLS))?;
        let rows = st.query_map([], Self::map_policy)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }
    pub fn get_policy(&self, id: Uuid) -> Result<Option<Policy>> {
        Ok(self
            .lock()
            .query_row(&format!("SELECT {} FROM policies WHERE id = ?", Self::POLICY_COLS), [id.to_string()], Self::map_policy)
            .optional()?)
    }
    pub fn upsert_policy(&self, p: &Policy) -> Result<()> {
        self.lock().execute(
            "INSERT INTO policies(id, name, description, enabled, priority, targets, settings, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT(id) DO UPDATE SET name = ?2, description = ?3, enabled = ?4, priority = ?5, targets = ?6, settings = ?7, updated_at = ?9",
            params![
                p.id.to_string(), p.name, p.description, p.enabled as i64, p.priority, serde_json::to_string(&p.targets)?,
                serde_json::to_string(&p.settings)?, ts(p.created_at), ts(p.updated_at)
            ],
        )?;
        Ok(())
    }
    pub fn delete_policy(&self, id: Uuid) -> Result<bool> {
        Ok(self.lock().execute("DELETE FROM policies WHERE id = ?", [id.to_string()])? > 0)
    }

    // ---------- sessions ----------
    pub fn insert_session(&self, token_hash: &str, user: Uuid, expires: DateTime<Utc>) -> Result<()> {
        self.lock().execute(
            "INSERT INTO sessions(token_hash, user_id, created_at, expires_at) VALUES (?, ?, ?, ?)",
            params![token_hash, user.to_string(), ts(Utc::now()), ts(expires)],
        )?;
        Ok(())
    }
    pub fn session_user(&self, token_hash: &str) -> Result<Option<Uuid>> {
        let row: Option<(String, String)> = self
            .lock()
            .query_row("SELECT user_id, expires_at FROM sessions WHERE token_hash = ?", [token_hash], |r| Ok((r.get(0)?, r.get(1)?)))
            .optional()?;
        Ok(match row {
            Some((u, exp)) if req_ts(exp.clone()) > Utc::now() => Some(uid(u)),
            _ => None,
        })
    }
    pub fn delete_session(&self, token_hash: &str) -> Result<()> {
        self.lock().execute("DELETE FROM sessions WHERE token_hash = ?", [token_hash])?;
        Ok(())
    }
    pub fn delete_user_sessions(&self, user: Uuid) -> Result<()> {
        self.lock().execute("DELETE FROM sessions WHERE user_id = ?", [user.to_string()])?;
        Ok(())
    }
}

fn role_str(r: Role) -> &'static str {
    match r {
        Role::Admin => "admin",
        Role::User => "user",
    }
}
fn os_str(o: OsFamily) -> &'static str {
    match o {
        OsFamily::Windows => "windows",
        OsFamily::Macos => "macos",
        OsFamily::Linux => "linux",
        OsFamily::Android => "android",
    }
}
