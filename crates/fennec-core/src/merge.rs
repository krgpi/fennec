use crate::types::{SpeakerTimedSegment, TimedSegment, TranscriptEntry, TranscriptSource};

pub fn is_cjk(c: char) -> bool {
    let v = c as u32;
    (0x3000..=0x9FFF).contains(&v)
        || (0xAC00..=0xD7AF).contains(&v)
        || (0xF900..=0xFAFF).contains(&v)
        || (0x20000..=0x2FA1F).contains(&v)
        || (0xFF01..=0xFF60).contains(&v)
        || (0xFFE0..=0xFFEF).contains(&v)
}

pub fn join_segment_texts<S: AsRef<str>>(texts: &[S]) -> String {
    let Some(first) = texts.first() else {
        return String::new();
    };
    let mut result = first.as_ref().to_string();
    for next in &texts[1..] {
        let next = next.as_ref();
        if next.is_empty() {
            continue;
        }
        if next.starts_with(' ') || result.is_empty() {
            result.push_str(next);
        } else {
            let last_non_cjk = result.chars().last().is_some_and(|c| !is_cjk(c));
            let first_non_cjk = next.chars().next().is_some_and(|c| !is_cjk(c));
            if last_non_cjk && first_non_cjk {
                result.push(' ');
            }
            result.push_str(next);
        }
    }
    result
}

pub fn deduplicate_mic_echo(system: &[TimedSegment], mic: &[TimedSegment]) -> Vec<TimedSegment> {
    if system.is_empty() {
        return mic.to_vec();
    }
    mic.iter()
        .filter(|m| {
            let duration = m.end - m.start;
            if duration <= 0.0 {
                return true;
            }
            let total_overlap: f64 = system
                .iter()
                .map(|s| (m.end.min(s.end) - m.start.max(s.start)).max(0.0))
                .sum();
            total_overlap / duration < 0.5
        })
        .cloned()
        .collect()
}

pub fn assign_speakers(
    segments: &[TimedSegment],
    diarization: &[SpeakerTimedSegment],
) -> Vec<TimedSegment> {
    segments
        .iter()
        .map(|seg| {
            let mut best: Option<&SpeakerTimedSegment> = None;
            let mut best_overlap = 0.0f64;
            for d in diarization {
                let overlap = (seg.end.min(d.end) - seg.start.max(d.start)).max(0.0);
                if overlap > best_overlap {
                    best_overlap = overlap;
                    best = Some(d);
                }
            }
            let mut result = seg.clone();
            result.speaker_id = best.map(|d| d.speaker_id);
            result
        })
        .collect()
}

pub fn merge_segments(system: &[TimedSegment], mic: &[TimedSegment]) -> Vec<TranscriptEntry> {
    struct Tagged<'a> {
        source: TranscriptSource,
        text: &'a str,
        start: f64,
        end: f64,
    }

    const PAUSE_THRESHOLD: f64 = 5.0;
    const MAX_ENTRY_DURATION: f64 = 30.0;

    let mut all: Vec<Tagged> = Vec::with_capacity(system.len() + mic.len());
    for s in system {
        all.push(Tagged {
            source: TranscriptSource::System,
            text: &s.text,
            start: s.start,
            end: s.end,
        });
    }
    for s in mic {
        all.push(Tagged {
            source: TranscriptSource::Mic,
            text: &s.text,
            start: s.start,
            end: s.end,
        });
    }
    all.sort_by(|a, b| a.start.total_cmp(&b.start));

    if all.is_empty() {
        return Vec::new();
    }

    let mut entries = Vec::new();
    let mut current_source = all[0].source;
    let mut current_texts = vec![all[0].text];
    let mut current_start = all[0].start;
    let mut current_end = all[0].end;

    for seg in &all[1..] {
        let pause_gap = seg.start - current_end;
        if seg.source == current_source
            && pause_gap < PAUSE_THRESHOLD
            && seg.end - current_start < MAX_ENTRY_DURATION
        {
            current_texts.push(seg.text);
            current_end = seg.end;
        } else {
            entries.push(TranscriptEntry::new(
                current_source,
                join_segment_texts(&current_texts),
                current_start,
            ));
            current_source = seg.source;
            current_texts = vec![seg.text];
            current_start = seg.start;
            current_end = seg.end;
        }
    }
    entries.push(TranscriptEntry::new(
        current_source,
        join_segment_texts(&current_texts),
        current_start,
    ));

    entries
}

pub fn merge_segments_with_speakers(segments: &[TimedSegment]) -> Vec<TranscriptEntry> {
    const PAUSE_THRESHOLD: f64 = 3.0;
    const MAX_ENTRY_DURATION: f64 = 30.0;

    if segments.is_empty() {
        return Vec::new();
    }

    let mut sorted: Vec<&TimedSegment> = segments.iter().collect();
    sorted.sort_by(|a, b| a.start.total_cmp(&b.start));

    let mut entries = Vec::new();
    let mut current_speaker = sorted[0].speaker_id;
    let mut current_texts = vec![sorted[0].text.as_str()];
    let mut current_start = sorted[0].start;
    let mut current_end = sorted[0].end;

    let push_entry = |entries: &mut Vec<TranscriptEntry>,
                      texts: &[&str],
                      start: f64,
                      speaker: Option<i32>| {
        let mut entry =
            TranscriptEntry::new(TranscriptSource::System, join_segment_texts(texts), start);
        entry.speaker_id = speaker;
        entries.push(entry);
    };

    for seg in &sorted[1..] {
        let pause = seg.start - current_end;
        if seg.speaker_id == current_speaker
            && pause < PAUSE_THRESHOLD
            && seg.end - current_start < MAX_ENTRY_DURATION
        {
            current_texts.push(&seg.text);
            current_end = seg.end;
        } else {
            push_entry(&mut entries, &current_texts, current_start, current_speaker);
            current_speaker = seg.speaker_id;
            current_texts = vec![&seg.text];
            current_start = seg.start;
            current_end = seg.end;
        }
    }
    push_entry(&mut entries, &current_texts, current_start, current_speaker);

    entries
}

pub fn split_into_sentences(text: &str) -> Vec<String> {
    const ENDINGS: [char; 6] = ['。', '？', '！', '.', '?', '!'];
    const BREAK_CHARS: [char; 4] = ['、', ',', ' ', '\u{3000}'];
    const GROUP_SIZE: usize = 3;
    const MAX_CHARS: usize = 250;

    let mut sentences: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut current_len = 0usize;
    let mut sentence_count = 0usize;

    for ch in text.chars() {
        current.push(ch);
        current_len += 1;
        if ENDINGS.contains(&ch) {
            sentence_count += 1;
            if sentence_count >= GROUP_SIZE {
                sentences.push(std::mem::take(&mut current));
                current_len = 0;
                sentence_count = 0;
            }
        } else if current_len >= MAX_CHARS {
            if let Some((idx, brk)) = current
                .char_indices()
                .rev()
                .find(|(_, c)| BREAK_CHARS.contains(c))
            {
                let after = idx + brk.len_utf8();
                let rest = current[after..].to_string();
                current.truncate(after);
                sentences.push(std::mem::replace(&mut current, rest));
                current_len = current.chars().count();
            } else {
                sentences.push(std::mem::take(&mut current));
                current_len = 0;
            }
            sentence_count = 0;
        }
    }
    if !current.trim().is_empty() {
        sentences.push(current);
    }
    if sentences.is_empty() {
        vec![text.to_string()]
    } else {
        sentences
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(text: &str, start: f64, end: f64) -> TimedSegment {
        TimedSegment::new(text, start, end)
    }

    #[test]
    fn join_cjk_no_space() {
        let joined = join_segment_texts(&["こんにちは", "世界"]);
        assert_eq!(joined, "こんにちは世界");
    }

    #[test]
    fn join_latin_inserts_space() {
        let joined = join_segment_texts(&["Hello", "world"]);
        assert_eq!(joined, "Hello world");
    }

    #[test]
    fn join_mixed_no_space() {
        assert_eq!(join_segment_texts(&["こんにちは", "world"]), "こんにちはworld");
        assert_eq!(join_segment_texts(&["Hello", "世界"]), "Hello世界");
    }

    #[test]
    fn join_respects_leading_space_and_empties() {
        assert_eq!(join_segment_texts(&["Hello", " world"]), "Hello world");
        assert_eq!(join_segment_texts(&["Hello", "", "world"]), "Hello world");
        assert_eq!(join_segment_texts::<&str>(&[]), "");
        assert_eq!(join_segment_texts(&["", "world"]), "world");
    }

    #[test]
    fn echo_removed_at_50_percent_overlap() {
        let system = vec![seg("s", 0.0, 10.0)];
        let exactly_half = vec![seg("m", 5.0, 15.0)];
        assert!(deduplicate_mic_echo(&system, &exactly_half).is_empty());

        let just_under = vec![seg("m", 5.1, 15.1)];
        assert_eq!(deduplicate_mic_echo(&system, &just_under).len(), 1);

        let over = vec![seg("m", 2.0, 12.0)];
        assert!(deduplicate_mic_echo(&system, &over).is_empty());
    }

    #[test]
    fn echo_zero_duration_kept() {
        let system = vec![seg("s", 0.0, 10.0)];
        let mic = vec![seg("m", 5.0, 5.0)];
        assert_eq!(deduplicate_mic_echo(&system, &mic).len(), 1);
    }

    #[test]
    fn echo_empty_system_keeps_all() {
        let mic = vec![seg("m", 0.0, 1.0)];
        assert_eq!(deduplicate_mic_echo(&[], &mic).len(), 1);
    }

    #[test]
    fn echo_overlap_sums_across_system_segments() {
        let system = vec![seg("a", 0.0, 3.0), seg("b", 4.0, 7.0)];
        let mic = vec![seg("m", 0.0, 10.0)];
        assert!(deduplicate_mic_echo(&system, &mic).is_empty());
    }

    #[test]
    fn merge_same_source_within_gap() {
        let system = vec![seg("一つ目。", 0.0, 3.0), seg("二つ目。", 6.0, 9.0)];
        let entries = merge_segments(&system, &[]);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].text, "一つ目。二つ目。");
        assert_eq!(entries[0].start_time, 0.0);
    }

    #[test]
    fn merge_splits_at_gap_boundary() {
        let system = vec![seg("a", 0.0, 3.0), seg("b", 8.0, 9.0)];
        let entries = merge_segments(&system, &[]);
        assert_eq!(entries.len(), 2);
    }

    #[test]
    fn merge_splits_at_exactly_5s_gap() {
        let system = vec![seg("a", 0.0, 3.0), seg("b", 8.0, 9.0)];
        let entries = merge_segments(&system, &[]);
        assert_eq!(entries.len(), 2);
    }

    #[test]
    fn merge_splits_when_entry_reaches_30s() {
        let system = vec![
            seg("a", 0.0, 14.0),
            seg("b", 15.0, 29.0),
            seg("c", 29.5, 31.0),
        ];
        let entries = merge_segments(&system, &[]);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[1].text, "c");
    }

    #[test]
    fn merge_splits_on_source_change() {
        let system = vec![seg("sys", 0.0, 2.0)];
        let mic = vec![seg("mic", 2.5, 4.0)];
        let entries = merge_segments(&system, &mic);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].source, TranscriptSource::System);
        assert_eq!(entries[1].source, TranscriptSource::Mic);
    }

    #[test]
    fn merge_empty_inputs() {
        assert!(merge_segments(&[], &[]).is_empty());
    }

    #[test]
    fn assign_speakers_max_overlap_wins() {
        let segments = vec![seg("a", 0.0, 10.0)];
        let diar = vec![
            SpeakerTimedSegment { speaker_id: 0, start: 0.0, end: 3.0 },
            SpeakerTimedSegment { speaker_id: 1, start: 3.0, end: 10.0 },
        ];
        let assigned = assign_speakers(&segments, &diar);
        assert_eq!(assigned[0].speaker_id, Some(1));
    }

    #[test]
    fn assign_speakers_no_overlap_is_none() {
        let segments = vec![seg("a", 20.0, 25.0)];
        let diar = vec![SpeakerTimedSegment { speaker_id: 0, start: 0.0, end: 3.0 }];
        let assigned = assign_speakers(&segments, &diar);
        assert_eq!(assigned[0].speaker_id, None);
    }

    #[test]
    fn merge_with_speakers_groups_by_speaker() {
        let mut a = seg("こんにちは", 0.0, 2.0);
        a.speaker_id = Some(0);
        let mut b = seg("どうも", 2.5, 4.0);
        b.speaker_id = Some(0);
        let mut c = seg("はい", 4.5, 6.0);
        c.speaker_id = Some(1);
        let entries = merge_segments_with_speakers(&[a, b, c]);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].text, "こんにちはどうも");
        assert_eq!(entries[0].speaker_id, Some(0));
        assert_eq!(entries[1].speaker_id, Some(1));
    }

    #[test]
    fn merge_with_speakers_splits_at_3s_pause() {
        let mut a = seg("a", 0.0, 2.0);
        a.speaker_id = Some(0);
        let mut b = seg("b", 5.5, 6.0);
        b.speaker_id = Some(0);
        let entries = merge_segments_with_speakers(&[a, b]);
        assert_eq!(entries.len(), 2);
    }

    #[test]
    fn split_sentences_groups_of_three() {
        let text = "一。二。三。四。";
        let parts = split_into_sentences(text);
        assert_eq!(parts, vec!["一。二。三。".to_string(), "四。".to_string()]);
    }

    #[test]
    fn split_sentences_long_text_breaks_at_comma() {
        let long = format!("{}、{}", "あ".repeat(200), "い".repeat(100));
        let parts = split_into_sentences(&long);
        assert!(parts.len() >= 2);
        assert!(parts[0].ends_with('、'));
    }

    #[test]
    fn split_sentences_long_text_without_break() {
        let long = "あ".repeat(300);
        let parts = split_into_sentences(&long);
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0].chars().count(), 250);
    }

    #[test]
    fn split_sentences_empty_and_whitespace() {
        assert_eq!(split_into_sentences("abc"), vec!["abc".to_string()]);
        assert_eq!(split_into_sentences(""), vec!["".to_string()]);
    }
}
