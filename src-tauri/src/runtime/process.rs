//! Process-group helpers for app-owned runtime commands.
//!
//! Python, Node.js, npm, and pip are allowed to create descendants. Killing
//! only the direct `Child` would leave those descendants behind when a health
//! check, cancellation, restart, or app shutdown occurs.

use std::path::Path;
use std::process::{Child, Command};
use std::thread;
use std::time::Duration;

pub(crate) fn prepare_command(command: &mut Command) {
    #[cfg(unix)]
    {
        use std::io;
        use std::os::unix::process::CommandExt;

        // SAFETY: the hook only changes the child process's own process group
        // before exec; it does not touch memory shared with the parent.
        unsafe {
            command.pre_exec(|| {
                if libc::setpgid(0, 0) == -1 {
                    return Err(io::Error::last_os_error());
                }
                Ok(())
            });
        }
    }

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;

        // CREATE_NEW_PROCESS_GROUP. The termination path uses taskkill /T so
        // descendants are included even when the child created a new process.
        command.creation_flags(0x0000_0200);
    }
}

pub(crate) fn terminate(child: &mut Child) {
    #[cfg(unix)]
    {
        terminate_unix(child);
    }

    #[cfg(windows)]
    {
        terminate_windows(child);
    }

    #[cfg(not(any(unix, windows)))]
    {
        let _ = child.kill();
        let _ = child.wait();
    }
}

/// Best-effort cleanup for services left by an older app process that did not
/// persist a `Child` handle. The command line must contain this app-owned
/// runtime root, so unrelated Python/Node projects are never targeted.
pub(crate) fn terminate_orphaned_processes(runtime_root: &Path) {
    #[cfg(unix)]
    {
        let marker = runtime_root.to_string_lossy();
        let current_pid = std::process::id();
        let output = Command::new("ps").args(["-axo", "pid=,command="]).output();
        let Ok(output) = output else {
            return;
        };
        let pids = String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter_map(|line| {
                let trimmed = line.trim_start();
                let split_at = trimmed.find(char::is_whitespace)?;
                let pid = trimmed[..split_at].parse::<u32>().ok()?;
                let command = trimmed[split_at..].trim_start();
                (pid != current_pid && command.contains(marker.as_ref())).then_some(pid)
            })
            .collect::<Vec<_>>();
        for pid in &pids {
            unsafe {
                libc::kill(*pid as libc::pid_t, libc::SIGTERM);
            }
        }
        if !pids.is_empty() {
            thread::sleep(Duration::from_millis(75));
        }
        for pid in pids {
            let still_running = unsafe { libc::kill(pid as libc::pid_t, 0) == 0 };
            if still_running {
                unsafe {
                    libc::kill(pid as libc::pid_t, libc::SIGKILL);
                }
            }
        }
    }

    #[cfg(not(unix))]
    let _ = runtime_root;
}

#[cfg(unix)]
fn terminate_unix(child: &mut Child) {
    if child.try_wait().ok().flatten().is_some() {
        return;
    }
    let process_group = -(child.id() as libc::pid_t);
    // The graceful signal gives uvicorn/Node a chance to close sockets and
    // flush logs before the bounded hard-stop fallback.
    unsafe {
        libc::kill(process_group, libc::SIGTERM);
    }
    for _ in 0..20 {
        if child.try_wait().ok().flatten().is_some() {
            return;
        }
        thread::sleep(Duration::from_millis(25));
    }
    unsafe {
        libc::kill(process_group, libc::SIGKILL);
    }
    let _ = child.wait();
}

#[cfg(windows)]
fn terminate_windows(child: &mut Child) {
    if child.try_wait().ok().flatten().is_some() {
        return;
    }
    let taskkill = std::env::var_os("SystemRoot")
        .map(|root| {
            std::path::PathBuf::from(root)
                .join("System32")
                .join("taskkill.exe")
        })
        .unwrap_or_else(|| "taskkill.exe".into());
    let pid = child.id().to_string();
    let _ = Command::new(taskkill)
        .args(["/T", "/F", "/PID", pid.as_str()])
        .status();
    let _ = child.wait();
}
