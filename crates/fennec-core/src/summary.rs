const MAX_LENGTH: usize = 30;

pub fn sanitize_title(raw: &str) -> String {
    let mut text = raw.trim().to_string();

    if text.starts_with("```") {
        let lines: Vec<&str> = text.split('\n').collect();
        let start = lines
            .iter()
            .position(|l| !l.starts_with("```"))
            .unwrap_or(lines.len());
        let end = lines
            .iter()
            .rposition(|l| !l.starts_with("```"))
            .map(|i| i + 1)
            .unwrap_or(start);
        text = lines[start..end].join("\n").trim().to_string();
    }

    if text.contains('{') {
        if let Ok(serde_json::Value::Object(obj)) = serde_json::from_str(&text) {
            if let Some(title) = obj.get("title").and_then(|v| v.as_str()) {
                text = title.trim().to_string();
            }
        }
    }

    text = text.trim_matches(['"', '\'']).to_string();

    for prefix in ["タイトル：", "タイトル:", "Title:", "title:"] {
        if let Some(rest) = text.strip_prefix(prefix) {
            text = rest.trim().to_string();
            break;
        }
    }

    if text.chars().count() > MAX_LENGTH {
        text = text.chars().take(MAX_LENGTH).collect();
    }

    text
}

pub fn title_prompt(transcript: &str) -> String {
    let trimmed: String = transcript.chars().take(2000).collect();
    format!(
        "以下の文字起こしテキストの内容を、20文字以内の短いタイトルにまとめてください。\nタイトルのみを出力し、それ以外は何も出力しないでください。\n\n{trimmed}"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_title_passthrough() {
        assert_eq!(sanitize_title(" 定例会議まとめ \n"), "定例会議まとめ");
    }

    #[test]
    fn strips_code_fences() {
        assert_eq!(sanitize_title("```\nタイトル\n```"), "タイトル");
        assert_eq!(sanitize_title("```json\n{\"title\": \"会議\"}\n```"), "会議");
    }

    #[test]
    fn extracts_json_title() {
        assert_eq!(sanitize_title("{\"title\": \"Q3計画\"}"), "Q3計画");
    }

    #[test]
    fn strips_surrounding_quotes() {
        assert_eq!(sanitize_title("\"タイトル\""), "タイトル");
        assert_eq!(sanitize_title("'タイトル'"), "タイトル");
    }

    #[test]
    fn truncates_to_30_chars() {
        let long = "あ".repeat(50);
        assert_eq!(sanitize_title(&long).chars().count(), 30);
    }

    #[test]
    fn invalid_json_kept_as_is() {
        assert_eq!(sanitize_title("{壊れたjson"), "{壊れたjson");
    }

    #[test]
    fn strips_title_prefix() {
        assert_eq!(sanitize_title("タイトル: 定例会議"), "定例会議");
        assert_eq!(sanitize_title("タイトル：定例会議"), "定例会議");
        assert_eq!(sanitize_title("Title: Weekly Sync"), "Weekly Sync");
    }
}
