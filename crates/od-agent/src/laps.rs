//! Local administrator password rotation (like Windows LAPS). The agent keeps
//! one local admin account with a random password, rotates it on schedule or
//! on request, and escrows each new password on the Management Server before
//! setting it, so the server always knows the current one.

use chrono::Utc;
use od_core::policy::Setting;
use od_core::protocol::LapsEscrow;
use od_core::{ComplianceItem, ComplianceState};
use rand::Rng;

use crate::accounts::{os, safe_name};
use crate::state::{AgentState, LapsState};

fn item(state: ComplianceState, message: impl Into<String>) -> ComplianceItem {
    ComplianceItem { setting: "local_admin_password".into(), state, message: message.into() }
}

/// A random password with upper, lower, digits and symbols, without
/// look-alike characters.
pub fn generate(len: usize) -> String {
    const SETS: [&[u8]; 4] = [b"ABCDEFGHJKLMNPQRSTUVWXYZ", b"abcdefghijkmnopqrstuvwxyz", b"23456789", b"!#%+-=?@^_"];
    let mut rng = rand::thread_rng();
    let all: Vec<u8> = SETS.concat();
    let mut pw: Vec<u8> = SETS.iter().map(|s| s[rng.gen_range(0..s.len())]).collect();
    while pw.len() < len.max(14) {
        pw.push(all[rng.gen_range(0..all.len())]);
    }
    for i in (1..pw.len()).rev() {
        pw.swap(i, rng.gen_range(0..=i));
    }
    String::from_utf8(pw).expect("ascii")
}

/// Returns a compliance item when a LAPS setting applies, and whether the
/// agent state changed.
pub async fn maintain(
    settings: &[Setting],
    rotate_requested: bool,
    state: &mut AgentState,
    client: &od_client::Client,
) -> (Option<ComplianceItem>, bool) {
    let Some(Setting::LocalAdminPassword { account, length, rotate_days }) =
        settings.iter().find(|s| matches!(s, Setting::LocalAdminPassword { .. }))
    else {
        return (None, false);
    };
    let account = account.trim().to_lowercase();
    if !safe_name(&account) {
        return (
            Some(item(ComplianceState::Failed, format!("\"{account}\" can't be used: pick a name that isn't a built-in account"))),
            false,
        );
    }
    if state.accounts.contains_key(&account) {
        return (
            Some(item(ComplianceState::Failed, format!("\"{account}\" is a directory sign-in account; choose another LAPS account name"))),
            false,
        );
    }
    let managed = state.laps.as_ref().filter(|l| l.account == account);
    let exists = match os::exists(&account).await {
        Ok(e) => e,
        Err(e) => return (Some(item(ComplianceState::Failed, format!("{e:#}"))), false),
    };
    if exists && managed.is_none() {
        return (
            Some(item(
                ComplianceState::Failed,
                format!("A local account named \"{account}\" already exists and isn't managed by OpenDirectory; choose another name"),
            )),
            false,
        );
    }
    let due = match managed {
        None => true,
        Some(l) => rotate_requested || !exists || Utc::now() - l.rotated_at >= chrono::Duration::days((*rotate_days).max(1) as i64),
    };
    if !due {
        let next = managed.map(|l| l.rotated_at + chrono::Duration::days((*rotate_days).max(1) as i64)).unwrap_or_else(Utc::now);
        return (Some(item(ComplianceState::Compliant, format!("{account} is managed; next rotation {}", next.format("%Y-%m-%d")))), false);
    }
    let pw = generate((*length).clamp(14, 64) as usize);
    // Escrow first: if the server doesn't have it, don't change anything locally.
    if let Err(e) = client.post::<serde_json::Value, _>("/agent/laps", &LapsEscrow { account: account.clone(), password: pw.clone() }).await
    {
        return (
            Some(item(
                ComplianceState::Failed,
                format!("Couldn't store the new password on the Management Server, so it wasn't changed: {e}"),
            )),
            false,
        );
    }
    let res = async {
        if exists {
            os::set_password(&account, &pw).await?;
        } else {
            os::create(&account, "Local Administrator (LAPS)", &pw).await?;
        }
        os::set_admin(&account, true).await?;
        os::set_enabled(&account, true).await?;
        anyhow::Ok(())
    }
    .await;
    match res {
        Ok(()) => {
            state.laps = Some(LapsState { account: account.clone(), rotated_at: Utc::now() });
            (Some(item(ComplianceState::Applied, format!("{account}'s password was rotated and stored on the Management Server"))), true)
        }
        Err(e) => (
            Some(item(
                ComplianceState::Failed,
                format!("The new password was stored but setting it locally failed ({e:#}); the previous password still works"),
            )),
            false,
        ),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn generated_passwords_are_complex() {
        for _ in 0..50 {
            let p = super::generate(20);
            assert_eq!(p.len(), 20);
            assert!(p.chars().any(|c| c.is_ascii_uppercase()) && p.chars().any(|c| c.is_ascii_lowercase()));
            assert!(p.chars().any(|c| c.is_ascii_digit()) && p.chars().any(|c| !c.is_alphanumeric()));
        }
    }
}
