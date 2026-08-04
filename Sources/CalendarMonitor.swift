import EventKit
import Foundation

struct MeetingEvent {
    let identifier: String
    let title: String
}

struct CalendarInfo: Identifiable {
    let id: String
    let title: String
    let color: CGColor
    let source: String
}

final class CalendarMonitor {
    private var eventStore = EKEventStore()
    private(set) var hasAccess = false

    private static let meetingURLPatterns = [
        "zoom.us",
        "meet.google.com",
        "teams.microsoft.com",
        "teams.live.com",
        "webex.com",
        "whereby.com",
        "facetime.apple.com",
    ]

    func requestAccess() async -> Bool {
        do {
            let granted = try await eventStore.requestFullAccessToEvents()
            if granted {
                eventStore = EKEventStore()
            }
            hasAccess = granted
            return granted
        } catch {
            return false
        }
    }

    func availableCalendars() -> [CalendarInfo] {
        guard EKEventStore.authorizationStatus(for: .event) == .fullAccess else { return [] }
        return EKEventStore().calendars(for: .event).map { cal in
            CalendarInfo(
                id: cal.calendarIdentifier,
                title: cal.title,
                color: cal.cgColor,
                source: cal.source.title
            )
        }.sorted { $0.title.localizedCompare($1.title) == .orderedAscending }
    }

    func findActiveMeetingEvent(minutesBefore: Int, requireMeetingURL: Bool, calendarIdentifiers: Set<String> = []) -> MeetingEvent? {
        guard hasAccess else { return nil }

        let now = Date()
        let searchStart = now.addingTimeInterval(-3600 * 8)
        let searchEnd = now.addingTimeInterval(TimeInterval(minutesBefore * 60 + 60))

        let calendars: [EKCalendar]? = calendarIdentifiers.isEmpty ? nil : eventStore.calendars(for: .event).filter { calendarIdentifiers.contains($0.calendarIdentifier) }
        let predicate = eventStore.predicateForEvents(withStart: searchStart, end: searchEnd, calendars: calendars)
        let events = eventStore.events(matching: predicate)

        guard let event = events.first(where: { event in
            guard !event.isAllDay else { return false }
            if requireMeetingURL && !Self.hasMeetingURL(event) { return false }
            let reminderStart = event.startDate.addingTimeInterval(-TimeInterval(minutesBefore * 60))
            return now >= reminderStart && now <= event.endDate
        }) else { return nil }

        return MeetingEvent(identifier: event.eventIdentifier, title: event.title ?? String(localized: "無題のイベント"))
    }

    private static func hasMeetingURL(_ event: EKEvent) -> Bool {
        if let url = event.url?.absoluteString.lowercased(),
           meetingURLPatterns.contains(where: { url.contains($0) }) {
            return true
        }
        for text in [event.notes, event.location].compactMap({ $0?.lowercased() }) {
            if meetingURLPatterns.contains(where: { text.contains($0) }) {
                return true
            }
        }
        return false
    }
}
