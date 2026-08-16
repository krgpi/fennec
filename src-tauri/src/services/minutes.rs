use crate::services::shell_env::{shell_command, shell_environment};
use anyhow::Context;
use chrono::Utc;
use fennec_core::minutes::{
    build_prompt, content_hash, default_minutes_file_name, extract_file_name, MinutesBackend,
    PromptContext,
};
use fennec_core::session::{load_metadata, save_metadata};
use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

fn cli_cache() -> &'static Mutex<HashMap<MinutesBackend, Option<PathBuf>>> {
    static CACHE: OnceLock<Mutex<HashMap<MinutesBackend, Option<PathBuf>>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn refresh_cli_cache() {
    cli_cache().lock().unwrap().clear();
}

pub fn find_cli(backend: MinutesBackend) -> Option<PathBuf> {
    if let Some(cached) = cli_cache().lock().unwrap().get(&backend) {
        return cached.clone();
    }
    let path = find_binary(backend.binary_name());
    cli_cache().lock().unwrap().insert(backend, path.clone());
    path
}

pub fn available_backends() -> Vec<MinutesBackend> {
    MinutesBackend::ALL
        .into_iter()
        .filter(|b| find_cli(*b).is_some())
        .collect()
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(windows)]
fn is_executable(path: &Path) -> bool {
    path.is_file()
}

fn find_binary(name: &str) -> Option<PathBuf> {
    let home = dirs_home();

    #[cfg(unix)]
    let search_dirs = vec![
        home.join(".local/bin"),
        PathBuf::from("/usr/local/bin"),
        PathBuf::from("/opt/homebrew/bin"),
        home.join(".claude/local"),
    ];
    #[cfg(windows)]
    let search_dirs = vec![home.join(".local").join("bin")];

    #[cfg(unix)]
    let candidates = vec![name.to_string()];
    #[cfg(windows)]
    let candidates = vec![format!("{name}.cmd"), format!("{name}.exe"), name.to_string()];

    for dir in &search_dirs {
        for candidate in &candidates {
            let path = dir.join(candidate);
            let resolved = std::fs::canonicalize(&path).unwrap_or(path);
            if is_executable(&resolved) {
                return Some(resolved);
            }
        }
    }

    let versions_dir = home.join(".local/share").join(name).join("versions");
    if let Ok(entries) = std::fs::read_dir(&versions_dir) {
        let mut names: Vec<PathBuf> = entries.filter_map(|e| e.ok()).map(|e| e.path()).collect();
        names.sort();
        for path in names.into_iter().rev() {
            if is_executable(&path) {
                return Some(path);
            }
        }
    }

    #[cfg(unix)]
    let which_cmd = format!("which {name}");
    #[cfg(windows)]
    let which_cmd = format!("where {name}");
    if let Ok(output) = shell_command(&which_cmd).output() {
        if output.status.success() {
            let path = String::from_utf8_lossy(&output.stdout);
            let path = path.lines().next().unwrap_or("").trim();
            if !path.is_empty() {
                return Some(PathBuf::from(path));
            }
        }
    }

    None
}

fn dirs_home() -> PathBuf {
    #[cfg(unix)]
    {
        PathBuf::from(std::env::var("HOME").unwrap_or_default())
    }
    #[cfg(windows)]
    {
        PathBuf::from(std::env::var("USERPROFILE").unwrap_or_default())
    }
}

pub struct MinutesRequest {
    pub session_id: String,
    pub session_folder: PathBuf,
    pub transcript: String,
    pub backend: MinutesBackend,
    pub model: String,
    pub context_folder: Option<PathBuf>,
    pub output_folder: Option<PathBuf>,
    pub custom_prompt: Option<String>,
}

pub struct MinutesOutcome {
    pub output_file: PathBuf,
    pub body: String,
}

pub fn generate(
    req: &MinutesRequest,
    child_slot: Arc<Mutex<Option<Child>>>,
    cancelled: Arc<AtomicBool>,
) -> anyhow::Result<MinutesOutcome> {
    let cli = find_cli(req.backend)
        .ok_or_else(|| anyhow::anyhow!("{} CLI が見つかりません", req.backend.display_name()))?;

    let claude_md = req.context_folder.as_deref().and_then(load_claude_md);
    let existing_file_names = req
        .output_folder
        .as_deref()
        .map(list_md_file_names)
        .unwrap_or_default();
    let previous_minutes = req.output_folder.as_deref().and_then(find_latest_md);

    let ctx = PromptContext {
        has_context_folder: req.context_folder.is_some(),
        claude_md,
        existing_file_names,
        previous_minutes,
        custom_prompt: req.custom_prompt.clone(),
    };
    let prompt = build_prompt(&req.transcript, &req.session_id, &ctx);

    let codex_output_file = req.backend.uses_stdin_prompt().then(|| {
        std::env::temp_dir().join(format!("codex_minutes_{}.md", uuid::Uuid::new_v4()))
    });

    let args = req
        .backend
        .build_arguments(&req.model, &prompt, codex_output_file.as_deref());

    let mut cmd = Command::new(&cli);
    cmd.args(&args);
    cmd.current_dir(
        req.context_folder
            .as_deref()
            .or(req.output_folder.as_deref())
            .unwrap_or(&req.session_folder),
    );
    cmd.env_clear();
    cmd.envs(shell_environment());
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());
    if req.backend.uses_stdin_prompt() {
        cmd.stdin(Stdio::piped());
    }

    let mut child = cmd
        .spawn()
        .with_context(|| format!("CLI の起動に失敗しました: {}", cli.display()))?;

    if req.backend.uses_stdin_prompt() {
        if let Some(mut stdin) = child.stdin.take() {
            use std::io::Write;
            let _ = stdin.write_all(prompt.as_bytes());
        }
    }

    let mut stdout_pipe = child.stdout.take();
    let mut stderr_pipe = child.stderr.take();
    *child_slot.lock().unwrap() = Some(child);

    let stderr_handle = std::thread::spawn(move || {
        let mut buf = String::new();
        if let Some(stderr) = stderr_pipe.as_mut() {
            let _ = stderr.read_to_string(&mut buf);
        }
        buf
    });
    let mut stdout_text = String::new();
    if let Some(stdout) = stdout_pipe.as_mut() {
        let _ = stdout.read_to_string(&mut stdout_text);
    }
    let stderr_text = stderr_handle.join().unwrap_or_default();

    let status = {
        let mut slot = child_slot.lock().unwrap();
        let mut child = slot.take().context("process handle lost")?;
        child.wait()?
    };

    let mut output = stdout_text;
    if let Some(tmp) = &codex_output_file {
        if let Ok(content) = std::fs::read_to_string(tmp) {
            if !content.is_empty() {
                output = content;
            }
        }
        let _ = std::fs::remove_file(tmp);
    }

    if !status.success() {
        if cancelled.load(Ordering::Relaxed) {
            anyhow::bail!("cancelled");
        }
        let code = status.code().unwrap_or(-1);
        let suffix = if stderr_text.trim().is_empty() {
            String::new()
        } else {
            format!(": {}", stderr_text.trim())
        };
        anyhow::bail!("生成に失敗しました (exit {code}){suffix}");
    }

    let fallback = default_minutes_file_name(&req.session_id);
    let (file_name, body) = extract_file_name(&output, &fallback);
    let output_file = match &req.output_folder {
        Some(folder) => folder.join(file_name),
        None => req.session_folder.join("minutes.md"),
    };
    std::fs::write(&output_file, &body)
        .with_context(|| format!("ファイルの保存に失敗しました: {}", output_file.display()))?;

    Ok(MinutesOutcome {
        output_file,
        body,
    })
}

/// 生成結果をセッションフォルダに取り込み、出力先フォルダへ書いた場合は書き出し済みとして記録する。
pub fn record_outcome(folder: &Path, outcome: &MinutesOutcome, preset_id: Option<uuid::Uuid>) {
    let session_minutes = folder.join("minutes.md");
    let exported = outcome.output_file != session_minutes;
    if exported {
        if let Err(e) = std::fs::copy(&outcome.output_file, &session_minutes) {
            log::error!("failed to copy minutes into session folder: {e}");
        }
    }
    let Some(mut metadata) = load_metadata(folder) else {
        return;
    };
    metadata.minutes_file = Some("minutes.md".to_string());
    metadata.minutes_preset_id = preset_id;
    if exported {
        metadata.minutes_exported_at = Some(Utc::now());
        metadata.minutes_exported_path = Some(outcome.output_file.clone());
        metadata.minutes_exported_hash = Some(content_hash(&outcome.body));
    }
    if let Err(e) = save_metadata(folder, &metadata) {
        log::error!("failed to update session.json: {e}");
    }
}

fn load_claude_md(folder: &Path) -> Option<String> {
    for name in ["CLAUDE.md", "claude.md"] {
        if let Ok(text) = std::fs::read_to_string(folder.join(name)) {
            if !text.is_empty() {
                return Some(text);
            }
        }
    }
    None
}

fn list_md_file_names(folder: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("md"))
        .filter_map(|p| p.file_name().and_then(|n| n.to_str()).map(String::from))
        .collect();
    names.sort();
    names
}

fn find_latest_md(folder: &Path) -> Option<String> {
    let entries = std::fs::read_dir(folder).ok()?;
    let mut files: Vec<(std::time::SystemTime, PathBuf)> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("md"))
        .filter_map(|p| {
            let modified = std::fs::metadata(&p).ok()?.modified().ok()?;
            Some((modified, p))
        })
        .collect();
    files.sort_by(|a, b| b.0.cmp(&a.0));
    let (_, latest) = files.into_iter().next()?;
    std::fs::read_to_string(latest).ok().filter(|t| !t.is_empty())
}
