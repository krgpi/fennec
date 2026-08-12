use crate::types::{TranscriptEntry, TranscriptSource};
use regex::Regex;
use std::sync::LazyLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LabelLanguage {
    Ja,
    En,
}

impl LabelLanguage {
    pub fn system_label(self) -> &'static str {
        match self {
            Self::Ja => "PC音声",
            Self::En => "System Audio",
        }
    }

    pub fn mic_label(self) -> &'static str {
        match self {
            Self::Ja => "マイク",
            Self::En => "Mic",
        }
    }

    pub fn speaker_label(self, speaker_id: i32) -> String {
        match self {
            Self::Ja => format!("話者{}", speaker_id + 1),
            Self::En => format!("Speaker {}", speaker_id + 1),
        }
    }
}

pub fn format_time(t: f64) -> String {
    let total = t as i64;
    let h = total / 3600;
    let m = (total % 3600) / 60;
    let s = total % 60;
    if h > 0 {
        format!("{}:{:02}:{:02}", h, m, s)
    } else {
        format!("{:02}:{:02}", m, s)
    }
}

pub fn build_transcript_text(entries: &[TranscriptEntry], lang: LabelLanguage) -> String {
    entries
        .iter()
        .map(|entry| {
            let time = format_time(entry.start_time);
            let label = match entry.speaker_id {
                Some(id) => lang.speaker_label(id),
                None => match entry.source {
                    TranscriptSource::System => lang.system_label().to_string(),
                    TranscriptSource::Mic => lang.mic_label().to_string(),
                },
            };
            format!("[{}] {}: {}", time, label, entry.text)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

static LINE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\[(\d+):(\d{2})(?::(\d{2}))?\]\s+(.+?):\s+(.+)$").unwrap()
});

static SPEAKER_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(?:話者|Speaker\s*)(\d+)$").unwrap());

pub fn parse_transcript_text(text: &str) -> Vec<TranscriptEntry> {
    text.lines().filter_map(parse_transcript_line).collect()
}

fn parse_transcript_line(line: &str) -> Option<TranscriptEntry> {
    let caps = LINE_RE.captures(line)?;
    let first: f64 = caps[1].parse().ok()?;
    let second: f64 = caps[2].parse().ok()?;
    let start_time = match caps.get(3) {
        Some(third) => first * 3600.0 + second * 60.0 + third.as_str().parse::<f64>().ok()?,
        None => first * 60.0 + second,
    };
    let label = &caps[4];
    let text = caps[5].to_string();

    let (source, speaker_id) = if let Some(sp) = SPEAKER_RE.captures(label) {
        let n: i32 = sp[1].parse().ok()?;
        (TranscriptSource::System, Some(n - 1))
    } else if label == "マイク" || label == "Mic" {
        (TranscriptSource::Mic, None)
    } else {
        (TranscriptSource::System, None)
    };

    let mut entry = TranscriptEntry::new(source, text, start_time);
    entry.speaker_id = speaker_id;
    Some(entry)
}

static PREVIEW_PREFIX_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\[\d+:\d{2}(?::\d{2})?\]\s+.+?:\s+").unwrap());

pub fn transcript_preview(head: &str) -> Option<String> {
    for line in head.split('\n').filter(|l| !l.is_empty()) {
        if let Some(m) = PREVIEW_PREFIX_RE.find(line) {
            let content = &line[m.end()..];
            if !content.is_empty() {
                return Some(content.chars().take(80).collect());
            }
        } else {
            return Some(line.chars().take(80).collect());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_time_under_an_hour() {
        assert_eq!(format_time(0.0), "00:00");
        assert_eq!(format_time(75.4), "01:15");
        assert_eq!(format_time(3599.0), "59:59");
    }

    #[test]
    fn format_time_hour_rollover() {
        assert_eq!(format_time(3600.0), "1:00:00");
        assert_eq!(format_time(4530.0), "1:15:30");
        assert_eq!(format_time(7325.0), "2:02:05");
    }

    #[test]
    fn build_text_matches_legacy_format() {
        let entries = vec![
            TranscriptEntry::new(TranscriptSource::System, "それでは始めます。", 0.0),
            TranscriptEntry::new(TranscriptSource::Mic, "はい。", 12.0),
        ];
        let text = build_transcript_text(&entries, LabelLanguage::Ja);
        assert_eq!(text, "[00:00] PC音声: それでは始めます。\n[00:12] マイク: はい。");
    }

    #[test]
    fn build_text_uses_speaker_label() {
        let mut entry = TranscriptEntry::new(TranscriptSource::System, "こんにちは", 5.0);
        entry.speaker_id = Some(0);
        let text = build_transcript_text(&[entry], LabelLanguage::Ja);
        assert_eq!(text, "[00:05] 話者1: こんにちは");
    }

    #[test]
    fn parse_roundtrip_ja() {
        let text = "[00:00] PC音声: それでは始めます。\n[00:12] マイク: はい。\n[01:05] 話者2: 続けます";
        let entries = parse_transcript_text(text);
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].source, TranscriptSource::System);
        assert_eq!(entries[1].source, TranscriptSource::Mic);
        assert_eq!(entries[1].start_time, 12.0);
        assert_eq!(entries[2].speaker_id, Some(1));
    }

    #[test]
    fn parse_english_labels() {
        let text = "[00:00] System Audio: hello\n[00:05] Mic: hi\n[00:10] Speaker 3: yes";
        let entries = parse_transcript_text(text);
        assert_eq!(entries[0].source, TranscriptSource::System);
        assert_eq!(entries[1].source, TranscriptSource::Mic);
        assert_eq!(entries[2].speaker_id, Some(2));
    }

    #[test]
    fn parse_hour_format_and_legacy_over_60_minutes() {
        let entries = parse_transcript_text("[1:15:30] マイク: あとで\n[75:30] マイク: レガシー");
        assert_eq!(entries[0].start_time, 4530.0);
        assert_eq!(entries[1].start_time, 4530.0);
    }

    #[test]
    fn preview_strips_prefix() {
        let head = "[00:00] PC音声: それでは定例を始めましょう。\n[00:12] マイク: はい。";
        assert_eq!(
            transcript_preview(head),
            Some("それでは定例を始めましょう。".to_string())
        );
    }

    #[test]
    fn preview_plain_text_passthrough() {
        assert_eq!(transcript_preview("ただのテキスト"), Some("ただのテキスト".to_string()));
        assert_eq!(transcript_preview(""), None);
    }
}
