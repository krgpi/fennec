use serde::{Deserialize, Serialize};

pub const MEETING_URL_PATTERNS: [&str; 7] = [
    "zoom.us",
    "meet.google.com",
    "teams.microsoft.com",
    "teams.live.com",
    "webex.com",
    "whereby.com",
    "facetime.apple.com",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CalendarInfo {
    pub id: String,
    pub title: String,
    pub color: Option<String>,
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CalendarEvent {
    pub identifier: String,
    pub title: String,
    pub start_epoch: i64,
    pub end_epoch: i64,
    pub is_all_day: bool,
    pub url: Option<String>,
    pub notes: Option<String>,
    pub location: Option<String>,
}

pub fn has_meeting_url(event: &CalendarEvent) -> bool {
    if let Some(url) = &event.url {
        let url = url.to_lowercase();
        if MEETING_URL_PATTERNS.iter().any(|p| url.contains(p)) {
            return true;
        }
    }
    for text in [&event.notes, &event.location].into_iter().flatten() {
        let text = text.to_lowercase();
        if MEETING_URL_PATTERNS.iter().any(|p| text.contains(p)) {
            return true;
        }
    }
    false
}

pub fn find_active_meeting_event(
    events: &[CalendarEvent],
    now_epoch: i64,
    minutes_before: i64,
    require_meeting_url: bool,
) -> Option<&CalendarEvent> {
    events.iter().find(|event| {
        if event.is_all_day {
            return false;
        }
        if require_meeting_url && !has_meeting_url(event) {
            return false;
        }
        let reminder_start = event.start_epoch - minutes_before * 60;
        now_epoch >= reminder_start && now_epoch <= event.end_epoch
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(url: Option<&str>, notes: Option<&str>, location: Option<&str>) -> CalendarEvent {
        CalendarEvent {
            identifier: "e1".into(),
            title: "会議".into(),
            start_epoch: 1000,
            end_epoch: 4600,
            is_all_day: false,
            url: url.map(String::from),
            notes: notes.map(String::from),
            location: location.map(String::from),
        }
    }

    #[test]
    fn detects_url_in_each_field() {
        assert!(has_meeting_url(&event(Some("https://zoom.us/j/123"), None, None)));
        assert!(has_meeting_url(&event(None, Some("参加: https://meet.google.com/abc"), None)));
        assert!(has_meeting_url(&event(None, None, Some("Teams.Microsoft.com/l/x"))));
        assert!(!has_meeting_url(&event(Some("https://example.com"), None, None)));
    }

    #[test]
    fn active_window_includes_reminder_lead() {
        let events = vec![event(Some("https://zoom.us/j/1"), None, None)];
        assert!(find_active_meeting_event(&events, 700, 5, true).is_some());
        assert!(find_active_meeting_event(&events, 699, 5, true).is_none());
        assert!(find_active_meeting_event(&events, 4600, 5, true).is_some());
        assert!(find_active_meeting_event(&events, 4601, 5, true).is_none());
    }

    #[test]
    fn all_day_and_missing_url_excluded() {
        let mut all_day = event(Some("https://zoom.us/j/1"), None, None);
        all_day.is_all_day = true;
        assert!(find_active_meeting_event(&[all_day], 2000, 5, true).is_none());

        let plain = event(None, None, None);
        assert!(find_active_meeting_event(&[plain.clone()], 2000, 5, true).is_none());
        assert!(find_active_meeting_event(&[plain], 2000, 5, false).is_some());
    }
}
