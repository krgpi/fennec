import AppKit
import AVFoundation
import Foundation

private struct CommandError: Error {
    let message: String
}

final class CommandServer {
    static private(set) var shared: CommandServer?

    static func start(manager: AudioCaptureManager, whisperManager: WhisperModelManager, recordingStore: RecordingStore) {
        guard shared == nil else { return }
        let server = CommandServer(manager: manager, whisperManager: whisperManager, recordingStore: recordingStore)
        server.listen()
        shared = server
    }

    private let manager: AudioCaptureManager
    private let whisperManager: WhisperModelManager
    private let recordingStore: RecordingStore
    private var listenFD: Int32 = -1

    private init(manager: AudioCaptureManager, whisperManager: WhisperModelManager, recordingStore: RecordingStore) {
        self.manager = manager
        self.whisperManager = whisperManager
        self.recordingStore = recordingStore
    }

    deinit {
        if listenFD >= 0 { close(listenFD) }
    }

    private func listen() {
        let path = FennecIPC.socketPath
        try? FileManager.default.createDirectory(
            atPath: (path as NSString).deletingLastPathComponent,
            withIntermediateDirectories: true
        )
        unlink(path)

        let fd = socket(AF_UNIX, SOCK_STREAM, 0)
        guard fd >= 0 else { return }

        var addr = sockaddr_un()
        addr.sun_family = sa_family_t(AF_UNIX)
        let pathBytes = path.utf8CString
        guard pathBytes.count <= MemoryLayout.size(ofValue: addr.sun_path) else {
            close(fd)
            return
        }
        withUnsafeMutableBytes(of: &addr.sun_path) { dest in
            pathBytes.withUnsafeBytes { src in
                dest.copyBytes(from: src)
            }
        }
        let size = socklen_t(MemoryLayout<sockaddr_un>.size)
        let bindResult = withUnsafePointer(to: &addr) { ptr in
            ptr.withMemoryRebound(to: sockaddr.self, capacity: 1) { sa in
                bind(fd, sa, size)
            }
        }
        guard bindResult == 0, Darwin.listen(fd, 8) == 0 else {
            close(fd)
            return
        }
        chmod(path, 0o600)
        listenFD = fd

        Thread.detachNewThread { [weak self] in
            self?.acceptLoop()
        }
    }

    private func acceptLoop() {
        while listenFD >= 0 {
            let clientFD = accept(listenFD, nil, nil)
            guard clientFD >= 0 else {
                if errno == EINTR { continue }
                break
            }
            DispatchQueue.global(qos: .userInitiated).async { [weak self] in
                self?.serve(clientFD)
            }
        }
    }

    private func serve(_ fd: Int32) {
        defer { close(fd) }
        guard let request = readRequestLine(fd) else { return }

        let send: ([String: Any]) -> Void = { object in
            guard let data = FennecIPC.encodeLine(object) else { return }
            data.withUnsafeBytes { buf in
                var offset = 0
                while offset < buf.count {
                    let written = write(fd, buf.baseAddress!.advanced(by: offset), buf.count - offset)
                    if written <= 0 { return }
                    offset += written
                }
            }
        }

        do {
            let command = request["command"] as? String ?? ""
            let args = request["args"] as? [String: String] ?? [:]
            let data = try handle(command: command, args: args, send: send)
            var response: [String: Any] = ["ok": true]
            if let data { response["data"] = data }
            send(response)
        } catch let error as CommandError {
            send(["ok": false, "error": error.message])
        } catch {
            send(["ok": false, "error": error.localizedDescription])
        }
    }

    private func readRequestLine(_ fd: Int32) -> [String: Any]? {
        var buffer = Data()
        var chunk = [UInt8](repeating: 0, count: 4096)
        while buffer.count < 1_048_576 {
            let count = read(fd, &chunk, chunk.count)
            guard count > 0 else { return nil }
            buffer.append(contentsOf: chunk[0..<count])
            if let newline = buffer.firstIndex(of: 0x0A) {
                let line = buffer.prefix(upTo: newline)
                return (try? JSONSerialization.jsonObject(with: line)) as? [String: Any]
            }
        }
        return nil
    }

    private final class ResultBox<T> {
        var value: T?
    }

    private func onMain<T>(_ body: @escaping @MainActor () async -> T) -> T {
        let box = ResultBox<T>()
        let sem = DispatchSemaphore(value: 0)
        Task { @MainActor in
            box.value = await body()
            sem.signal()
        }
        sem.wait()
        return box.value!
    }

    // MARK: - Dispatch

    private func handle(command: String, args: [String: String], send: ([String: Any]) -> Void) throws -> [String: Any]? {
        switch command {
        case "status": return handleStatus()
        case "record.start": return try handleRecordStart()
        case "record.stop": return try handleRecordStop()
        case "record.status": return handleStatus()
        case "sessions.list": return handleSessionsList()
        case "sessions.show": return try handleSessionsShow(args)
        case "sessions.delete": return try handleSessionsDelete(args)
        case "transcribe": return try handleTranscribe(args, send: send)
        case "minutes": return try handleMinutes(args, send: send)
        case "preset.list": return handlePresetList()
        case "preset.show": return try handlePresetShow(args)
        case "preset.create": return try handlePresetCreate(args)
        case "preset.delete": return try handlePresetDelete(args)
        case "config.list": return handleConfigList()
        case "config.get": return try handleConfigGet(args)
        case "config.set": return try handleConfigSet(args)
        case "model.list": return handleModelList()
        case "model.download": return try handleModelDownload(args, send: send)
        case "hook.list": return handleHookList()
        case "hook.add": return try handleHookAdd(args)
        case "hook.set-enabled": return try handleHookSetEnabled(args)
        case "hook.delete": return try handleHookDelete(args)
        default:
            throw CommandError(message: "unknown command: \(command)")
        }
    }

    // MARK: - Status / Record

    private func handleStatus() -> [String: Any] {
        onMain {
            var result: [String: Any] = [
                "running": true,
                "recording": self.manager.isRecording,
                "transcribing": self.manager.autoTranscribeSessionId != nil,
            ]
            if self.manager.isRecording {
                result["duration"] = self.manager.duration
            }
            if let folder = self.manager.currentSessionFolder, self.manager.isRecording {
                result["sessionId"] = folder.lastPathComponent
            }
            if let id = self.manager.autoTranscribeSessionId {
                result["transcribingSessionId"] = id
            }
            return result
        }
    }

    private func handleRecordStart() throws -> [String: Any] {
        let error: String? = onMain {
            guard !self.manager.isRecording else { return String(localized: "すでに録音中です") }
            guard self.manager.selectedDeviceID != nil else { return String(localized: "マイクが選択されていません") }
            await self.manager.startRecording()
            if !self.manager.isRecording {
                return self.manager.errorMessage ?? String(localized: "録音を開始できませんでした")
            }
            return nil
        }
        if let error { throw CommandError(message: error) }
        return onMain {
            ["sessionId": self.manager.currentSessionFolder?.lastPathComponent ?? ""]
        }
    }

    private func handleRecordStop() throws -> [String: Any] {
        let result: (String?, String?) = onMain {
            guard self.manager.isRecording else { return (nil, String(localized: "録音していません")) }
            let sessionId = self.manager.currentSessionFolder?.lastPathComponent
            await self.manager.stopRecording()
            self.recordingStore.loadSessions()
            return (sessionId, nil)
        }
        if let error = result.1 { throw CommandError(message: error) }
        return ["sessionId": result.0 ?? ""]
    }

    // MARK: - Sessions

    private func loadedSessions() -> [RecordingSession] {
        onMain {
            self.recordingStore.loadSessions()
            return self.recordingStore.sessions
        }
    }

    private func findSession(_ args: [String: String]) throws -> RecordingSession {
        guard let id = args["sessionId"], !id.isEmpty else {
            throw CommandError(message: "sessionId is required")
        }
        let sessions = loadedSessions()
        if let session = sessions.first(where: { $0.id == id }) {
            return session
        }
        if id == "latest", let latest = sessions.first {
            return latest
        }
        throw CommandError(message: "session not found: \(id)")
    }

    private func sessionDict(_ session: RecordingSession) -> [String: Any] {
        var dict: [String: Any] = [
            "id": session.id,
            "createdAt": ISO8601DateFormatter().string(from: session.createdAt),
            "folder": session.folderURL.path,
            "hasTranscript": session.hasTranscript,
        ]
        if let summary = session.summary { dict["summary"] = summary }
        if let engine = session.engineType { dict["engine"] = engine }
        if let minutes = session.minutesFile {
            dict["minutesFile"] = session.folderURL.appendingPathComponent(minutes).path
        }
        if let transcript = session.transcriptURL, session.hasTranscript {
            dict["transcriptFile"] = transcript.path
        }
        if let duration = audioDuration(session.systemAudioURL ?? session.micAudioURL) {
            dict["duration"] = duration
        }
        return dict
    }

    private func audioDuration(_ url: URL?) -> Double? {
        guard let url, FileManager.default.fileExists(atPath: url.path),
              let file = try? AVAudioFile(forReading: url),
              file.fileFormat.sampleRate > 0 else { return nil }
        return Double(file.length) / file.fileFormat.sampleRate
    }

    private func handleSessionsList() -> [String: Any] {
        ["sessions": loadedSessions().map(sessionDict)]
    }

    private func handleSessionsShow(_ args: [String: String]) throws -> [String: Any] {
        sessionDict(try findSession(args))
    }

    private func handleSessionsDelete(_ args: [String: String]) throws -> [String: Any] {
        let session = try findSession(args)
        onMain {
            self.recordingStore.deleteSession(session)
        }
        return ["deleted": session.id]
    }

    // MARK: - Transcribe

    private func handleTranscribe(_ args: [String: String], send: ([String: Any]) -> Void) throws -> [String: Any] {
        let session = try findSession(args)
        var engine: TranscriptionEngineType?
        if let raw = args["engine"] {
            guard let parsed = TranscriptionEngineType(rawValue: raw) else {
                throw CommandError(message: "unknown engine: \(raw) (apple | whisper)")
            }
            engine = parsed
        }
        guard session.systemAudioURL != nil || session.micAudioURL != nil else {
            throw CommandError(message: "session has no audio files")
        }

        let started = onMain {
            self.manager.startTranscribeSession(
                folder: session.folderURL,
                systemAudio: session.systemAudioURL,
                micAudio: session.micAudioURL,
                engine: engine
            )
        }
        guard started else {
            throw CommandError(message: String(localized: "録音中または文字起こし中のため実行できません"))
        }

        while true {
            let state: (String?, String?, Double?) = onMain {
                (self.manager.autoTranscribeSessionId, self.manager.autoTranscribePhase, self.manager.autoTranscribeProgress)
            }
            guard state.0 != nil else { break }
            var progress: [String: Any] = ["progress": state.1 ?? "transcribing"]
            if let fraction = state.2 { progress["fraction"] = fraction }
            send(progress)
            Thread.sleep(forTimeInterval: 0.5)
        }

        let refreshed = loadedSessions().first { $0.id == session.id }
        guard let refreshed, refreshed.hasTranscript else {
            throw CommandError(message: "transcription finished but no transcript was produced")
        }
        return sessionDict(refreshed)
    }

    // MARK: - Minutes / Presets

    private func presetStore() -> MinutesPresetStore {
        MinutesPresetStore.shared
    }

    private func findPreset(_ args: [String: String], key: String = "preset") throws -> MinutesPreset {
        guard let name = args[key], !name.isEmpty else {
            throw CommandError(message: "\(key) is required")
        }
        let presets = onMain { self.presetStore().presets }
        if let preset = presets.first(where: { $0.name == name })
            ?? presets.first(where: { $0.id.uuidString == name }) {
            return preset
        }
        throw CommandError(message: "preset not found: \(name)")
    }

    private func presetDict(_ preset: MinutesPreset) -> [String: Any] {
        var dict: [String: Any] = [
            "id": preset.id.uuidString,
            "name": preset.name,
            "backend": preset.backend.rawValue,
            "model": preset.model,
        ]
        if let ctx = preset.resolveContextFolder() { dict["contextFolder"] = ctx.path }
        if let out = preset.resolveOutputFolder() { dict["outputFolder"] = out.path }
        return dict
    }

    private func handleMinutes(_ args: [String: String], send: ([String: Any]) -> Void) throws -> [String: Any] {
        let session = try findSession(args)
        var preset = try findPreset(args)
        guard let transcriptURL = session.transcriptURL,
              let transcript = try? String(contentsOf: transcriptURL, encoding: .utf8),
              !transcript.isEmpty else {
            throw CommandError(message: "session has no transcript: \(session.id)")
        }

        send(["progress": String(localized: "議事録を生成中 (\(preset.backend.displayName))")])
        preset.lastUsedAt = Date()

        let result: (URL?, String?) = onMain {
            self.presetStore().addOrUpdate(preset)
            let generator = MinutesGenerator()
            await generator.generate(transcript: transcript, preset: preset, sessionDate: session.id)
            return (generator.outputFileURL, generator.errorMessage)
        }
        if let error = result.1 { throw CommandError(message: error) }
        guard let outputURL = result.0 else {
            throw CommandError(message: "generation produced no output")
        }

        onMain {
            let minutesName = "minutes.md"
            let dest = session.folderURL.appendingPathComponent(minutesName)
            let fm = FileManager.default
            try? fm.removeItem(at: dest)
            try? fm.copyItem(at: outputURL, to: dest)
            var updated = session
            updated.minutesFile = minutesName
            updated.minutesPresetId = preset.id
            self.recordingStore.saveMetadata(for: updated)
            self.recordingStore.loadSessions()
        }
        return ["outputFile": outputURL.path]
    }

    private func handlePresetList() -> [String: Any] {
        let presets = onMain { self.presetStore().presets }
        return ["presets": presets.map(presetDict)]
    }

    private func handlePresetShow(_ args: [String: String]) throws -> [String: Any] {
        presetDict(try findPreset(args, key: "name"))
    }

    private func handlePresetCreate(_ args: [String: String]) throws -> [String: Any] {
        guard let name = args["name"], !name.isEmpty else {
            throw CommandError(message: "name is required")
        }
        guard let contextPath = args["context"], !contextPath.isEmpty else {
            throw CommandError(message: "context folder is required")
        }
        let outputPath = args["output"] ?? contextPath

        var backend = MinutesBackend.claude
        if let raw = args["backend"] {
            guard let parsed = MinutesBackend(rawValue: raw) else {
                throw CommandError(message: "unknown backend: \(raw) (claude | codex | gemini)")
            }
            backend = parsed
        }

        let fm = FileManager.default
        var isDir: ObjCBool = false
        let contextURL = URL(fileURLWithPath: (contextPath as NSString).expandingTildeInPath).standardizedFileURL
        let outputURL = URL(fileURLWithPath: (outputPath as NSString).expandingTildeInPath).standardizedFileURL
        for url in [contextURL, outputURL] {
            guard fm.fileExists(atPath: url.path, isDirectory: &isDir), isDir.boolValue else {
                throw CommandError(message: "folder not found: \(url.path)")
            }
        }
        guard let ctxBookmark = MinutesPreset.createBookmark(for: contextURL),
              let outBookmark = MinutesPreset.createBookmark(for: outputURL) else {
            throw CommandError(message: "failed to create folder bookmarks")
        }

        let existing = onMain { self.presetStore().presets }
        guard !existing.contains(where: { $0.name == name }) else {
            throw CommandError(message: "preset already exists: \(name)")
        }

        let preset = MinutesPreset(
            name: name,
            contextFolderBookmark: ctxBookmark,
            outputFolderBookmark: outBookmark,
            backend: backend,
            model: args["model"]
        )
        onMain {
            self.presetStore().addOrUpdate(preset)
        }
        return presetDict(preset)
    }

    private func handlePresetDelete(_ args: [String: String]) throws -> [String: Any] {
        let preset = try findPreset(args, key: "name")
        onMain {
            self.presetStore().delete(preset)
        }
        return ["deleted": preset.name]
    }

    // MARK: - Config

    private static let configKeys = [
        "liveTranscriptionEnabled",
        "autoTranscribeEnabled",
        "autoTranscribeEngine",
        "silenceAutoStopEnabled",
        "silenceAutoStopMinutes",
        "transcriptionLocale",
        "whisperModelId",
        "hookTimeoutSeconds",
    ]

    private func configValue(_ key: String) -> String? {
        onMain {
            switch key {
            case "liveTranscriptionEnabled": String(self.manager.liveTranscriptionEnabled)
            case "autoTranscribeEnabled": String(self.manager.autoTranscribeEnabled)
            case "autoTranscribeEngine": self.manager.autoTranscribeEngine.rawValue
            case "silenceAutoStopEnabled": String(self.manager.silenceAutoStopEnabled)
            case "silenceAutoStopMinutes": String(self.manager.silenceAutoStopMinutes)
            case "transcriptionLocale": self.manager.transcriptionLocale.identifier
            case "whisperModelId": self.whisperManager.selectedModelId
            case "hookTimeoutSeconds": String(HookRunner.timeoutSeconds)
            default: nil
            }
        }
    }

    private func handleConfigList() -> [String: Any] {
        var values: [String: Any] = [:]
        for key in Self.configKeys {
            values[key] = configValue(key) ?? ""
        }
        return ["config": values]
    }

    private func handleConfigGet(_ args: [String: String]) throws -> [String: Any] {
        guard let key = args["key"], let value = configValue(key) else {
            throw CommandError(message: "unknown config key: \(args["key"] ?? "") (available: \(Self.configKeys.joined(separator: ", ")))")
        }
        return ["key": key, "value": value]
    }

    private func handleConfigSet(_ args: [String: String]) throws -> [String: Any] {
        guard let key = args["key"], Self.configKeys.contains(key) else {
            throw CommandError(message: "unknown config key: \(args["key"] ?? "") (available: \(Self.configKeys.joined(separator: ", ")))")
        }
        guard let value = args["value"] else {
            throw CommandError(message: "value is required")
        }

        func parseBool() throws -> Bool {
            switch value.lowercased() {
            case "true", "1", "on", "yes": return true
            case "false", "0", "off", "no": return false
            default: throw CommandError(message: "expected true/false: \(value)")
            }
        }
        func parseInt() throws -> Int {
            guard let n = Int(value), n > 0 else {
                throw CommandError(message: "expected positive integer: \(value)")
            }
            return n
        }

        switch key {
        case "liveTranscriptionEnabled":
            let flag = try parseBool()
            onMain { self.manager.liveTranscriptionEnabled = flag }
        case "autoTranscribeEnabled":
            let flag = try parseBool()
            onMain { self.manager.autoTranscribeEnabled = flag }
        case "autoTranscribeEngine":
            guard let engine = TranscriptionEngineType(rawValue: value) else {
                throw CommandError(message: "unknown engine: \(value) (apple | whisper)")
            }
            onMain { self.manager.autoTranscribeEngine = engine }
        case "silenceAutoStopEnabled":
            let flag = try parseBool()
            onMain { self.manager.silenceAutoStopEnabled = flag }
        case "silenceAutoStopMinutes":
            let minutes = try parseInt()
            onMain { self.manager.silenceAutoStopMinutes = minutes }
        case "transcriptionLocale":
            let locale = TranscriptionLocale.resolve(value)
            onMain { self.manager.transcriptionLocale = locale }
        case "whisperModelId":
            guard WhisperModelInfo.all.contains(where: { $0.id == value }) else {
                throw CommandError(message: "unknown model: \(value)")
            }
            onMain { self.whisperManager.selectedModelId = value }
        case "hookTimeoutSeconds":
            let seconds = try parseInt()
            UserDefaults.standard.set(seconds, forKey: "hookTimeoutSeconds")
        default:
            throw CommandError(message: "unknown config key: \(key)")
        }
        return ["key": key, "value": value]
    }

    // MARK: - Whisper models

    private func handleModelList() -> [String: Any] {
        let (downloaded, selected) = onMain {
            (self.whisperManager.downloadedModelIds, self.whisperManager.selectedModelId)
        }
        let models = WhisperModelInfo.all.map { info -> [String: Any] in
            [
                "id": info.id,
                "name": info.name,
                "size": info.size,
                "downloaded": downloaded.contains(info.id),
                "selected": info.id == selected,
            ]
        }
        return ["models": models]
    }

    private func handleModelDownload(_ args: [String: String], send: ([String: Any]) -> Void) throws -> [String: Any] {
        guard let modelId = args["model"], WhisperModelInfo.all.contains(where: { $0.id == modelId }) else {
            throw CommandError(message: "unknown model: \(args["model"] ?? "") (see `fennec model list`)")
        }
        let alreadyDownloading = onMain { self.whisperManager.isDownloading }
        if alreadyDownloading {
            throw CommandError(message: String(localized: "モデルをダウンロード中です"))
        }

        let sem = DispatchSemaphore(value: 0)
        Task { @MainActor in
            self.whisperManager.selectedModelId = modelId
            await self.whisperManager.downloadModel()
            sem.signal()
        }
        while sem.wait(timeout: .now() + 1) == .timedOut {
            let fraction = onMain { self.whisperManager.downloadProgress }
            send(["progress": "downloading", "fraction": fraction])
        }

        let error = onMain { self.whisperManager.downloadError }
        if let error { throw CommandError(message: error) }
        return ["model": modelId, "path": onMain { self.whisperManager.currentModelPath } ?? ""]
    }

    // MARK: - Hooks

    private func hookDict(_ hook: AutomationHook) -> [String: Any] {
        [
            "id": hook.id.uuidString,
            "event": hook.event.rawValue,
            "command": hook.command,
            "enabled": hook.enabled,
        ]
    }

    private func handleHookList() -> [String: Any] {
        let hooks = onMain { HookStore.shared.hooks }
        return ["hooks": hooks.map(hookDict)]
    }

    private func handleHookAdd(_ args: [String: String]) throws -> [String: Any] {
        guard let eventRaw = args["event"], let event = HookEvent(rawValue: eventRaw) else {
            let events = HookEvent.allCases.map(\.rawValue).joined(separator: ", ")
            throw CommandError(message: "unknown event: \(args["event"] ?? "") (available: \(events))")
        }
        guard let command = args["hookCommand"], !command.isEmpty else {
            throw CommandError(message: "command is required")
        }
        let hook = AutomationHook(event: event, command: command)
        onMain {
            HookStore.shared.hooks.append(hook)
        }
        return hookDict(hook)
    }

    private func findHook(_ args: [String: String]) throws -> AutomationHook {
        guard let id = args["id"], !id.isEmpty else {
            throw CommandError(message: "id is required")
        }
        let hooks = onMain { HookStore.shared.hooks }
        let matches = hooks.filter { $0.id.uuidString.lowercased().hasPrefix(id.lowercased()) }
        guard matches.count == 1 else {
            throw CommandError(message: matches.isEmpty ? "hook not found: \(id)" : "ambiguous hook id: \(id)")
        }
        return matches[0]
    }

    private func handleHookSetEnabled(_ args: [String: String]) throws -> [String: Any] {
        let hook = try findHook(args)
        let enabled = args["enabled"] == "true"
        onMain {
            if let idx = HookStore.shared.hooks.firstIndex(where: { $0.id == hook.id }) {
                HookStore.shared.hooks[idx].enabled = enabled
            }
        }
        var updated = hook
        updated.enabled = enabled
        return hookDict(updated)
    }

    private func handleHookDelete(_ args: [String: String]) throws -> [String: Any] {
        let hook = try findHook(args)
        onMain {
            HookStore.shared.hooks.removeAll { $0.id == hook.id }
        }
        return ["deleted": hook.id.uuidString]
    }
}
