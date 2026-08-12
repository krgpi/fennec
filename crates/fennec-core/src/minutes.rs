use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MinutesBackend {
    Claude,
    Codex,
    Gemini,
}

impl MinutesBackend {
    pub const ALL: [MinutesBackend; 3] = [Self::Claude, Self::Codex, Self::Gemini];

    pub fn display_name(self) -> &'static str {
        match self {
            Self::Claude => "Claude Code",
            Self::Codex => "Codex",
            Self::Gemini => "Gemini",
        }
    }

    pub fn binary_name(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
            Self::Gemini => "gemini",
        }
    }

    pub fn default_model(self) -> &'static str {
        match self {
            Self::Claude => "sonnet",
            Self::Codex => "",
            Self::Gemini => "gemini-2.5-flash",
        }
    }

    pub fn available_models(self) -> Vec<(&'static str, &'static str)> {
        match self {
            Self::Claude => vec![("sonnet", "Sonnet"), ("opus", "Opus"), ("haiku", "Haiku")],
            Self::Codex => vec![("", "デフォルト")],
            Self::Gemini => vec![("gemini-2.5-pro", "2.5 Pro"), ("gemini-2.5-flash", "2.5 Flash")],
        }
    }

    pub fn uses_stdin_prompt(self) -> bool {
        matches!(self, Self::Codex)
    }

    pub fn build_arguments(
        self,
        model: &str,
        prompt: &str,
        output_file: Option<&Path>,
    ) -> Vec<String> {
        match self {
            Self::Claude => vec![
                "-p".into(),
                "--model".into(),
                model.into(),
                "--output-format".into(),
                "text".into(),
                prompt.into(),
            ],
            Self::Codex => {
                let mut args: Vec<String> = vec![
                    "exec".into(),
                    "--skip-git-repo-check".into(),
                    "--ephemeral".into(),
                ];
                if !model.is_empty() {
                    args.push("--model".into());
                    args.push(model.into());
                }
                if let Some(output_file) = output_file {
                    args.push("-o".into());
                    args.push(output_file.to_string_lossy().into_owned());
                }
                args.push("-".into());
                args
            }
            Self::Gemini => vec!["-p".into(), prompt.into(), "-m".into(), model.into()],
        }
    }

    pub fn from_str_arg(s: &str) -> Option<Self> {
        match s {
            "claude" => Some(Self::Claude),
            "codex" => Some(Self::Codex),
            "gemini" => Some(Self::Gemini),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MinutesPreset {
    pub id: Uuid,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_folder: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_folder: Option<PathBuf>,
    pub backend: MinutesBackend,
    pub model: String,
    #[serde(with = "crate::session::iso8601")]
    pub created_at: DateTime<Utc>,
    #[serde(with = "crate::session::iso8601")]
    pub last_used_at: DateTime<Utc>,
}

impl MinutesPreset {
    pub fn new(name: impl Into<String>, backend: MinutesBackend, now: DateTime<Utc>) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            context_folder: None,
            output_folder: None,
            backend,
            model: backend.default_model().to_string(),
            created_at: now,
            last_used_at: now,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct PromptContext {
    pub has_context_folder: bool,
    pub claude_md: Option<String>,
    pub existing_file_names: Vec<String>,
    pub previous_minutes: Option<String>,
}

pub fn build_prompt(transcript: &str, session_date: &str, ctx: &PromptContext) -> String {
    let context_instruction = if ctx.has_context_folder {
        "\n- コンテキストフォルダ内の関連ファイルも参考にしてよい"
    } else {
        ""
    };

    let file_name_instruction = if ctx.existing_file_names.is_empty() {
        "ファイル名は自由に決めてください。".to_string()
    } else {
        format!(
            "出力先フォルダの既存ファイル: {}\n既存ファイルの命名規則に合わせてファイル名を決めてください。",
            ctx.existing_file_names.join(", ")
        )
    };

    let mut prompt = format!(
        "以下は会議の文字起こしです。これを元に議事録を作成してください。\n\n# 文字起こし\n{transcript}\n\n# 指示\n- **出力の1行目にファイル名のみを記述**（拡張子 .md 付き、それ以外のテキストは不要）\n- 2行目以降に Markdown 形式の議事録を出力\n- 参加者、議題、決定事項、アクションアイテムを整理{context_instruction}\n- 議事録のみを出力し、余計な説明は不要\n\n# ファイル名\n{file_name_instruction}\n日付情報: {session_date}"
    );

    if let Some(claude_md) = &ctx.claude_md {
        prompt.push_str(&format!(
            "\n\n# コンテキストフォルダの CLAUDE.md\n以下の指示に従って議事録のフォーマットや内容を調整してください。\n\n{claude_md}"
        ));
    }

    if let Some(previous) = &ctx.previous_minutes {
        prompt.push_str(&format!(
            "\n\n# 前回の議事録\n以下は前回の議事録です。構成・フォーマット・見出し構造を踏襲し、内容の連続性（前回のアクションアイテムの進捗、継続議題など）を意識してください。\n\n{previous}"
        ));
    }

    prompt
}

fn is_newline(c: char) -> bool {
    matches!(
        c,
        '\n' | '\r' | '\u{000B}' | '\u{000C}' | '\u{0085}' | '\u{2028}' | '\u{2029}'
    )
}

pub fn extract_file_name(output: &str, fallback: &str) -> (String, String) {
    let trimmed = output.trim();
    let Some(newline_idx) = trimmed.find(is_newline) else {
        return (fallback.to_string(), trimmed.to_string());
    };

    let first_line = trimmed[..newline_idx].trim_matches([' ', '\t']);
    let mut rest_start = newline_idx + trimmed[newline_idx..].chars().next().unwrap().len_utf8();
    if trimmed[newline_idx..].starts_with("\r\n") {
        rest_start = newline_idx + 2;
    }
    let rest = trimmed[rest_start..].trim();

    if first_line.ends_with(".md")
        && (!first_line.contains(' ') || first_line.chars().count() <= 100)
    {
        let sanitized = first_line.replace('/', "_");
        (sanitized, rest.to_string())
    } else {
        (fallback.to_string(), trimmed.to_string())
    }
}

pub fn default_minutes_file_name(session_date: &str) -> String {
    format!("minutes_{}.md", session_date)
}

pub fn content_hash(content: &str) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(content.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_minimal_matches_swift_output() {
        let prompt = build_prompt("こんにちは", "20260809_103000", &PromptContext::default());
        let expected = "以下は会議の文字起こしです。これを元に議事録を作成してください。\n\n# 文字起こし\nこんにちは\n\n# 指示\n- **出力の1行目にファイル名のみを記述**（拡張子 .md 付き、それ以外のテキストは不要）\n- 2行目以降に Markdown 形式の議事録を出力\n- 参加者、議題、決定事項、アクションアイテムを整理\n- 議事録のみを出力し、余計な説明は不要\n\n# ファイル名\nファイル名は自由に決めてください。\n日付情報: 20260809_103000";
        assert_eq!(prompt, expected);
    }

    #[test]
    fn prompt_with_all_context_sections() {
        let ctx = PromptContext {
            has_context_folder: true,
            claude_md: Some("指示内容".into()),
            existing_file_names: vec!["a.md".into(), "b.md".into()],
            previous_minutes: Some("前回内容".into()),
        };
        let prompt = build_prompt("T", "D", &ctx);
        assert!(prompt.contains("- 参加者、議題、決定事項、アクションアイテムを整理\n- コンテキストフォルダ内の関連ファイルも参考にしてよい\n"));
        assert!(prompt.contains("出力先フォルダの既存ファイル: a.md, b.md\n既存ファイルの命名規則に合わせてファイル名を決めてください。"));
        assert!(prompt.contains("# コンテキストフォルダの CLAUDE.md\n以下の指示に従って議事録のフォーマットや内容を調整してください。\n\n指示内容"));
        assert!(prompt.ends_with("# 前回の議事録\n以下は前回の議事録です。構成・フォーマット・見出し構造を踏襲し、内容の連続性（前回のアクションアイテムの進捗、継続議題など）を意識してください。\n\n前回内容"));
    }

    #[test]
    fn extract_valid_file_name() {
        let (name, body) = extract_file_name("meeting_0809.md\n# 議事録\n内容", "fallback.md");
        assert_eq!(name, "meeting_0809.md");
        assert_eq!(body, "# 議事録\n内容");
    }

    #[test]
    fn extract_sanitizes_slashes() {
        let (name, _) = extract_file_name("2026/08/09.md\nbody", "fallback.md");
        assert_eq!(name, "2026_08_09.md");
    }

    #[test]
    fn extract_falls_back_without_newline() {
        let (name, body) = extract_file_name("本文のみ", "fallback.md");
        assert_eq!(name, "fallback.md");
        assert_eq!(body, "本文のみ");
    }

    #[test]
    fn extract_falls_back_when_first_line_not_md() {
        let (name, body) = extract_file_name("# 議事録\n内容", "fallback.md");
        assert_eq!(name, "fallback.md");
        assert_eq!(body, "# 議事録\n内容");
    }

    #[test]
    fn extract_allows_spaces_when_short() {
        let (name, _) = extract_file_name("my notes.md\nbody", "fallback.md");
        assert_eq!(name, "my notes.md");
    }

    #[test]
    fn codex_args_with_model_and_output() {
        let args = MinutesBackend::Codex.build_arguments(
            "gpt-5",
            "prompt",
            Some(Path::new("/tmp/out.md")),
        );
        assert_eq!(
            args,
            vec![
                "exec",
                "--skip-git-repo-check",
                "--ephemeral",
                "--model",
                "gpt-5",
                "-o",
                "/tmp/out.md",
                "-"
            ]
        );
    }

    #[test]
    fn codex_args_default_model_omitted() {
        let args = MinutesBackend::Codex.build_arguments("", "prompt", None);
        assert_eq!(args, vec!["exec", "--skip-git-repo-check", "--ephemeral", "-"]);
    }

    #[test]
    fn claude_and_gemini_args() {
        assert_eq!(
            MinutesBackend::Claude.build_arguments("sonnet", "P", None),
            vec!["-p", "--model", "sonnet", "--output-format", "text", "P"]
        );
        assert_eq!(
            MinutesBackend::Gemini.build_arguments("gemini-2.5-flash", "P", None),
            vec!["-p", "P", "-m", "gemini-2.5-flash"]
        );
    }

    #[test]
    fn backend_serde_lowercase() {
        assert_eq!(
            serde_json::to_string(&MinutesBackend::Claude).unwrap(),
            "\"claude\""
        );
        let b: MinutesBackend = serde_json::from_str("\"gemini\"").unwrap();
        assert_eq!(b, MinutesBackend::Gemini);
    }
}
