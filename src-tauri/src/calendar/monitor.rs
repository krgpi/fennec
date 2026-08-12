use crate::state::AppState;
use fennec_core::calendar::{find_active_meeting_event, CalendarEvent};
use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Manager};
use tauri_plugin_notification::NotificationExt;

const CHECK_INTERVAL: Duration = Duration::from_secs(30);

pub fn start(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let ics_cache = Arc::new(super::ics::IcsCache::new());
        let notified = Arc::new(Mutex::new(HashSet::<String>::new()));
        loop {
            tokio::time::sleep(CHECK_INTERVAL).await;
            let app = app.clone();
            let ics_cache = ics_cache.clone();
            let notified = notified.clone();
            let result = tauri::async_runtime::spawn_blocking(move || {
                check(&app, &ics_cache, &notified);
            })
            .await;
            if let Err(e) = result {
                log::warn!("meeting reminder check failed: {e}");
            }
        }
    });
}

fn check(app: &AppHandle, ics_cache: &super::ics::IcsCache, notified: &Mutex<HashSet<String>>) {
    let state = app.state::<AppState>();
    let (enabled, minutes_before, require_url, calendar_ids, ics_urls) = {
        let settings = state.settings.read().unwrap();
        (
            settings.meeting_reminder_enabled,
            settings.meeting_reminder_minutes_before as i64,
            settings.meeting_reminder_require_meeting_url,
            settings.meeting_reminder_calendar_identifiers.clone(),
            settings.calendar_ics_urls.clone(),
        )
    };
    if !enabled {
        return;
    }
    if state.recorder.lock().unwrap().is_some() {
        return;
    }

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let search_start = now - 3600 * 8;
    let search_end = now + minutes_before * 60 + 60;

    let mut events: Vec<CalendarEvent> = Vec::new();
    #[cfg(target_os = "macos")]
    events.extend(super::eventkit::fetch_events(
        search_start,
        search_end,
        calendar_ids.as_deref(),
    ));
    #[cfg(not(target_os = "macos"))]
    let _ = (search_start, search_end, calendar_ids);
    events.extend(ics_cache.events_for(&ics_urls));

    let Some(event) = find_active_meeting_event(&events, now, minutes_before, require_url) else {
        return;
    };

    if !notified.lock().unwrap().insert(event.identifier.clone()) {
        return;
    }

    let result = app
        .notification()
        .builder()
        .title("会議リマインダー")
        .body(format!(
            "「{}」の時間です。録音を開始しますか？",
            event.title
        ))
        .show();
    if let Err(e) = result {
        log::warn!("failed to show meeting reminder notification: {e}");
    }
}
