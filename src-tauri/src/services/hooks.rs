use crate::services::shell_env::{shell_command, shell_environment};
use chrono::Utc;
use fennec_core::hooks::{AutomationHook, HookEvent};
use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

pub struct HookFire {
    pub hooks: Vec<AutomationHook>,
    pub timeout_seconds: u32,
    pub log_path: PathBuf,
}

pub fn fire(
    config: HookFire,
    event: HookEvent,
    session_folder: Option<&Path>,
    extra: HashMap<String, String>,
) {
    let hooks: Vec<AutomationHook> = config
        .hooks
        .into_iter()
        .filter(|h| h.event == event && h.enabled)
        .collect();
    if hooks.is_empty() {
        return;
    }

    let mut env: HashMap<String, String> = shell_environment().clone();
    env.insert("FENNEC_EVENT".to_string(), event.raw_value().to_string());
    if let Some(folder) = session_folder {
        let session_id = folder
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default()
            .to_string();
        env.insert("FENNEC_SESSION_ID".to_string(), session_id.clone());
        env.insert(
            "FENNEC_SESSION_DIR".to_string(),
            folder.to_string_lossy().into_owned(),
        );
        let transcript = folder.join(format!("transcript_{}.txt", session_id));
        if transcript.exists() {
            env.insert(
                "FENNEC_TRANSCRIPT_FILE".to_string(),
                transcript.to_string_lossy().into_owned(),
            );
        }
    }
    for (key, value) in extra {
        env.insert(key, value);
    }

    for hook in hooks {
        let env = env.clone();
        let log_path = config.log_path.clone();
        let timeout = config.timeout_seconds;
        std::thread::spawn(move || {
            run_hook(&hook, event, env, timeout, &log_path);
        });
    }
}

fn run_hook(
    hook: &AutomationHook,
    event: HookEvent,
    env: HashMap<String, String>,
    timeout_seconds: u32,
    log_path: &Path,
) {
    let mut cmd = shell_command(&hook.command);
    cmd.env_clear();
    cmd.envs(&env);
    cmd.stdout(std::process::Stdio::null());
    cmd.stderr(std::process::Stdio::piped());

    let started = Instant::now();
    let mut child = match cmd.spawn() {
        Ok(child) => child,
        Err(e) => {
            log_line(log_path, event, &hook.command, &format!("launch failed: {e}"));
            return;
        }
    };

    let deadline = started + Duration::from_secs(timeout_seconds as u64);
    let mut timed_out = false;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) => {
                if Instant::now() >= deadline {
                    timed_out = true;
                    let _ = child.kill();
                    break child.wait().ok();
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(_) => break None,
        }
    };

    let stderr_text = child
        .stderr
        .take()
        .and_then(|mut s| {
            use std::io::Read;
            let mut buf = String::new();
            s.read_to_string(&mut buf).ok().map(|_| buf)
        })
        .unwrap_or_default()
        .trim()
        .to_string();

    let elapsed = format!("{:.1}s", started.elapsed().as_secs_f64());
    let mut result = if timed_out {
        format!("timeout after {timeout_seconds}s")
    } else {
        match status {
            Some(s) => format!("exit {}", s.code().unwrap_or(-1)),
            None => "unknown status".to_string(),
        }
    };
    result.push_str(&format!(" ({elapsed})"));
    if !stderr_text.is_empty() {
        let truncated: String = stderr_text.chars().take(500).collect();
        result.push_str(&format!(" stderr: {truncated}"));
    }
    log_line(log_path, event, &hook.command, &result);
}

fn log_line(log_path: &Path, event: HookEvent, command: &str, result: &str) {
    if let Some(parent) = log_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let line = format!(
        "[{}] {}: {} → {}\n",
        Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        event.raw_value(),
        command,
        result
    );
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)
    {
        let _ = file.write_all(line.as_bytes());
    }
}
