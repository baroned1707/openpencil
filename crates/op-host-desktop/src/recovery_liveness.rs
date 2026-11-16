//! Whether a process id still names a running process.
//!
//! Startup recovery must not take the live snapshots of another running
//! instance (for example a debug build next to the installed app): an entry is
//! only offered when the process that wrote it is gone.
//!
//! * Unix: `kill(pid, 0)` sends no signal, it only checks. `EPERM` means the
//!   process exists but belongs to someone else, which counts as alive.
//! * Windows: asks `tasklist` for the pid. When the lookup itself cannot run
//!   the process is treated as not alive (the entry is then offered), because
//!   hiding a user's unsaved work is worse than offering it twice.
//!
//! A pid can be reused by an unrelated process after a crash; then the entry is
//! hidden until that process exits. That is the cost of the simple probe.

/// `true` when `pid` is a running process.
pub(crate) fn process_is_alive(pid: u32) -> bool {
    probe(pid)
}

#[cfg(unix)]
fn probe(pid: u32) -> bool {
    // `kill(0, …)` and `kill(-1, …)` address whole process groups / every
    // process: only a positive pid may be probed.
    let Ok(pid) = libc::pid_t::try_from(pid) else {
        return false;
    };
    if pid <= 0 {
        return false;
    }
    // SAFETY: signal 0 performs the existence and permission check only.
    let result = unsafe { libc::kill(pid, 0) };
    result == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

#[cfg(windows)]
fn probe(pid: u32) -> bool {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let output = std::process::Command::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}"), "/NH", "/FO", "CSV"])
        .creation_flags(CREATE_NO_WINDOW)
        .output();
    match output {
        Ok(output) => String::from_utf8_lossy(&output.stdout).contains(&format!("\"{pid}\"")),
        Err(_) => false,
    }
}

#[cfg(not(any(unix, windows)))]
fn probe(_pid: u32) -> bool {
    false
}
