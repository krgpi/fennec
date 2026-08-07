import Accelerate
import AVFoundation
import CoreAudio
import CoreMedia
import ScreenCaptureKit
import UserNotifications

struct InputDevice: Identifiable, Hashable {
    let id: AudioDeviceID
    let uid: String
    let name: String
    let sampleRate: Double
    let channelCount: Int
}

struct TimedSegment {
    let text: String
    let start: TimeInterval
    let end: TimeInterval
    var speakerId: Int?
}

enum TranscriptSource {
    case system
    case mic

    var label: String {
        switch self {
        case .system: return String(localized: "transcript.source.system", defaultValue: "PC音声")
        case .mic: return String(localized: "transcript.source.mic", defaultValue: "マイク")
        }
    }
}

struct TranscriptEntry: Identifiable {
    let id = UUID()
    let source: TranscriptSource
    let text: String
    let startTime: TimeInterval
    var speakerId: Int?
    var translation: String?
}

func joinSegmentTexts(_ texts: [String]) -> String {
    guard let first = texts.first else { return "" }
    var result = first
    for i in 1..<texts.count {
        let next = texts[i]
        if next.isEmpty { continue }
        if next.hasPrefix(" ") || result.isEmpty {
            result += next
        } else if let lastChar = result.last, !lastChar.isCJK, let firstChar = next.first, !firstChar.isCJK {
            result += " " + next
        } else {
            result += next
        }
    }
    return result
}

private extension Character {
    var isCJK: Bool {
        return unicodeScalars.contains { scalar in
            let v = scalar.value
            return (0x3000...0x9FFF).contains(v) ||
                   (0xAC00...0xD7AF).contains(v) ||
                   (0xF900...0xFAFF).contains(v) ||
                   (0x20000...0x2FA1F).contains(v) ||
                   (0xFF01...0xFF60).contains(v) ||
                   (0xFFE0...0xFFEF).contains(v)
        }
    }
}

final class AudioCaptureManager: NSObject, ObservableObject {
    @Published var isRecording = false
    @Published var systemAudioURL: URL?
    @Published var micAudioURL: URL?
    @Published var errorMessage: String?
    @Published var duration: TimeInterval = 0

    @Published var inputDevices: [InputDevice] = []
    @Published var selectedDeviceID: AudioDeviceID?
    @Published var useSystemDefaultMic: Bool = UserDefaults.standard.object(forKey: "useSystemDefaultMic") == nil
        ? true
        : UserDefaults.standard.bool(forKey: "useSystemDefaultMic") {
        didSet {
            UserDefaults.standard.set(useSystemDefaultMic, forKey: "useSystemDefaultMic")
            if useSystemDefaultMic {
                selectedDeviceID = defaultInputDeviceID() ?? inputDevices.first?.id
            }
        }
    }
    @Published var defaultInputDeviceName: String?
    @Published var sysTranscript = ""
    @Published var micTranscript = ""
    @Published var mergedTranscript: [TranscriptEntry] = []
    @Published var isTranscribing = false
    @Published var transcriptFileURL: URL?
    @Published var autoTranscribeSessionId: String?
    @Published var autoTranscribePhase: String?
    @Published var autoTranscribeProgress: Double?
    @Published var sysAudioLevel: Float = 0
    @Published var micAudioLevel: Float = 0
    @Published var liveTranscriptDiag: String = ""
    @Published var transcriptionLocale: Locale = TranscriptionLocale.stored {
        didSet {
            UserDefaults.standard.set(transcriptionLocale.identifier, forKey: "transcriptionLocale")
        }
    }
    @Published var liveTranscriptionEnabled: Bool = UserDefaults.standard.object(forKey: "liveTranscriptionEnabled") == nil ? true : UserDefaults.standard.bool(forKey: "liveTranscriptionEnabled") {
        didSet {
            UserDefaults.standard.set(liveTranscriptionEnabled, forKey: "liveTranscriptionEnabled")
        }
    }
    #if DEBUG
    @Published var monitorMicUsage: Bool = false {
        didSet {
            UserDefaults.standard.set(monitorMicUsage, forKey: "monitorMicUsage")
            if monitorMicUsage {
                startMicUsageListener()
            } else {
                stopMicUsageListener()
            }
        }
    }
    #endif
    @Published var showMicStartAlert = false
    @Published var showMicStopAlert = false
    @Published var microphoneMode: AVCaptureDevice.MicrophoneMode = AVCaptureDevice.activeMicrophoneMode
    @Published var silenceAutoStopEnabled: Bool = UserDefaults.standard.bool(forKey: "silenceAutoStopEnabled") {
        didSet {
            UserDefaults.standard.set(silenceAutoStopEnabled, forKey: "silenceAutoStopEnabled")
            if silenceAutoStopEnabled {
                Self.requestNotificationPermission()
            }
        }
    }
    @Published var silenceAutoStopMinutes: Int = {
        let stored = UserDefaults.standard.integer(forKey: "silenceAutoStopMinutes")
        return stored > 0 ? stored : 5
    }() {
        didSet {
            UserDefaults.standard.set(silenceAutoStopMinutes, forKey: "silenceAutoStopMinutes")
        }
    }
    @Published var stoppedBySilenceDetection = false
    @Published var micNoInputWarning = false
    @Published var meetingReminderEnabled: Bool = false {
        didSet {
            UserDefaults.standard.set(meetingReminderEnabled, forKey: "meetingReminderEnabled")
            if meetingReminderEnabled {
                Self.requestNotificationPermission()
                startMeetingReminderMonitor()
            } else {
                stopMeetingReminderMonitor()
            }
        }
    }
    @Published var meetingReminderMinutesBefore: Int = {
        let stored = UserDefaults.standard.integer(forKey: "meetingReminderMinutesBefore")
        return stored > 0 ? stored : 5
    }() {
        didSet {
            UserDefaults.standard.set(meetingReminderMinutesBefore, forKey: "meetingReminderMinutesBefore")
        }
    }
    @Published var meetingReminderRequireMeetingURL: Bool = {
        UserDefaults.standard.object(forKey: "meetingReminderRequireMeetingURL") == nil
            ? true : UserDefaults.standard.bool(forKey: "meetingReminderRequireMeetingURL")
    }() {
        didSet {
            UserDefaults.standard.set(meetingReminderRequireMeetingURL, forKey: "meetingReminderRequireMeetingURL")
        }
    }
    @Published var meetingReminderRequireMic: Bool = {
        UserDefaults.standard.object(forKey: "meetingReminderRequireMic") == nil
            ? true : UserDefaults.standard.bool(forKey: "meetingReminderRequireMic")
    }() {
        didSet {
            UserDefaults.standard.set(meetingReminderRequireMic, forKey: "meetingReminderRequireMic")
        }
    }
    @Published var meetingReminderCalendarIdentifiers: Set<String>? = {
        guard let stored = UserDefaults.standard.stringArray(forKey: "meetingReminderCalendarIdentifiers") else { return nil }
        return Set(stored)
    }() {
        didSet {
            if let ids = meetingReminderCalendarIdentifiers {
                UserDefaults.standard.set(Array(ids), forKey: "meetingReminderCalendarIdentifiers")
            } else {
                UserDefaults.standard.removeObject(forKey: "meetingReminderCalendarIdentifiers")
            }
        }
    }
    @Published var diarizationEnabled: Bool = UserDefaults.standard.bool(forKey: "diarizationEnabled") {
        didSet {
            UserDefaults.standard.set(diarizationEnabled, forKey: "diarizationEnabled")
        }
    }
    @Published var autoTranscribeEnabled: Bool = UserDefaults.standard.object(forKey: "autoTranscribeEnabled") == nil ? true : UserDefaults.standard.bool(forKey: "autoTranscribeEnabled") {
        didSet {
            UserDefaults.standard.set(autoTranscribeEnabled, forKey: "autoTranscribeEnabled")
        }
    }
    @Published var autoTranscribeEngine: TranscriptionEngineType = TranscriptionEngineType(rawValue: UserDefaults.standard.string(forKey: "autoTranscribeEngine") ?? "") ?? .apple {
        didSet {
            UserDefaults.standard.set(autoTranscribeEngine.rawValue, forKey: "autoTranscribeEngine")
        }
    }
    @Published var isDiarizing = false

    var whisperModelManager: WhisperModelManager?
    var translationService: TranslationService?
    private var whisperTranscriber = WhisperTranscriber()
    #if DEBUG
    let speakerDiarizer = SpeakerDiarizer()
    #endif
    private(set) var currentSessionFolder: URL?
    private var autoTranscribeTask: Task<Void, Never>?
    private var currentAutoTranscriber: Transcriber?

    private var sysStreamingTranscriber: StreamingTranscriber?
    private var micStreamingTranscriber: StreamingTranscriber?

    private var stream: SCStream?
    private var sysWriter: AVAssetWriter?
    private var sysWriterInput: AVAssetWriterInput?
    private var sysSessionStarted = false

    private var captureSession: AVCaptureSession?
    private var micWriter: AVAssetWriter?
    private var micWriterInput: AVAssetWriterInput?
    private var micSessionStarted = false

    private var timer: Timer?
    private var recordingStart: Date?

    private let sysQueue = DispatchQueue(label: "io.github.krgpi.Fennec.sys-audio")
    private let micQueue = DispatchQueue(label: "io.github.krgpi.Fennec.mic-audio")
    private let micGain: Float = 2.0
    private var peakSysLevel: Float = 0
    private var peakMicLevel: Float = 0
    private var deviceRefreshWork: DispatchWorkItem?
    #if DEBUG
    private var micUsageProcessObjectIDs: Set<AudioObjectID> = []
    private var micUsageListeningProcessList = false
    private var micOtherAppActive = false
    private var micUsagePollTimer: Timer?
    #endif
    private var silenceStartDate: Date?
    private var silenceAutoStopTriggered = false
    private let silenceLevelThreshold: Float = 0.02
    private var micEverHadInput = false
    private let micNoInputGracePeriod: TimeInterval = 10
    private var micNoInputAlertShown = false
    private var micInputTicks = 0
    private let micInputRequiredTicks = 8
    let calendarMonitor = CalendarMonitor()
    private var meetingReminderTimer: Timer?
    private var promptedEventIdentifiers: Set<String> = []

    override init() {
        super.init()
        refreshInputDevices()
        startDeviceListener()
        Self.registerNotificationCategories()
        UNUserNotificationCenter.current().delegate = self
        #if DEBUG
        if UserDefaults.standard.bool(forKey: "monitorMicUsage") {
            monitorMicUsage = true
            startMicUsageListener()
        }
        #endif
        if UserDefaults.standard.bool(forKey: "meetingReminderEnabled") {
            meetingReminderEnabled = true
        }
        #if DEBUG
        if diarizationEnabled && !speakerDiarizer.isModelReady {
            diarizationEnabled = false
        }
        #endif
    }

    deinit {
        stopDeviceListener()
        #if DEBUG
        stopMicUsageListener()
        #endif
        stopMeetingReminderMonitor()
    }

    func refreshMicrophoneMode() {
        microphoneMode = AVCaptureDevice.activeMicrophoneMode
    }

    // MARK: - Device Enumeration

    func refreshInputDevices() {
        var address = AudioObjectPropertyAddress(
            mSelector: kAudioHardwarePropertyDevices,
            mScope: kAudioObjectPropertyScopeGlobal,
            mElement: kAudioObjectPropertyElementMain
        )
        var size: UInt32 = 0
        guard AudioObjectGetPropertyDataSize(
            AudioObjectID(kAudioObjectSystemObject), &address, 0, nil, &size
        ) == noErr else { return }

        let count = Int(size) / MemoryLayout<AudioDeviceID>.size
        var deviceIDs = [AudioDeviceID](repeating: 0, count: count)
        guard AudioObjectGetPropertyData(
            AudioObjectID(kAudioObjectSystemObject), &address, 0, nil, &size, &deviceIDs
        ) == noErr else { return }

        var devices: [InputDevice] = []
        for id in deviceIDs {
            guard let info = inputDeviceInfo(for: id) else { continue }
            devices.append(info)
        }

        inputDevices = devices

        let defaultID = defaultInputDeviceID()
        defaultInputDeviceName = devices.first(where: { $0.id == defaultID })?.name

        if useSystemDefaultMic || selectedDeviceID == nil || !devices.contains(where: { $0.id == selectedDeviceID }) {
            selectedDeviceID = defaultID ?? devices.first?.id
        }
    }

    private func inputDeviceInfo(for id: AudioDeviceID) -> InputDevice? {
        var streamAddr = AudioObjectPropertyAddress(
            mSelector: kAudioDevicePropertyStreamConfiguration,
            mScope: kAudioDevicePropertyScopeInput,
            mElement: kAudioObjectPropertyElementMain
        )
        var streamSize: UInt32 = 0
        guard AudioObjectGetPropertyDataSize(id, &streamAddr, 0, nil, &streamSize) == noErr else { return nil }

        let raw = UnsafeMutableRawPointer.allocate(byteCount: Int(streamSize), alignment: MemoryLayout<AudioBufferList>.alignment)
        defer { raw.deallocate() }
        guard AudioObjectGetPropertyData(id, &streamAddr, 0, nil, &streamSize, raw) == noErr else { return nil }

        let bufferList = UnsafeMutableAudioBufferListPointer(raw.assumingMemoryBound(to: AudioBufferList.self))
        let channels = bufferList.reduce(0) { $0 + Int($1.mNumberChannels) }
        guard channels > 0 else { return nil }

        var nameAddr = AudioObjectPropertyAddress(
            mSelector: kAudioObjectPropertyName,
            mScope: kAudioObjectPropertyScopeGlobal,
            mElement: kAudioObjectPropertyElementMain
        )
        var name: CFString = "" as CFString
        var nameSize = UInt32(MemoryLayout<CFString>.size)
        guard AudioObjectGetPropertyData(id, &nameAddr, 0, nil, &nameSize, &name) == noErr else { return nil }

        var uidAddr = AudioObjectPropertyAddress(
            mSelector: kAudioDevicePropertyDeviceUID,
            mScope: kAudioObjectPropertyScopeGlobal,
            mElement: kAudioObjectPropertyElementMain
        )
        var uid: CFString = "" as CFString
        var uidSize = UInt32(MemoryLayout<CFString>.size)
        guard AudioObjectGetPropertyData(id, &uidAddr, 0, nil, &uidSize, &uid) == noErr else { return nil }

        var rateAddr = AudioObjectPropertyAddress(
            mSelector: kAudioDevicePropertyNominalSampleRate,
            mScope: kAudioObjectPropertyScopeGlobal,
            mElement: kAudioObjectPropertyElementMain
        )
        var rate: Float64 = 0
        var rateSize = UInt32(MemoryLayout<Float64>.size)
        AudioObjectGetPropertyData(id, &rateAddr, 0, nil, &rateSize, &rate)

        return InputDevice(id: id, uid: uid as String, name: name as String, sampleRate: rate, channelCount: channels)
    }

    private func startDeviceListener() {
        let ptr = Unmanaged.passUnretained(self).toOpaque()
        var address = AudioObjectPropertyAddress(
            mSelector: kAudioHardwarePropertyDevices,
            mScope: kAudioObjectPropertyScopeGlobal,
            mElement: kAudioObjectPropertyElementMain
        )
        AudioObjectAddPropertyListener(
            AudioObjectID(kAudioObjectSystemObject),
            &address,
            audioDeviceListChanged,
            ptr
        )
        var defaultAddr = AudioObjectPropertyAddress(
            mSelector: kAudioHardwarePropertyDefaultInputDevice,
            mScope: kAudioObjectPropertyScopeGlobal,
            mElement: kAudioObjectPropertyElementMain
        )
        AudioObjectAddPropertyListener(
            AudioObjectID(kAudioObjectSystemObject),
            &defaultAddr,
            defaultInputDeviceChanged,
            ptr
        )
    }

    private func stopDeviceListener() {
        let ptr = Unmanaged.passUnretained(self).toOpaque()
        var address = AudioObjectPropertyAddress(
            mSelector: kAudioHardwarePropertyDevices,
            mScope: kAudioObjectPropertyScopeGlobal,
            mElement: kAudioObjectPropertyElementMain
        )
        AudioObjectRemovePropertyListener(
            AudioObjectID(kAudioObjectSystemObject),
            &address,
            audioDeviceListChanged,
            ptr
        )
        var defaultAddr = AudioObjectPropertyAddress(
            mSelector: kAudioHardwarePropertyDefaultInputDevice,
            mScope: kAudioObjectPropertyScopeGlobal,
            mElement: kAudioObjectPropertyElementMain
        )
        AudioObjectRemovePropertyListener(
            AudioObjectID(kAudioObjectSystemObject),
            &defaultAddr,
            defaultInputDeviceChanged,
            ptr
        )
    }

    func handleDefaultInputDeviceChanged() {
        DispatchQueue.main.async { [weak self] in
            guard let self else { return }
            let newDefault = self.defaultInputDeviceID()
            self.defaultInputDeviceName = self.inputDevices.first(where: { $0.id == newDefault })?.name
            if self.useSystemDefaultMic, let newDefault {
                self.selectedDeviceID = newDefault
            }
        }
    }

    func scheduleDeviceRefresh() {
        DispatchQueue.main.async { [weak self] in
            guard let self else { return }
            self.deviceRefreshWork?.cancel()
            let work = DispatchWorkItem { [weak self] in
                self?.refreshInputDevices()
            }
            self.deviceRefreshWork = work
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.3, execute: work)
        }
    }

    // MARK: - Mic Usage Detection

    private static let processObjectListAddress = AudioObjectPropertyAddress(
        mSelector: kAudioHardwarePropertyProcessObjectList,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMain
    )
    private static let processIsRunningInputAddress = AudioObjectPropertyAddress(
        mSelector: kAudioProcessPropertyIsRunningInput,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMain
    )

    private func audioProcessObjectList() -> [AudioObjectID] {
        var address = Self.processObjectListAddress
        var dataSize: UInt32 = 0
        guard AudioObjectGetPropertyDataSize(
            AudioObjectID(kAudioObjectSystemObject), &address, 0, nil, &dataSize
        ) == noErr, dataSize > 0 else { return [] }
        var objects = [AudioObjectID](repeating: 0, count: Int(dataSize) / MemoryLayout<AudioObjectID>.size)
        guard AudioObjectGetPropertyData(
            AudioObjectID(kAudioObjectSystemObject), &address, 0, nil, &dataSize, &objects
        ) == noErr else { return [] }
        return objects
    }

    private static let micUsageSystemIgnoredBundleIDs: Set<String> = [
        "com.apple.CoreSpeech",
        "com.apple.replayd",
    ]

    static let defaultMicUsageExcludedBundleIDs = [
        "com.apple.podcasts",
        "com.apple.Music",
        "com.apple.TV",
        "com.apple.iBooksX",
        "com.apple.news",
    ]

    #if DEBUG
    @Published var micUsageExcludedBundleIDs: [String] = UserDefaults.standard.stringArray(forKey: "micUsageExcludedBundleIDs") ?? AudioCaptureManager.defaultMicUsageExcludedBundleIDs {
        didSet {
            UserDefaults.standard.set(micUsageExcludedBundleIDs, forKey: "micUsageExcludedBundleIDs")
        }
    }
    #else
    private let micUsageExcludedBundleIDs: [String] = AudioCaptureManager.defaultMicUsageExcludedBundleIDs
    #endif

    private func processPID(_ object: AudioObjectID) -> pid_t? {
        var address = AudioObjectPropertyAddress(
            mSelector: kAudioProcessPropertyPID,
            mScope: kAudioObjectPropertyScopeGlobal,
            mElement: kAudioObjectPropertyElementMain
        )
        var pid: pid_t = -1
        var size = UInt32(MemoryLayout<pid_t>.size)
        guard AudioObjectGetPropertyData(object, &address, 0, nil, &size, &pid) == noErr else { return nil }
        return pid
    }

    private func processBundleID(_ object: AudioObjectID) -> String? {
        var address = AudioObjectPropertyAddress(
            mSelector: kAudioProcessPropertyBundleID,
            mScope: kAudioObjectPropertyScopeGlobal,
            mElement: kAudioObjectPropertyElementMain
        )
        var value: Unmanaged<CFString>?
        var size = UInt32(MemoryLayout<Unmanaged<CFString>?>.size)
        guard AudioObjectGetPropertyData(object, &address, 0, nil, &size, &value) == noErr,
              let cf = value?.takeRetainedValue() else { return nil }
        return cf as String
    }

    func anyOtherProcessUsingMic() -> Bool {
        let ownPID = ProcessInfo.processInfo.processIdentifier
        var runningAddr = Self.processIsRunningInputAddress
        for object in audioProcessObjectList() {
            var isRunning: UInt32 = 0
            var size = UInt32(MemoryLayout<UInt32>.size)
            guard AudioObjectGetPropertyData(object, &runningAddr, 0, nil, &size, &isRunning) == noErr,
                  isRunning != 0 else { continue }
            guard let pid = processPID(object), pid != ownPID else { continue }
            if kill(pid, 0) != 0 && errno == ESRCH { continue }
            if let bundle = processBundleID(object),
               Self.micUsageSystemIgnoredBundleIDs.contains(bundle) || micUsageExcludedBundleIDs.contains(bundle) {
                continue
            }
            return true
        }
        return false
    }

    // MARK: - Mic Usage Monitor

    #if DEBUG
    private static let processListenerSelectors: [AudioObjectPropertySelector] = [
        kAudioProcessPropertyIsRunning,
        kAudioProcessPropertyDevices,
    ]

    private func startMicUsageListener() {
        stopMicUsageListener()
        let ptr = Unmanaged.passUnretained(self).toOpaque()
        var listAddr = Self.processObjectListAddress
        AudioObjectAddPropertyListener(AudioObjectID(kAudioObjectSystemObject), &listAddr, micUsageChanged, ptr)
        micUsageListeningProcessList = true
        refreshMicProcessListeners()
        micOtherAppActive = anyOtherProcessUsingMic()
        micUsagePollTimer = Timer.scheduledTimer(withTimeInterval: 5, repeats: true) { [weak self] _ in
            self?.evaluateMicUsageState()
        }
    }

    private func stopMicUsageListener() {
        micUsagePollTimer?.invalidate()
        micUsagePollTimer = nil
        let ptr = Unmanaged.passUnretained(self).toOpaque()
        if micUsageListeningProcessList {
            var listAddr = Self.processObjectListAddress
            AudioObjectRemovePropertyListener(AudioObjectID(kAudioObjectSystemObject), &listAddr, micUsageChanged, ptr)
            micUsageListeningProcessList = false
        }
        for object in micUsageProcessObjectIDs {
            for selector in Self.processListenerSelectors {
                var addr = AudioObjectPropertyAddress(
                    mSelector: selector,
                    mScope: kAudioObjectPropertyScopeGlobal,
                    mElement: kAudioObjectPropertyElementMain
                )
                AudioObjectRemovePropertyListener(object, &addr, micUsageChanged, ptr)
            }
        }
        micUsageProcessObjectIDs = []
    }

    private func refreshMicProcessListeners() {
        let ptr = Unmanaged.passUnretained(self).toOpaque()
        let current = Set(audioProcessObjectList())
        for object in micUsageProcessObjectIDs.subtracting(current) {
            for selector in Self.processListenerSelectors {
                var addr = AudioObjectPropertyAddress(
                    mSelector: selector,
                    mScope: kAudioObjectPropertyScopeGlobal,
                    mElement: kAudioObjectPropertyElementMain
                )
                AudioObjectRemovePropertyListener(object, &addr, micUsageChanged, ptr)
            }
        }
        for object in current.subtracting(micUsageProcessObjectIDs) {
            for selector in Self.processListenerSelectors {
                var addr = AudioObjectPropertyAddress(
                    mSelector: selector,
                    mScope: kAudioObjectPropertyScopeGlobal,
                    mElement: kAudioObjectPropertyElementMain
                )
                AudioObjectAddPropertyListener(object, &addr, micUsageChanged, ptr)
            }
        }
        micUsageProcessObjectIDs = current
    }

    func handleMicUsageChange() {
        DispatchQueue.main.asyncAfter(deadline: .now() + 1.0) { [weak self] in
            self?.evaluateMicUsageState()
        }
    }

    private func evaluateMicUsageState() {
        guard monitorMicUsage else { return }
        refreshMicProcessListeners()
        let isRunning = anyOtherProcessUsingMic()
        guard isRunning != micOtherAppActive else { return }
        micOtherAppActive = isRunning

        if isRunning && !isRecording {
            showMicStartAlert = true
            sendMicUsageNotification(started: true)
        } else if !isRunning && isRecording {
            showMicStopAlert = true
            sendMicUsageNotification(started: false)
        }
    }
    #endif

    // MARK: - Meeting Reminder

    private func startMeetingReminderMonitor() {
        stopMeetingReminderMonitor()
        Task {
            let granted = await calendarMonitor.requestAccess()
            guard granted else {
                await MainActor.run {
                    self.meetingReminderEnabled = false
                    self.errorMessage = String(localized: "カレンダーへのアクセスが許可されていません。システム設定 > プライバシーとセキュリティ > カレンダー で許可してください。")
                }
                return
            }
            await MainActor.run {
                self.meetingReminderTimer = Timer.scheduledTimer(withTimeInterval: 30, repeats: true) { [weak self] _ in
                    self?.checkMeetingReminder()
                }
                self.checkMeetingReminder()
            }
        }
    }

    private func stopMeetingReminderMonitor() {
        meetingReminderTimer?.invalidate()
        meetingReminderTimer = nil
    }

    private func checkMeetingReminder() {
        guard meetingReminderEnabled, !isRecording else { return }

        guard let event = calendarMonitor.findActiveMeetingEvent(
            minutesBefore: meetingReminderMinutesBefore,
            requireMeetingURL: meetingReminderRequireMeetingURL,
            calendarIdentifiers: meetingReminderCalendarIdentifiers ?? []
        ) else { return }

        guard !promptedEventIdentifiers.contains(event.identifier) else { return }

        if meetingReminderRequireMic {
            guard anyOtherProcessUsingMic() else { return }
        }

        promptedEventIdentifiers.insert(event.identifier)
        sendMeetingReminderNotification(eventTitle: event.title)
    }

    private static let meetingReminderCategoryId = "MEETING_REMINDER"
    #if DEBUG
    private static let micStartCategoryId = "MIC_USAGE_START"
    private static let micStopCategoryId = "MIC_USAGE_STOP"
    #endif
    private static let startRecordingActionId = "START_RECORDING"
    private static let stopRecordingActionId = "STOP_RECORDING"

    private static func registerNotificationCategories() {
        let startAction = UNNotificationAction(
            identifier: startRecordingActionId,
            title: String(localized: "録音開始"),
            options: .foreground
        )
        let stopAction = UNNotificationAction(
            identifier: stopRecordingActionId,
            title: String(localized: "録音停止"),
            options: .foreground
        )
        let meetingCategory = UNNotificationCategory(
            identifier: meetingReminderCategoryId,
            actions: [startAction],
            intentIdentifiers: []
        )
        #if DEBUG
        let micStartCategory = UNNotificationCategory(
            identifier: micStartCategoryId,
            actions: [startAction],
            intentIdentifiers: []
        )
        let micStopCategory = UNNotificationCategory(
            identifier: micStopCategoryId,
            actions: [stopAction],
            intentIdentifiers: []
        )
        UNUserNotificationCenter.current().setNotificationCategories([meetingCategory, micStartCategory, micStopCategory])
        #else
        UNUserNotificationCenter.current().setNotificationCategories([meetingCategory])
        #endif
    }

    func sendMeetingReminderNotification(eventTitle: String) {
        let content = UNMutableNotificationContent()
        content.title = String(localized: "会議リマインダー")
        content.body = String(localized: "「\(eventTitle)」の時間です。録音を開始しますか？")
        content.sound = .default
        content.interruptionLevel = .timeSensitive
        content.categoryIdentifier = Self.meetingReminderCategoryId
        let request = UNNotificationRequest(identifier: UUID().uuidString, content: content, trigger: nil)
        UNUserNotificationCenter.current().add(request)
    }

    #if DEBUG
    private func sendMicUsageNotification(started: Bool) {
        let content = UNMutableNotificationContent()
        if started {
            content.title = String(localized: "マイク使用を検知")
            content.body = String(localized: "他のアプリがマイクを使用し始めました。録音を開始しますか？")
            content.categoryIdentifier = Self.micStartCategoryId
        } else {
            content.title = String(localized: "マイク使用の終了を検知")
            content.body = String(localized: "他のアプリがマイクの使用を停止しました。録音を停止しますか？")
            content.categoryIdentifier = Self.micStopCategoryId
        }
        content.sound = .default
        content.interruptionLevel = .timeSensitive
        let request = UNNotificationRequest(identifier: UUID().uuidString, content: content, trigger: nil)
        UNUserNotificationCenter.current().add(request)
    }
    #endif

    private func defaultInputDeviceID() -> AudioDeviceID? {
        var address = AudioObjectPropertyAddress(
            mSelector: kAudioHardwarePropertyDefaultInputDevice,
            mScope: kAudioObjectPropertyScopeGlobal,
            mElement: kAudioObjectPropertyElementMain
        )
        var deviceID: AudioDeviceID = 0
        var size = UInt32(MemoryLayout<AudioDeviceID>.size)
        guard AudioObjectGetPropertyData(
            AudioObjectID(kAudioObjectSystemObject), &address, 0, nil, &size, &deviceID
        ) == noErr, deviceID != AudioObjectID(kAudioObjectUnknown) else { return nil }
        return deviceID
    }

    // MARK: - Recording

    func switchMicrophone(to deviceID: AudioDeviceID) {
        guard isRecording, let session = captureSession else { return }

        guard let deviceUID = inputDevices.first(where: { $0.id == deviceID })?.uid,
              let newDevice = AVCaptureDevice(uniqueID: deviceUID) else { return }

        session.beginConfiguration()
        for input in session.inputs {
            session.removeInput(input)
        }
        if let newInput = try? AVCaptureDeviceInput(device: newDevice),
           session.canAddInput(newInput) {
            session.addInput(newInput)
        }
        session.commitConfiguration()
    }

    func startRecording() async {
        guard !isRecording else { return }

        await tearDown()

        do {
            let micGranted = await AVCaptureDevice.requestAccess(for: .audio)
            guard micGranted else {
                await setError(String(localized: "マイクの使用が許可されていません。システム設定 > プライバシーとセキュリティ > マイク で許可してください。"))
                return
            }

            let speechAuthorized = liveTranscriptionEnabled ? await Transcriber.requestAuthorization() : false

            let baseDir = StorageLocation.baseDirectory
            let ts = Self.timestamp()
            let sessionDir = baseDir.appendingPathComponent(ts)
            try FileManager.default.createDirectory(at: sessionDir, withIntermediateDirectories: true)
            self.currentSessionFolder = sessionDir

            // ── Mic (AVCaptureSession → AVAssetWriter → .m4a) ──

            let micURL = sessionDir.appendingPathComponent("mic_\(ts).m4a")

            guard let deviceUID = inputDevices.first(where: { $0.id == selectedDeviceID })?.uid,
                  let captureDevice = AVCaptureDevice(uniqueID: deviceUID) else {
                await setError(String(localized: "選択したマイクが見つかりません"))
                return
            }

            let session = AVCaptureSession()
            let input = try AVCaptureDeviceInput(device: captureDevice)
            guard session.canAddInput(input) else {
                await setError(String(localized: "マイク入力を追加できません"))
                return
            }
            session.addInput(input)

            let audioOutput = AVCaptureAudioDataOutput()
            audioOutput.audioSettings = [
                AVFormatIDKey: Int(kAudioFormatLinearPCM),
                AVSampleRateKey: 48000,
                AVNumberOfChannelsKey: 1,
                AVLinearPCMBitDepthKey: 32,
                AVLinearPCMIsFloatKey: true,
                AVLinearPCMIsNonInterleaved: false,
            ]
            audioOutput.setSampleBufferDelegate(self, queue: micQueue)
            guard session.canAddOutput(audioOutput) else {
                await setError(String(localized: "音声出力を追加できません"))
                return
            }
            session.addOutput(audioOutput)

            let mWriter = try AVAssetWriter(outputURL: micURL, fileType: .m4a)
            let mInput = AVAssetWriterInput(
                mediaType: .audio,
                outputSettings: [
                    AVFormatIDKey: kAudioFormatMPEG4AAC,
                    AVSampleRateKey: 48000,
                    AVNumberOfChannelsKey: 1,
                    AVEncoderBitRateKey: 128_000,
                ]
            )
            mInput.expectsMediaDataInRealTime = true
            mWriter.add(mInput)
            guard mWriter.startWriting() else {
                await setError(String(localized: "マイク録音ファイル作成エラー: \(mWriter.error?.localizedDescription ?? "")"))
                return
            }
            self.micWriter = mWriter
            self.micWriterInput = mInput
            self.micSessionStarted = false
            self.captureSession = session

            session.startRunning()

            // ── System Audio (ScreenCaptureKit → AVAssetWriter → .m4a) ──

            let content = try await SCShareableContent.excludingDesktopWindows(false, onScreenWindowsOnly: false)
            guard let display = content.displays.first else {
                await tearDown()
                await setError(String(localized: "ディスプレイが見つかりません"))
                return
            }

            let sysURL = sessionDir.appendingPathComponent("system_\(ts).m4a")

            let writer = try AVAssetWriter(outputURL: sysURL, fileType: .m4a)
            let writerInput = AVAssetWriterInput(
                mediaType: .audio,
                outputSettings: [
                    AVFormatIDKey: kAudioFormatMPEG4AAC,
                    AVSampleRateKey: 48000,
                    AVNumberOfChannelsKey: 2,
                    AVEncoderBitRateKey: 128_000,
                ]
            )
            writerInput.expectsMediaDataInRealTime = true
            writer.add(writerInput)
            guard writer.startWriting() else {
                await tearDown()
                await setError(String(localized: "録音ファイル作成エラー: \(writer.error?.localizedDescription ?? "")"))
                return
            }
            self.sysWriter = writer
            self.sysWriterInput = writerInput
            self.sysSessionStarted = false

            let config = SCStreamConfiguration()
            config.capturesAudio = true
            config.excludesCurrentProcessAudio = true
            config.channelCount = 2
            config.sampleRate = 48000
            config.width = 2
            config.height = 2
            config.minimumFrameInterval = CMTime(value: 1, timescale: 1)
            config.queueDepth = 1

            let filter = SCContentFilter(display: display, excludingApplications: [], exceptingWindows: [])
            let scStream = SCStream(filter: filter, configuration: config, delegate: self)
            try scStream.addStreamOutput(self, type: .audio, sampleHandlerQueue: sysQueue)
            try scStream.addStreamOutput(self, type: .screen, sampleHandlerQueue: sysQueue)
            try await scStream.startCapture()
            self.stream = scStream

            // ── 開始 ──

            await MainActor.run {
                self.systemAudioURL = sysURL
                self.micAudioURL = micURL
                self.sysTranscript = ""
                self.micTranscript = ""
                self.mergedTranscript = []
                self.transcriptFileURL = nil
                self.isRecording = true
                self.errorMessage = nil
                HookRunner.fire(.recordingStarted, sessionFolder: sessionDir)
                self.silenceStartDate = nil
                self.silenceAutoStopTriggered = false
                self.micNoInputWarning = false
                self.micEverHadInput = false
                self.micNoInputAlertShown = false
                self.micInputTicks = 0
                self.recordingStart = Date()
                self.timer = Timer.scheduledTimer(withTimeInterval: 0.25, repeats: true) { [weak self] _ in
                    guard let self, let start = self.recordingStart else { return }
                    self.duration = Date().timeIntervalSince(start)
                    let newSys = self.rmsToLevel(self.peakSysLevel)
                    let newMic = self.rmsToLevel(self.peakMicLevel)
                    if abs(self.sysAudioLevel - newSys) > 0.01 {
                        self.sysAudioLevel = newSys
                    }
                    if abs(self.micAudioLevel - newMic) > 0.01 {
                        self.micAudioLevel = newMic
                    }
                    self.peakSysLevel = 0
                    self.peakMicLevel = 0

                    if let sys = self.sysStreamingTranscriber, let mic = self.micStreamingTranscriber {
                        self.liveTranscriptDiag = "sys: ap=\(sys.appendCount) res=\(sys.resultCount)(len\(sys.lastResultLength)) err=\(sys.errorCount) ses=\(sys.sessionCount) last=\(sys.lastError ?? "-") | mic: ap=\(mic.appendCount) res=\(mic.resultCount)(len\(mic.lastResultLength)) err=\(mic.errorCount) ses=\(mic.sessionCount) last=\(mic.lastError ?? "-")"
                    }

                    if !self.micEverHadInput {
                        if newMic > self.silenceLevelThreshold {
                            self.micInputTicks += 1
                            if self.micInputTicks >= self.micInputRequiredTicks {
                                self.micEverHadInput = true
                                self.micNoInputWarning = false
                            }
                        } else {
                            self.micInputTicks = 0
                            if self.duration >= self.micNoInputGracePeriod && !self.micNoInputAlertShown {
                                self.micNoInputAlertShown = true
                                self.micNoInputWarning = true
                                NSApp.requestUserAttention(.criticalRequest)
                            }
                        }
                    }

                    if self.silenceAutoStopEnabled && !self.silenceAutoStopTriggered {
                        let isSilent = newSys < self.silenceLevelThreshold && newMic < self.silenceLevelThreshold
                        if isSilent {
                            if self.silenceStartDate == nil {
                                self.silenceStartDate = Date()
                            } else if let silenceStart = self.silenceStartDate,
                                      Date().timeIntervalSince(silenceStart) >= TimeInterval(self.silenceAutoStopMinutes * 60) {
                                self.silenceAutoStopTriggered = true
                                Task { @MainActor [weak self] in
                                    guard let self else { return }
                                    await self.stopRecording()
                                    self.stoppedBySilenceDetection = true
                                    self.sendSilenceAutoStopNotification()
                                }
                            }
                        } else {
                            self.silenceStartDate = nil
                        }
                    }
                }
            }

            if speechAuthorized && liveTranscriptionEnabled {
                let locale = transcriptionLocale
                let sysTx = StreamingTranscriber(locale: locale)
                let micTx = StreamingTranscriber(locale: locale)
                let recognizerStatus = sysTx.diagnosticStatus
                sysStreamingTranscriber = sysTx
                micStreamingTranscriber = micTx
                let elapsed: () -> TimeInterval = { [weak self] in self?.duration ?? 0 }
                sysTx.start(timeProvider: elapsed) { [weak self] text in
                    self?.sysTranscript = text
                }
                micTx.start(timeProvider: elapsed) { [weak self] text in
                    self?.micTranscript = text
                }
                await MainActor.run {
                    self.liveTranscriptDiag = "locale=\(locale.identifier) \(recognizerStatus)"
                }
            } else if liveTranscriptionEnabled {
                await MainActor.run {
                    self.liveTranscriptDiag = "speechAuthorized=false"
                    self.errorMessage = String(localized: "音声認識が許可されていないため、リアルタイム文字起こしは無効です。システム設定 > プライバシーとセキュリティ > 音声認識 で許可してください。")
                }
            }

        } catch {
            await tearDown()
            await setError(error.localizedDescription)
        }
    }

    func stopRecording() async {
        let preSys = await MainActor.run { sysTranscript }
        let preMic = await MainActor.run { micTranscript }

        let sysSegments = sysStreamingTranscriber?.segments ?? []
        let micSegments = micStreamingTranscriber?.segments ?? []

        await sysStreamingTranscriber?.stop()
        await micStreamingTranscriber?.stop()
        sysStreamingTranscriber = nil
        micStreamingTranscriber = nil

        await MainActor.run {
            timer?.invalidate()
            timer = nil
        }

        await tearDown()

        let needsTranscription: Bool = await MainActor.run {
            isRecording = false
            sysAudioLevel = 0
            micAudioLevel = 0
            if sysTranscript.count < preSys.count { sysTranscript = preSys }
            if micTranscript.count < preMic.count { micTranscript = preMic }
            if !sysSegments.isEmpty || !micSegments.isEmpty {
                mergedTranscript = Self.mergeSegments(system: sysSegments, mic: micSegments)
            }
            saveLiveTranscript()
            saveSessionMetadata()
            if let folder = currentSessionFolder {
                HookRunner.fire(.recordingStopped, sessionFolder: folder)
            }

            let liveInsufficient = liveTranscriptionEnabled && sysTranscript.count + micTranscript.count < 10
            let shouldTranscribe = autoTranscribeEnabled || liveInsufficient
            if shouldTranscribe {
                isTranscribing = true
                autoTranscribeSessionId = currentSessionFolder?.lastPathComponent
            }
            return shouldTranscribe
        }

        if needsTranscription {
            autoTranscribeTask = Task { [weak self] in
                await self?.autoTranscribeRecordedFiles()
            }
        }
    }

    func cancelAutoTranscription() {
        currentAutoTranscriber?.cancel()
        autoTranscribeTask?.cancel()
        autoTranscribeTask = nil
        currentAutoTranscriber = nil
        isTranscribing = false
        autoTranscribeSessionId = nil
        autoTranscribePhase = nil
        autoTranscribeProgress = nil
    }

    @MainActor
    func startTranscribeSession(folder: URL, systemAudio: URL?, micAudio: URL?, engine: TranscriptionEngineType?) -> Bool {
        guard !isRecording, autoTranscribeSessionId == nil else { return false }
        currentSessionFolder = folder
        systemAudioURL = systemAudio
        micAudioURL = micAudio
        sysTranscript = ""
        micTranscript = ""
        mergedTranscript = []
        transcriptFileURL = nil
        isTranscribing = true
        autoTranscribeSessionId = folder.lastPathComponent
        autoTranscribeTask = Task { [weak self] in
            await self?.autoTranscribeRecordedFiles(engineOverride: engine)
        }
        return true
    }

    private func autoTranscribeRecordedFiles(engineOverride: TranscriptionEngineType? = nil) async {
        let sysURL = await MainActor.run { systemAudioURL }
        let micURL = await MainActor.run { micAudioURL }
        #if DEBUG
        let diarize = await MainActor.run { diarizationEnabled && !liveTranscriptionEnabled }
        #endif
        let engine = await MainActor.run { engineOverride ?? autoTranscribeEngine }
        let modelPath = await MainActor.run { whisperModelManager?.currentModelPath }

        var sysSegments: [TimedSegment] = []
        var micSegments: [TimedSegment] = []
        var sysText = ""
        var micText = ""

        let useWhisper = engine == .whisper && modelPath != nil

        for (url, isSys) in [(sysURL, true), (micURL, false)] {
            guard let url else { continue }
            await MainActor.run {
                autoTranscribePhase = isSys ? String(localized: "システム音声") : String(localized: "マイク")
                autoTranscribeProgress = 0
            }

            if useWhisper {
                let lang = transcriptionLocale.language.languageCode?.identifier ?? "ja"
                let result = await whisperTranscriber.transcribe(url: url, modelPath: modelPath!, language: lang, onUpdate: { [weak self] text in
                    DispatchQueue.main.async {
                        if isSys { self?.sysTranscript = text }
                        else { self?.micTranscript = text }
                    }
                }, onProgress: { [weak self] progress in
                    DispatchQueue.main.async { self?.autoTranscribeProgress = progress }
                }, isCancelled: { Task.isCancelled })
                if Task.isCancelled { return }
                if isSys { sysSegments = result.segments; sysText = result.text }
                else { micSegments = result.segments; micText = result.text }
            } else {
                let tx = Transcriber(locale: transcriptionLocale)
                currentAutoTranscriber = tx
                let result = await tx.transcribe(url: url, onUpdate: { [weak self] text in
                    DispatchQueue.main.async {
                        if isSys { self?.sysTranscript = text }
                        else { self?.micTranscript = text }
                    }
                }, onProgress: { [weak self] progress in
                    DispatchQueue.main.async { self?.autoTranscribeProgress = progress }
                })
                if Task.isCancelled { return }
                if isSys { sysSegments = result.segments; sysText = result.text }
                else { micSegments = result.segments; micText = result.text }
            }
        }

        currentAutoTranscriber = nil
        let filteredMic = Self.deduplicateMicEcho(system: sysSegments, mic: micSegments)
        var merged: [TranscriptEntry]

        #if DEBUG
        if diarize {
            await MainActor.run {
                isDiarizing = true
                autoTranscribePhase = String(localized: "話者識別")
                autoTranscribeProgress = nil
            }
            let allSegments = sysSegments + filteredMic
            if let audioURL = sysURL ?? micURL,
               let diarization = try? await speakerDiarizer.diarize(url: audioURL) {
                let labeled = Self.assignSpeakers(to: allSegments, using: diarization)
                merged = Self.mergeSegmentsWithSpeakers(labeled)
            } else {
                merged = Self.mergeSegments(system: sysSegments, mic: filteredMic)
            }
            await MainActor.run { isDiarizing = false }
        } else {
            merged = Self.mergeSegments(system: sysSegments, mic: filteredMic)
        }
        #else
        merged = Self.mergeSegments(system: sysSegments, mic: filteredMic)
        #endif

        await MainActor.run {
            let hasNewContent = !sysText.isEmpty || !micText.isEmpty || !merged.isEmpty
            if hasNewContent {
                sysTranscript = sysText
                micTranscript = micText
                mergedTranscript = merged
                saveLiveTranscript()
            }
            saveSessionMetadata()
            isTranscribing = false
            autoTranscribePhase = String(localized: "概要生成")
        }

        if let translationService {
            let translated = await translationService.translateEntries(merged)
            await MainActor.run { mergedTranscript = translated }
        }

        let transcriptText = await MainActor.run { readTranscriptText() }
        let summary = await SummaryGenerator.generate(from: transcriptText ?? "")

        await MainActor.run {
            if let summary, let folder = currentSessionFolder {
                updateSummaryInMetadata(folder: folder, summary: summary)
            }
            if let folder = currentSessionFolder {
                HookRunner.fire(.transcriptionCompleted, sessionFolder: folder)
            }
            autoTranscribeSessionId = nil
            autoTranscribePhase = nil
            autoTranscribeProgress = nil
        }
    }

    private func saveLiveTranscript() {
        guard let folder = currentSessionFolder,
              !sysTranscript.isEmpty || !micTranscript.isEmpty || !mergedTranscript.isEmpty else { return }

        let text: String
        if !mergedTranscript.isEmpty {
            text = TranscriptResultView.buildText(entries: mergedTranscript)
        } else {
            let totalDuration = duration
            var entries: [TranscriptEntry] = []
            for (source, transcript) in [(TranscriptSource.system, sysTranscript), (.mic, micTranscript)] {
                guard !transcript.isEmpty else { continue }
                let sentences = Self.splitIntoSentences(transcript)
                let totalChars = transcript.count
                var charOffset = 0
                for sentence in sentences {
                    let ratio = totalChars > 0 ? Double(charOffset) / Double(totalChars) : 0
                    let startTime = totalDuration * ratio
                    entries.append(TranscriptEntry(source: source, text: sentence, startTime: startTime))
                    charOffset += sentence.count
                }
            }
            entries.sort { $0.startTime < $1.startTime }
            text = TranscriptResultView.buildText(entries: entries)
        }
        let ts = folder.lastPathComponent
        let filename = "transcript_\(ts).txt"
        try? text.write(to: folder.appendingPathComponent(filename), atomically: true, encoding: .utf8)
        transcriptFileURL = folder.appendingPathComponent(filename)
    }

    private func saveSessionMetadata() {
        guard let folder = currentSessionFolder else { return }
        let metadataURL = folder.appendingPathComponent("session.json")
        let decoder = JSONDecoder()
        decoder.dateDecodingStrategy = .iso8601
        let existing = (try? Data(contentsOf: metadataURL)).flatMap { try? decoder.decode(SessionMetadata.self, from: $0) }

        let transcriptFilename = transcriptFileURL?.lastPathComponent ?? existing?.transcriptFile
        let metadata = SessionMetadata(
            createdAt: recordingStart ?? existing?.createdAt ?? Date(),
            systemAudioFile: systemAudioURL?.lastPathComponent ?? existing?.systemAudioFile,
            micAudioFile: micAudioURL?.lastPathComponent ?? existing?.micAudioFile,
            transcriptFile: transcriptFilename,
            engineType: transcriptFilename != nil ? (existing?.engineType ?? "apple") : nil,
            modelId: existing?.modelId,
            summary: existing?.summary,
            localeIdentifier: existing?.localeIdentifier ?? transcriptionLocale.identifier
        )
        let encoder = JSONEncoder()
        encoder.dateEncodingStrategy = .iso8601
        encoder.outputFormatting = .prettyPrinted
        guard let data = try? encoder.encode(metadata) else { return }
        try? data.write(to: metadataURL, options: .atomic)
    }

    private func readTranscriptText() -> String? {
        guard let url = transcriptFileURL else { return nil }
        return try? String(contentsOf: url, encoding: .utf8)
    }

    private func updateSummaryInMetadata(folder: URL, summary: String) {
        let metadataURL = folder.appendingPathComponent("session.json")
        let decoder = JSONDecoder()
        decoder.dateDecodingStrategy = .iso8601
        guard let data = try? Data(contentsOf: metadataURL),
              var metadata = try? decoder.decode(SessionMetadata.self, from: data) else { return }
        metadata.summary = summary
        let encoder = JSONEncoder()
        encoder.dateEncodingStrategy = .iso8601
        encoder.outputFormatting = .prettyPrinted
        guard let updated = try? encoder.encode(metadata) else { return }
        try? updated.write(to: metadataURL, options: .atomic)
    }

    private func tearDown() async {
        if let s = stream {
            try? await s.stopCapture()
            stream = nil
        }

        if let writer = sysWriter {
            sysWriterInput?.markAsFinished()
            await writer.finishWriting()
            sysWriter = nil
            sysWriterInput = nil
        }

        if let session = captureSession {
            session.stopRunning()
            captureSession = nil
        }

        if let writer = micWriter {
            micWriterInput?.markAsFinished()
            await writer.finishWriting()
            micWriter = nil
            micWriterInput = nil
        }
    }

    @MainActor
    private func setError(_ msg: String) {
        errorMessage = msg
    }

    private func autoSaveTranscript() -> URL? {
        guard let sysURL = systemAudioURL, !mergedTranscript.isEmpty else { return nil }
        let dir = sysURL.deletingLastPathComponent()
        let name = sysURL.deletingPathExtension().lastPathComponent
            .replacingOccurrences(of: "system_", with: "transcript_")
        let txtURL = dir.appendingPathComponent("\(name).txt")
        let content = TranscriptResultView.buildText(entries: mergedTranscript)
        try? content.write(to: txtURL, atomically: true, encoding: .utf8)
        return txtURL
    }

    static func deduplicateMicEcho(system: [TimedSegment], mic: [TimedSegment]) -> [TimedSegment] {
        guard !system.isEmpty else { return mic }
        return mic.filter { m in
            let duration = m.end - m.start
            guard duration > 0 else { return true }
            var totalOverlap: TimeInterval = 0
            for s in system {
                let overlapStart = max(m.start, s.start)
                let overlapEnd = min(m.end, s.end)
                totalOverlap += max(0, overlapEnd - overlapStart)
            }
            return totalOverlap / duration < 0.5
        }
    }

    #if DEBUG
    static func assignSpeakers(to segments: [TimedSegment], using diarization: [SpeakerTimedSegment]) -> [TimedSegment] {
        segments.map { seg in
            var best: SpeakerTimedSegment?
            var bestOverlap: TimeInterval = 0
            for d in diarization {
                let overlapStart = max(seg.start, d.start)
                let overlapEnd = min(seg.end, d.end)
                let overlap = max(0, overlapEnd - overlapStart)
                if overlap > bestOverlap {
                    bestOverlap = overlap
                    best = d
                }
            }
            var result = seg
            result.speakerId = best?.speakerId
            return result
        }
    }

    static func mergeSegmentsWithSpeakers(_ segments: [TimedSegment]) -> [TranscriptEntry] {
        guard !segments.isEmpty else { return [] }

        let pauseThreshold: TimeInterval = 3.0
        let sorted = segments.sorted { $0.start < $1.start }
        var entries: [TranscriptEntry] = []
        var currentSpeaker = sorted[0].speakerId
        var currentTexts = [sorted[0].text]
        var currentStart = sorted[0].start
        var currentEnd = sorted[0].end

        for i in 1..<sorted.count {
            let seg = sorted[i]
            let pause = seg.start - currentEnd
            if seg.speakerId == currentSpeaker && pause < pauseThreshold {
                currentTexts.append(seg.text)
                currentEnd = seg.end
            } else {
                entries.append(TranscriptEntry(
                    source: .system,
                    text: joinSegmentTexts(currentTexts),
                    startTime: currentStart,
                    speakerId: currentSpeaker
                ))
                currentSpeaker = seg.speakerId
                currentTexts = [seg.text]
                currentStart = seg.start
                currentEnd = seg.end
            }
        }
        entries.append(TranscriptEntry(
            source: .system,
            text: joinSegmentTexts(currentTexts),
            startTime: currentStart,
            speakerId: currentSpeaker
        ))

        return entries
    }
    #endif

    static func mergeSegments(system: [TimedSegment], mic: [TimedSegment]) -> [TranscriptEntry] {
        struct Tagged {
            let source: TranscriptSource
            let text: String
            let start: TimeInterval
            let end: TimeInterval
        }

        let pauseThreshold: TimeInterval = 5.0

        var all: [Tagged] = []
        for s in system { all.append(Tagged(source: .system, text: s.text, start: s.start, end: s.end)) }
        for s in mic { all.append(Tagged(source: .mic, text: s.text, start: s.start, end: s.end)) }
        all.sort { $0.start < $1.start }

        guard !all.isEmpty else { return [] }

        var entries: [TranscriptEntry] = []
        var currentSource = all[0].source
        var currentTexts = [all[0].text]
        var currentStart = all[0].start
        var currentEnd = all[0].end

        for i in 1..<all.count {
            let seg = all[i]
            let pauseGap = seg.start - currentEnd
            if seg.source == currentSource && pauseGap < pauseThreshold {
                currentTexts.append(seg.text)
                currentEnd = seg.end
            } else {
                entries.append(TranscriptEntry(
                    source: currentSource,
                    text: joinSegmentTexts(currentTexts),
                    startTime: currentStart
                ))
                currentSource = seg.source
                currentTexts = [seg.text]
                currentStart = seg.start
                currentEnd = seg.end
            }
        }
        entries.append(TranscriptEntry(
            source: currentSource,
            text: joinSegmentTexts(currentTexts),
            startTime: currentStart
        ))

        return entries
    }

    static func splitIntoSentences(_ text: String) -> [String] {
        let endings: [Character] = ["。", "？", "！", ".", "?", "!"]
        var sentences: [String] = []
        var current = ""
        var sentenceCount = 0
        let groupSize = 3
        let maxChars = 250

        for char in text {
            current.append(char)
            if endings.contains(char) {
                sentenceCount += 1
                if sentenceCount >= groupSize {
                    sentences.append(current)
                    current = ""
                    sentenceCount = 0
                }
            } else if current.count >= maxChars {
                let breakChars: [Character] = ["、", ",", " ", "　"]
                if let lastBreak = current.lastIndex(where: { breakChars.contains($0) }) {
                    let afterBreak = current.index(after: lastBreak)
                    sentences.append(String(current[...lastBreak]))
                    current = String(current[afterBreak...])
                } else {
                    sentences.append(current)
                    current = ""
                }
                sentenceCount = 0
            }
        }
        if !current.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            sentences.append(current)
        }
        return sentences.isEmpty ? [text] : sentences
    }

    private static func timestamp() -> String {
        let f = DateFormatter()
        f.dateFormat = "yyyyMMdd_HHmmss"
        return f.string(from: Date())
    }

    private func calcRMS(_ sampleBuffer: CMSampleBuffer) -> Float {
        guard let dataBuffer = CMSampleBufferGetDataBuffer(sampleBuffer),
              let formatDesc = CMSampleBufferGetFormatDescription(sampleBuffer),
              let asbd = CMAudioFormatDescriptionGetStreamBasicDescription(formatDesc)?.pointee else { return 0 }

        var length = 0
        var rawPtr: UnsafeMutablePointer<Int8>?
        guard CMBlockBufferGetDataPointer(dataBuffer, atOffset: 0, lengthAtOffsetOut: nil, totalLengthOut: &length, dataPointerOut: &rawPtr) == noErr,
              let ptr = rawPtr else { return 0 }

        let isFloat = asbd.mFormatFlags & kAudioFormatFlagIsFloat != 0

        if isFloat && asbd.mBitsPerChannel == 32 {
            let count = length / MemoryLayout<Float>.size
            var rms: Float = 0
            ptr.withMemoryRebound(to: Float.self, capacity: count) { floats in
                vDSP_rmsqv(floats, 1, &rms, vDSP_Length(count))
            }
            return rms
        } else if !isFloat && asbd.mBitsPerChannel == 16 {
            let count = length / MemoryLayout<Int16>.size
            return ptr.withMemoryRebound(to: Int16.self, capacity: count) { samples in
                var floats = [Float](repeating: 0, count: count)
                vDSP_vflt16(samples, 1, &floats, 1, vDSP_Length(count))
                var scale: Float = 1.0 / Float(Int16.max)
                vDSP_vsmul(floats, 1, &scale, &floats, 1, vDSP_Length(count))
                var rms: Float = 0
                vDSP_rmsqv(floats, 1, &rms, vDSP_Length(count))
                return rms
            }
        }
        return 0
    }

    private func rmsToLevel(_ rms: Float) -> Float {
        guard rms > 0 else { return 0 }
        let db = 20 * log10(rms)
        let minDb: Float = -50
        return max(0, min(1, (db - minDb) / -minDb))
    }

    // MARK: - Silence Auto-Stop Notification

    static func requestNotificationPermission() {
        UNUserNotificationCenter.current().requestAuthorization(options: [.alert, .sound]) { _, _ in }
    }

    private func sendSilenceAutoStopNotification() {
        let content = UNMutableNotificationContent()
        content.title = String(localized: "録音を自動停止しました")
        content.body = String(localized: "\(silenceAutoStopMinutes)分間音声が検出されなかったため、録音を停止しました。")
        content.sound = .default
        let request = UNNotificationRequest(identifier: UUID().uuidString, content: content, trigger: nil)
        UNUserNotificationCenter.current().add(request)
    }
}

// MARK: - System Audio (SCStreamOutput)

extension AudioCaptureManager: SCStreamOutput {
    func stream(_ stream: SCStream, didOutputSampleBuffer sampleBuffer: CMSampleBuffer, of type: SCStreamOutputType) {
        guard type == .audio,
              CMSampleBufferDataIsReady(sampleBuffer),
              CMSampleBufferGetNumSamples(sampleBuffer) > 0,
              let input = sysWriterInput,
              let writer = sysWriter,
              writer.status == .writing else { return }

        if !sysSessionStarted {
            writer.startSession(atSourceTime: CMSampleBufferGetPresentationTimeStamp(sampleBuffer))
            sysSessionStarted = true
        }

        if input.isReadyForMoreMediaData {
            input.append(sampleBuffer)
        }

        peakSysLevel = max(peakSysLevel, calcRMS(sampleBuffer))
        sysStreamingTranscriber?.append(sampleBuffer)
    }
}

// MARK: - Mic Audio (AVCaptureAudioDataOutputSampleBufferDelegate)

extension AudioCaptureManager: AVCaptureAudioDataOutputSampleBufferDelegate {
    func captureOutput(_ output: AVCaptureOutput, didOutput sampleBuffer: CMSampleBuffer, from connection: AVCaptureConnection) {
        guard CMSampleBufferDataIsReady(sampleBuffer),
              CMSampleBufferGetNumSamples(sampleBuffer) > 0,
              let input = micWriterInput,
              let writer = micWriter,
              writer.status == .writing else { return }

        amplifySampleBuffer(sampleBuffer)

        if !micSessionStarted {
            writer.startSession(atSourceTime: CMSampleBufferGetPresentationTimeStamp(sampleBuffer))
            micSessionStarted = true
        }

        if input.isReadyForMoreMediaData {
            input.append(sampleBuffer)
        }

        peakMicLevel = max(peakMicLevel, calcRMS(sampleBuffer))
        micStreamingTranscriber?.append(sampleBuffer)
    }

    private func amplifySampleBuffer(_ sampleBuffer: CMSampleBuffer) {
        guard let formatDesc = CMSampleBufferGetFormatDescription(sampleBuffer),
              let asbd = CMAudioFormatDescriptionGetStreamBasicDescription(formatDesc)?.pointee,
              asbd.mFormatID == kAudioFormatLinearPCM,
              let dataBuffer = CMSampleBufferGetDataBuffer(sampleBuffer) else { return }

        var length = 0
        var rawPtr: UnsafeMutablePointer<Int8>?
        guard CMBlockBufferGetDataPointer(dataBuffer, atOffset: 0, lengthAtOffsetOut: nil, totalLengthOut: &length, dataPointerOut: &rawPtr) == noErr,
              let ptr = rawPtr else { return }

        var gain = micGain
        let isFloat = asbd.mFormatFlags & kAudioFormatFlagIsFloat != 0

        if isFloat && asbd.mBitsPerChannel == 32 {
            let count = length / MemoryLayout<Float>.size
            ptr.withMemoryRebound(to: Float.self, capacity: count) { floats in
                vDSP_vsmul(floats, 1, &gain, floats, 1, vDSP_Length(count))
            }
        } else if !isFloat && asbd.mBitsPerChannel == 16 {
            let count = length / MemoryLayout<Int16>.size
            ptr.withMemoryRebound(to: Int16.self, capacity: count) { samples in
                for i in 0..<count {
                    let amplified = min(max(Int32(samples[i]) * Int32(gain), Int32(Int16.min)), Int32(Int16.max))
                    samples[i] = Int16(amplified)
                }
            }
        }
    }
}

// MARK: - SCStreamDelegate

extension AudioCaptureManager: SCStreamDelegate {
    func stream(_ stream: SCStream, didStopWithError error: Error) {
        Task { await setError(String(localized: "システム音声キャプチャエラー: \(error.localizedDescription)")) }
    }
}

// MARK: - Notification Delegate

extension AudioCaptureManager: UNUserNotificationCenterDelegate {
    func userNotificationCenter(_ center: UNUserNotificationCenter,
                                didReceive response: UNNotificationResponse,
                                withCompletionHandler completionHandler: @escaping () -> Void) {
        let category = response.notification.request.content.categoryIdentifier
        var isKnownCategory = category == Self.meetingReminderCategoryId
        #if DEBUG
        isKnownCategory = isKnownCategory
            || category == Self.micStartCategoryId
            || category == Self.micStopCategoryId
        #endif
        guard isKnownCategory else {
            completionHandler()
            return
        }

        DispatchQueue.main.async { [weak self] in
            NSApp.activate(ignoringOtherApps: true)
            for window in NSApp.windows where window.canBecomeMain {
                window.makeKeyAndOrderFront(nil)
                break
            }

            guard let self else {
                completionHandler()
                return
            }
            if response.actionIdentifier == Self.startRecordingActionId {
                self.showMicStartAlert = false
                Task { await self.startRecording() }
            } else if response.actionIdentifier == Self.stopRecordingActionId {
                self.showMicStopAlert = false
                Task { await self.stopRecording() }
            }
            completionHandler()
        }
    }

    func userNotificationCenter(_ center: UNUserNotificationCenter,
                                willPresent notification: UNNotification,
                                withCompletionHandler completionHandler: @escaping (UNNotificationPresentationOptions) -> Void) {
        completionHandler([.banner, .sound, .list])
    }
}

// MARK: - CoreAudio Device Change Listener

private func audioDeviceListChanged(
    _ objectID: AudioObjectID,
    _ numberAddresses: UInt32,
    _ addresses: UnsafePointer<AudioObjectPropertyAddress>,
    _ clientData: UnsafeMutableRawPointer?
) -> OSStatus {
    guard let clientData else { return noErr }
    let manager = Unmanaged<AudioCaptureManager>.fromOpaque(clientData).takeUnretainedValue()
    manager.scheduleDeviceRefresh()
    return noErr
}

private func defaultInputDeviceChanged(
    _ objectID: AudioObjectID,
    _ numberAddresses: UInt32,
    _ addresses: UnsafePointer<AudioObjectPropertyAddress>,
    _ clientData: UnsafeMutableRawPointer?
) -> OSStatus {
    guard let clientData else { return noErr }
    let manager = Unmanaged<AudioCaptureManager>.fromOpaque(clientData).takeUnretainedValue()
    manager.handleDefaultInputDeviceChanged()
    return noErr
}

#if DEBUG
private func micUsageChanged(
    _ objectID: AudioObjectID,
    _ numberAddresses: UInt32,
    _ addresses: UnsafePointer<AudioObjectPropertyAddress>,
    _ clientData: UnsafeMutableRawPointer?
) -> OSStatus {
    guard let clientData else { return noErr }
    let manager = Unmanaged<AudioCaptureManager>.fromOpaque(clientData).takeUnretainedValue()
    manager.handleMicUsageChange()
    return noErr
}
#endif
