use std::collections::HashMap;
use std::sync::OnceLock;

pub fn shell_environment() -> &'static HashMap<String, String> {
    static CACHE: OnceLock<HashMap<String, String>> = OnceLock::new();
    CACHE.get_or_init(build_environment)
}

#[cfg(unix)]
fn build_environment() -> HashMap<String, String> {
    let mut env: HashMap<String, String> = std::env::vars().collect();

    let shell = if cfg!(target_os = "macos") {
        "/bin/zsh".to_string()
    } else {
        env.get("SHELL").cloned().unwrap_or_else(|| "/bin/sh".to_string())
    };
    if let Ok(output) = std::process::Command::new(&shell)
        .args(["-l", "-c", "env"])
        .output()
    {
        if output.status.success() {
            for line in String::from_utf8_lossy(&output.stdout).lines() {
                if let Some((key, value)) = line.split_once('=') {
                    env.insert(key.to_string(), value.to_string());
                }
            }
        }
    }

    let home = env
        .get("HOME")
        .cloned()
        .unwrap_or_else(|| std::env::var("HOME").unwrap_or_default());
    let path_dirs = [
        format!("{home}/.local/bin"),
        "/opt/homebrew/bin".to_string(),
        "/usr/local/bin".to_string(),
        "/usr/bin".to_string(),
        "/bin".to_string(),
    ];
    let prefix = path_dirs.join(":");
    let path = match env.get("PATH") {
        Some(existing) => format!("{prefix}:{existing}"),
        None => prefix,
    };
    env.insert("PATH".to_string(), path);
    env.insert("HOME".to_string(), home);
    env
}

#[cfg(windows)]
fn build_environment() -> HashMap<String, String> {
    std::env::vars().collect()
}

pub fn shell_command(command: &str) -> std::process::Command {
    #[cfg(target_os = "macos")]
    {
        let mut cmd = std::process::Command::new("/bin/zsh");
        cmd.args(["-l", "-c", command]);
        cmd
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
        let mut cmd = std::process::Command::new(shell);
        cmd.args(["-c", command]);
        cmd
    }
    #[cfg(windows)]
    {
        let mut cmd = std::process::Command::new("cmd");
        cmd.args(["/C", command]);
        cmd
    }
}
