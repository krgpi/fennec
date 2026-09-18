use crate::agents::AgentDefinition;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MinutesPreset {
    pub id: Uuid,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_folder: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_folder: Option<PathBuf>,
    /// `AgentDefinition::id`（例: "claude" / "codex" / "gemini" / agents.json で追加した id）
    pub backend: String,
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_prompt: Option<String>,
    #[serde(with = "crate::session::iso8601")]
    pub created_at: DateTime<Utc>,
    #[serde(with = "crate::session::iso8601")]
    pub last_used_at: DateTime<Utc>,
}

impl MinutesPreset {
    pub fn new(name: impl Into<String>, agent: &AgentDefinition, now: DateTime<Utc>) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            context_folder: None,
            output_folder: None,
            backend: agent.id.clone(),
            model: agent.default_model.clone(),
            custom_prompt: None,
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
    pub custom_prompt: Option<String>,
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

    if let Some(custom) = ctx.custom_prompt.as_ref().filter(|s| !s.trim().is_empty()) {
        prompt.push_str(&format!(
            "\n\n# 追加の指示\n以下の指示を優先して議事録の内容やフォーマットを調整してください。\n\n{}",
            custom.trim()
        ));
    }

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
            custom_prompt: None,
        };
        let prompt = build_prompt("T", "D", &ctx);
        assert!(prompt.contains("- 参加者、議題、決定事項、アクションアイテムを整理\n- コンテキストフォルダ内の関連ファイルも参考にしてよい\n"));
        assert!(prompt.contains("出力先フォルダの既存ファイル: a.md, b.md\n既存ファイルの命名規則に合わせてファイル名を決めてください。"));
        assert!(prompt.contains("# コンテキストフォルダの CLAUDE.md\n以下の指示に従って議事録のフォーマットや内容を調整してください。\n\n指示内容"));
        assert!(prompt.ends_with("# 前回の議事録\n以下は前回の議事録です。構成・フォーマット・見出し構造を踏襲し、内容の連続性（前回のアクションアイテムの進捗、継続議題など）を意識してください。\n\n前回内容"));
    }

    #[test]
    fn prompt_with_custom_prompt() {
        let ctx = PromptContext {
            custom_prompt: Some("  箇条書きで簡潔に  ".into()),
            ..PromptContext::default()
        };
        let prompt = build_prompt("T", "D", &ctx);
        assert!(prompt.ends_with(
            "# 追加の指示\n以下の指示を優先して議事録の内容やフォーマットを調整してください。\n\n箇条書きで簡潔に"
        ));
    }

    #[test]
    fn prompt_ignores_blank_custom_prompt() {
        let ctx = PromptContext {
            custom_prompt: Some("   ".into()),
            ..PromptContext::default()
        };
        assert_eq!(
            build_prompt("T", "D", &ctx),
            build_prompt("T", "D", &PromptContext::default())
        );
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
}
