//! Processes: a Task Manager view (CPU, memory, user, command line) and
//! ending a process or a process tree.

use anyhow::{bail, Result};
use od_core::manage::{ProcessInfo, ProcessReport};
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, RefreshKind, System, UpdateKind, Users};

fn refresh_kind() -> ProcessRefreshKind {
    ProcessRefreshKind::nothing().with_cpu().with_memory().with_user(UpdateKind::OnlyIfNotSet).with_cmd(UpdateKind::OnlyIfNotSet)
}

pub async fn list() -> Result<ProcessReport> {
    Ok(tokio::task::spawn_blocking(|| {
        let mut sys = System::new_with_specifics(RefreshKind::nothing());
        sys.refresh_processes_specifics(ProcessesToUpdate::All, true, refresh_kind());
        sys.refresh_cpu_usage();
        // CPU usage is measured between two samples.
        std::thread::sleep(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL.max(std::time::Duration::from_millis(500)));
        sys.refresh_processes_specifics(ProcessesToUpdate::All, true, refresh_kind());
        sys.refresh_cpu_usage();
        sys.refresh_memory();
        let users = Users::new_with_refreshed_list();
        let cpus = sys.cpus().len().max(1) as f32;
        let processes = sys
            .processes()
            .values()
            .filter(|p| p.thread_kind().is_none()) // Linux threads show up as tasks
            .map(|p| ProcessInfo {
                pid: p.pid().as_u32(),
                parent: p.parent().map(|x| x.as_u32()),
                name: p.name().to_string_lossy().into_owned(),
                user: p.user_id().and_then(|u| users.get_user_by_id(u)).map(|u| u.name().to_string()).unwrap_or_default(),
                // Share of the whole machine, like Task Manager.
                cpu_percent: p.cpu_usage() / cpus,
                memory_bytes: p.memory(),
                run_secs: p.run_time(),
                command: p.cmd().iter().map(|a| a.to_string_lossy()).collect::<Vec<_>>().join(" ").chars().take(1000).collect(),
            })
            .collect();
        ProcessReport {
            processes,
            cpu_count: sys.cpus().len() as u32,
            cpu_percent: sys.global_cpu_usage(),
            memory_total: sys.total_memory(),
            memory_used: sys.used_memory(),
        }
    })
    .await?)
}

pub async fn kill(pid: u32, tree: bool, expect_name: &str) -> Result<String> {
    let me = std::process::id();
    let expect = expect_name.to_string();
    tokio::task::spawn_blocking(move || {
        let mut sys = System::new();
        sys.refresh_processes_specifics(ProcessesToUpdate::All, true, ProcessRefreshKind::nothing());
        let Some(p) = sys.process(Pid::from_u32(pid)) else { bail!("No process with PID {pid} is running (it may have exited already)") };
        let name = p.name().to_string_lossy().into_owned();
        if !expect.is_empty() && !name.eq_ignore_ascii_case(&expect) {
            bail!("PID {pid} is now {name}, not {expect}; refresh the list and try again");
        }
        // Children first, so nothing gets re-parented and survives.
        let mut targets = vec![];
        if tree {
            let mut stack = vec![Pid::from_u32(pid)];
            while let Some(parent) = stack.pop() {
                for (cpid, c) in sys.processes() {
                    if c.parent() == Some(parent) && c.thread_kind().is_none() && !targets.contains(cpid) {
                        targets.push(*cpid);
                        stack.push(*cpid);
                    }
                }
            }
            targets.reverse();
        }
        targets.push(Pid::from_u32(pid));
        if targets.iter().any(|t| t.as_u32() == me) {
            bail!("That would end the Endpoint Agent itself; restart the agent from the Overview tab instead");
        }
        let mut ended = 0;
        let mut failed = vec![];
        for t in &targets {
            if let Some(p) = sys.process(*t) {
                // Ask nicely first where the OS supports it, then force.
                let ok = p.kill_with(sysinfo::Signal::Term).unwrap_or(false) || p.kill();
                if ok {
                    ended += 1;
                } else {
                    failed.push(t.as_u32().to_string());
                }
            }
        }
        // Force anything that ignored the polite request.
        std::thread::sleep(std::time::Duration::from_secs(3));
        sys.refresh_processes_specifics(ProcessesToUpdate::All, true, ProcessRefreshKind::nothing());
        for t in &targets {
            if let Some(p) = sys.process(*t) {
                p.kill();
            }
        }
        if !failed.is_empty() {
            bail!("Couldn't end PID {} (access denied or protected process)", failed.join(", "));
        }
        Ok(if tree { format!("Ended {name} (PID {pid}) and {} child process(es)", ended - 1) } else { format!("Ended {name} (PID {pid})") })
    })
    .await?
}
