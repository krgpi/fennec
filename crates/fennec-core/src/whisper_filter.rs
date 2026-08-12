use crate::types::TimedSegment;

#[derive(Debug, Clone, PartialEq)]
pub struct WhisperSegment {
    pub text: String,
    pub start: f64,
    pub end: f64,
    pub no_speech_prob: f32,
    pub avg_logprob: f32,
}

pub const HALLUCINATION_PATTERNS: [&str; 6] = [
    "ご視聴ありがとうございました",
    "ありがとうございました",
    "チャンネル登録よろしくお願いします",
    "チャンネル登録お願いします",
    "おやすみなさい",
    "お疲れ様でした",
];

pub const NO_SPEECH_PROB_THRESHOLD: f32 = 0.6;
pub const AVG_LOGPROB_THRESHOLD: f32 = -0.9;
pub const DUPLICATE_WINDOW_SECS: f64 = 5.0;

pub fn filter_segments(segments: Vec<WhisperSegment>) -> Vec<WhisperSegment> {
    let mut filtered: Vec<WhisperSegment> = Vec::new();
    for seg in segments {
        if seg.no_speech_prob > NO_SPEECH_PROB_THRESHOLD {
            continue;
        }
        if seg.avg_logprob < AVG_LOGPROB_THRESHOLD {
            continue;
        }
        let cleaned = seg.text.trim();
        if cleaned.is_empty() {
            continue;
        }
        if HALLUCINATION_PATTERNS.contains(&cleaned) {
            continue;
        }
        let is_duplicate = filtered.iter().any(|existing| {
            existing.text == seg.text && (existing.start - seg.start).abs() < DUPLICATE_WINDOW_SECS
        });
        if !is_duplicate {
            filtered.push(seg);
        }
    }
    filtered
}

pub fn joined_text(segments: &[WhisperSegment]) -> String {
    segments
        .iter()
        .map(|s| s.text.trim())
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_string()
}

pub fn to_timed_segments(segments: &[WhisperSegment]) -> Vec<TimedSegment> {
    segments
        .iter()
        .map(|s| TimedSegment::new(s.text.trim(), s.start, s.end))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(text: &str, start: f64) -> WhisperSegment {
        WhisperSegment {
            text: text.to_string(),
            start,
            end: start + 2.0,
            no_speech_prob: 0.1,
            avg_logprob: -0.3,
        }
    }

    #[test]
    fn drops_high_no_speech_prob() {
        let mut s = seg("テキスト", 0.0);
        s.no_speech_prob = 0.61;
        assert!(filter_segments(vec![s]).is_empty());
    }

    #[test]
    fn keeps_boundary_no_speech_prob() {
        let mut s = seg("テキスト", 0.0);
        s.no_speech_prob = 0.6;
        assert_eq!(filter_segments(vec![s]).len(), 1);
    }

    #[test]
    fn drops_low_avg_logprob() {
        let mut s = seg("テキスト", 0.0);
        s.avg_logprob = -0.91;
        assert!(filter_segments(vec![s]).is_empty());
    }

    #[test]
    fn drops_exact_hallucination_only() {
        assert!(filter_segments(vec![seg("ご視聴ありがとうございました", 0.0)]).is_empty());
        assert!(filter_segments(vec![seg("  ご視聴ありがとうございました  ", 0.0)]).is_empty());
        let partial = filter_segments(vec![seg("本日はご視聴ありがとうございました", 0.0)]);
        assert_eq!(partial.len(), 1);
    }

    #[test]
    fn drops_empty_after_trim() {
        assert!(filter_segments(vec![seg("   ", 0.0)]).is_empty());
    }

    #[test]
    fn dedup_within_5s_window() {
        let result = filter_segments(vec![seg("同じ", 0.0), seg("同じ", 4.9)]);
        assert_eq!(result.len(), 1);

        let result = filter_segments(vec![seg("同じ", 0.0), seg("同じ", 5.0)]);
        assert_eq!(result.len(), 2);

        let result = filter_segments(vec![seg("違うA", 0.0), seg("違うB", 1.0)]);
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn joined_text_space_separated_trimmed() {
        let segs = vec![seg(" こんにちは ", 0.0), seg(" world ", 6.0)];
        assert_eq!(joined_text(&segs), "こんにちは world");
        assert_eq!(joined_text(&[]), "");
    }
}
