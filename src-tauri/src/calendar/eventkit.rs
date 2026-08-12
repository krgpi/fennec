use block2::{DynBlock, RcBlock};
use fennec_core::calendar::{CalendarEvent, CalendarInfo};
use objc2::rc::Retained;
use objc2::runtime::Bool;
use objc2::AllocAnyThread;
use objc2_core_graphics::CGColor;
use objc2_event_kit::{EKAuthorizationStatus, EKCalendar, EKEntityType, EKEventStore};
use objc2_foundation::{NSArray, NSDate, NSError};
use std::sync::mpsc;

fn new_store() -> Retained<EKEventStore> {
    unsafe { EKEventStore::init(EKEventStore::alloc()) }
}

pub fn has_full_access() -> bool {
    let status = unsafe { EKEventStore::authorizationStatusForEntityType(EKEntityType::Event) };
    status == EKAuthorizationStatus::FullAccess
}

pub fn request_access() -> bool {
    let store = new_store();
    let (tx, rx) = mpsc::sync_channel::<bool>(1);
    let block: RcBlock<dyn Fn(Bool, *mut NSError)> =
        RcBlock::new(move |granted: Bool, _error: *mut NSError| {
            let _ = tx.try_send(granted.as_bool());
        });
    unsafe {
        store.requestFullAccessToEventsWithCompletion(
            &*block as *const DynBlock<dyn Fn(Bool, *mut NSError)> as *mut _,
        );
    }
    rx.recv().unwrap_or(false)
}

pub fn list_calendars() -> Vec<CalendarInfo> {
    if !has_full_access() {
        return Vec::new();
    }
    let store = new_store();
    let calendars = unsafe { store.calendarsForEntityType(EKEntityType::Event) };
    let mut result: Vec<CalendarInfo> = calendars
        .to_vec()
        .iter()
        .map(|cal| unsafe {
            CalendarInfo {
                id: cal.calendarIdentifier().to_string(),
                title: cal.title().to_string(),
                color: cal.CGColor().as_deref().and_then(cg_color_hex),
                source: cal
                    .source()
                    .map(|s| s.title().to_string())
                    .unwrap_or_default(),
            }
        })
        .collect();
    result.sort_by(|a, b| a.title.cmp(&b.title));
    result
}

pub fn fetch_events(
    start_epoch: i64,
    end_epoch: i64,
    calendar_ids: Option<&[String]>,
) -> Vec<CalendarEvent> {
    if !has_full_access() {
        return Vec::new();
    }
    let store = new_store();
    let filter: Option<Retained<NSArray<EKCalendar>>> = match calendar_ids {
        Some(ids) if !ids.is_empty() => {
            let all = unsafe { store.calendarsForEntityType(EKEntityType::Event) };
            let matched: Vec<Retained<EKCalendar>> = all
                .to_vec()
                .into_iter()
                .filter(|cal| {
                    let id = unsafe { cal.calendarIdentifier() }.to_string();
                    ids.iter().any(|wanted| wanted == &id)
                })
                .collect();
            Some(NSArray::from_retained_slice(&matched))
        }
        _ => None,
    };

    unsafe {
        let start = NSDate::dateWithTimeIntervalSince1970(start_epoch as f64);
        let end = NSDate::dateWithTimeIntervalSince1970(end_epoch as f64);
        let predicate =
            store.predicateForEventsWithStartDate_endDate_calendars(&start, &end, filter.as_deref());
        let events = store.eventsMatchingPredicate(&predicate);
        events
            .to_vec()
            .iter()
            .map(|event| CalendarEvent {
                identifier: event
                    .eventIdentifier()
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| event.calendarItemIdentifier().to_string()),
                title: event.title().to_string(),
                start_epoch: event.startDate().timeIntervalSince1970() as i64,
                end_epoch: event.endDate().timeIntervalSince1970() as i64,
                is_all_day: event.isAllDay(),
                url: event.URL().and_then(|u| u.absoluteString()).map(|s| s.to_string()),
                notes: event.notes().map(|s| s.to_string()),
                location: event.location().map(|s| s.to_string()),
            })
            .collect()
    }
}

fn cg_color_hex(color: &CGColor) -> Option<String> {
    let count = CGColor::number_of_components(Some(color));
    let ptr = CGColor::components(Some(color));
    if ptr.is_null() {
        return None;
    }
    let components = unsafe { std::slice::from_raw_parts(ptr, count) };
    let (r, g, b) = match count {
        2 => (components[0], components[0], components[0]),
        3 | 4 => (components[0], components[1], components[2]),
        _ => return None,
    };
    let to8 = |v: f64| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    Some(format!("#{:02X}{:02X}{:02X}", to8(r), to8(g), to8(b)))
}
