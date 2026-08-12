use chrono::{Local, NaiveDate, NaiveDateTime, TimeZone};
use fennec_core::calendar::CalendarEvent;
use ical::parser::ical::component::IcalEvent;
use ical::property::Property;
use std::collections::HashMap;
use std::io::BufReader;
use std::sync::Mutex;
use std::time::{Duration, Instant};

const CACHE_TTL: Duration = Duration::from_secs(300);
const FETCH_TIMEOUT: Duration = Duration::from_secs(15);

struct CacheEntry {
    fetched_at: Instant,
    events: Vec<CalendarEvent>,
}

pub struct IcsCache {
    entries: Mutex<HashMap<String, CacheEntry>>,
    agent: ureq::Agent,
}

impl IcsCache {
    pub fn new() -> Self {
        let agent = ureq::Agent::config_builder()
            .timeout_global(Some(FETCH_TIMEOUT))
            .build()
            .new_agent();
        Self {
            entries: Mutex::new(HashMap::new()),
            agent,
        }
    }

    pub fn events_for(&self, urls: &[String]) -> Vec<CalendarEvent> {
        let mut out = Vec::new();
        for url in urls {
            let url = url.trim();
            if url.is_empty() {
                continue;
            }
            {
                let entries = self.entries.lock().unwrap();
                if let Some(entry) = entries.get(url) {
                    if entry.fetched_at.elapsed() < CACHE_TTL {
                        out.extend(entry.events.iter().cloned());
                        continue;
                    }
                }
            }
            let events = match self.fetch(url) {
                Ok(events) => events,
                Err(e) => {
                    log::warn!("failed to fetch ICS calendar {url}: {e}");
                    let mut entries = self.entries.lock().unwrap();
                    match entries.get_mut(url) {
                        Some(entry) => {
                            entry.fetched_at = Instant::now();
                            out.extend(entry.events.iter().cloned());
                        }
                        None => {
                            entries.insert(
                                url.to_string(),
                                CacheEntry {
                                    fetched_at: Instant::now(),
                                    events: Vec::new(),
                                },
                            );
                        }
                    }
                    continue;
                }
            };
            out.extend(events.iter().cloned());
            self.entries.lock().unwrap().insert(
                url.to_string(),
                CacheEntry {
                    fetched_at: Instant::now(),
                    events,
                },
            );
        }
        out
    }

    fn fetch(&self, url: &str) -> anyhow::Result<Vec<CalendarEvent>> {
        let mut response = self.agent.get(url).call()?;
        let text = response.body_mut().read_to_string()?;
        Ok(parse_ics(&text))
    }
}

pub fn parse_ics(text: &str) -> Vec<CalendarEvent> {
    let mut events = Vec::new();
    for calendar in ical::IcalParser::new(BufReader::new(text.as_bytes())).flatten() {
        for event in &calendar.events {
            if let Some(converted) = convert_event(event) {
                events.push(converted);
            }
        }
    }
    events
}

fn convert_event(event: &IcalEvent) -> Option<CalendarEvent> {
    let find = |name: &str| event.properties.iter().find(|p| p.name == name);
    if find("RRULE").is_some() {
        return None;
    }

    let dtstart = find("DTSTART")?;
    let (start_epoch, is_all_day) = parse_datetime(dtstart)?;
    let end_epoch = find("DTEND")
        .and_then(parse_datetime_prop)
        .unwrap_or(if is_all_day {
            start_epoch + 86400
        } else {
            start_epoch
        });

    let text_value = |name: &str| {
        find(name)
            .and_then(|p| p.value.as_deref())
            .map(unescape_text)
            .filter(|s| !s.is_empty())
    };

    let title = text_value("SUMMARY").unwrap_or_else(|| "無題のイベント".to_string());
    let identifier = text_value("UID").unwrap_or_else(|| format!("ics-{start_epoch}-{title}"));

    Some(CalendarEvent {
        identifier,
        title,
        start_epoch,
        end_epoch,
        is_all_day,
        url: text_value("URL"),
        notes: text_value("DESCRIPTION"),
        location: text_value("LOCATION"),
    })
}

fn parse_datetime_prop(prop: &Property) -> Option<i64> {
    parse_datetime(prop).map(|(epoch, _)| epoch)
}

fn parse_datetime(prop: &Property) -> Option<(i64, bool)> {
    let value = prop.value.as_deref()?.trim();
    let is_date_type = prop
        .params
        .as_ref()
        .map(|params| {
            params
                .iter()
                .any(|(key, values)| key == "VALUE" && values.iter().any(|v| v == "DATE"))
        })
        .unwrap_or(false);

    if is_date_type || value.len() == 8 {
        let date = NaiveDate::parse_from_str(value, "%Y%m%d").ok()?;
        let local = Local
            .from_local_datetime(&date.and_hms_opt(0, 0, 0)?)
            .earliest()?;
        return Some((local.timestamp(), true));
    }

    if let Some(stripped) = value.strip_suffix('Z') {
        let naive = NaiveDateTime::parse_from_str(stripped, "%Y%m%dT%H%M%S").ok()?;
        return Some((naive.and_utc().timestamp(), false));
    }

    // TZID は解決せずローカル時刻として扱う近似
    let naive = NaiveDateTime::parse_from_str(value, "%Y%m%dT%H%M%S").ok()?;
    let local = Local.from_local_datetime(&naive).earliest()?;
    Some((local.timestamp(), false))
}

fn unescape_text(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') | Some('N') => out.push('\n'),
            Some(escaped) => out.push(escaped),
            None => out.push('\\'),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use fennec_core::calendar::{find_active_meeting_event, has_meeting_url};

    const FIXTURE: &str = "BEGIN:VCALENDAR\r\n\
VERSION:2.0\r\n\
BEGIN:VEVENT\r\n\
UID:evt-1@example.com\r\n\
DTSTART:20260810T010000Z\r\n\
DTEND:20260810T020000Z\r\n\
SUMMARY:定例ミーティング\r\n\
DESCRIPTION:参加する: https://zoom.us/j/123456\\nメモ\\, その他\r\n\
LOCATION:会議室A\r\n\
END:VEVENT\r\n\
BEGIN:VEVENT\r\n\
UID:evt-2@example.com\r\n\
DTSTART;VALUE=DATE:20260811\r\n\
SUMMARY:終日イベント\r\n\
END:VEVENT\r\n\
BEGIN:VEVENT\r\n\
UID:evt-3@example.com\r\n\
DTSTART:20260812T090000Z\r\n\
DTEND:20260812T100000Z\r\n\
RRULE:FREQ=WEEKLY\r\n\
SUMMARY:繰り返し\r\n\
END:VEVENT\r\n\
BEGIN:VEVENT\r\n\
UID:evt-4@example.com\r\n\
DTSTART:20260813T090000Z\r\n\
DTEND:20260813T100000Z\r\n\
SUMMARY:URLプロパティ\r\n\
URL:https://meet.google.com/abc-defg-hij\r\n\
END:VEVENT\r\n\
END:VCALENDAR\r\n";

    #[test]
    fn parses_vevents_into_calendar_events() {
        let events = parse_ics(FIXTURE);
        assert_eq!(events.len(), 3);

        let first = &events[0];
        assert_eq!(first.identifier, "evt-1@example.com");
        assert_eq!(first.title, "定例ミーティング");
        assert_eq!(first.start_epoch, 1786323600);
        assert_eq!(first.end_epoch, 1786327200);
        assert!(!first.is_all_day);
        assert_eq!(first.location.as_deref(), Some("会議室A"));
        assert_eq!(
            first.notes.as_deref(),
            Some("参加する: https://zoom.us/j/123456\nメモ, その他")
        );
        assert!(has_meeting_url(first));

        let all_day = &events[1];
        assert!(all_day.is_all_day);
        assert_eq!(all_day.end_epoch - all_day.start_epoch, 86400);

        let with_url = &events[2];
        assert_eq!(
            with_url.url.as_deref(),
            Some("https://meet.google.com/abc-defg-hij")
        );
        assert!(has_meeting_url(with_url));
    }

    #[test]
    fn rrule_events_are_skipped() {
        let events = parse_ics(FIXTURE);
        assert!(events.iter().all(|e| e.identifier != "evt-3@example.com"));
    }

    #[test]
    fn parsed_events_work_with_core_window_check() {
        let events = parse_ics(FIXTURE);
        let found = find_active_meeting_event(&events, 1786323600 - 5 * 60, 5, true);
        assert_eq!(found.map(|e| e.identifier.as_str()), Some("evt-1@example.com"));
        assert!(find_active_meeting_event(&events, 1786323600 - 6 * 60, 5, true).is_none());
    }
}
