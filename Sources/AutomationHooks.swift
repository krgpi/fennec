import Foundation

enum HookEvent: String, Codable, CaseIterable, Identifiable {
    case recordingStarted
    case recordingStopped
    case transcriptionCompleted
    case minutesGenerated

    var id: String { rawValue }

    var displayName: String {
        switch self {
        case .recordingStarted: String(localized: "録音開始時")
        case .recordingStopped: String(localized: "録音停止時")
        case .transcriptionCompleted: String(localized: "文字起こし完了時")
        case .minutesGenerated: String(localized: "議事録生成時")
        }
    }
}

struct AutomationHook: Codable, Identifiable, Hashable {
    var id = UUID()
    var event: HookEvent
    var command: String
    var enabled = true
}

final class HookStore: ObservableObject {
    static let shared = HookStore()
    static let storageKey = "automationHooks"

    @Published var hooks: [AutomationHook] = [] {
        didSet { save() }
    }

    init() {
        hooks = Self.loadHooks()
    }

    static func loadHooks() -> [AutomationHook] {
        guard let data = UserDefaults.standard.data(forKey: storageKey) else { return [] }
        return (try? JSONDecoder().decode([AutomationHook].self, from: data)) ?? []
    }

    private func save() {
        guard let data = try? JSONEncoder().encode(hooks) else { return }
        UserDefaults.standard.set(data, forKey: Self.storageKey)
    }
}

enum ShellEnvironment {
    private static var cached: [String: String]?
    private static let queue = DispatchQueue(label: "io.github.krgpi.Fennec.shell-env")

    static func current() -> [String: String] {
        queue.sync {
            if let cached { return cached }
            var env = ProcessInfo.processInfo.environment
            let shell = Process()
            shell.executableURL = URL(fileURLWithPath: "/bin/zsh")
            shell.arguments = ["-l", "-c", "env"]
            let pipe = Pipe()
            shell.standardOutput = pipe
            shell.standardError = FileHandle.nullDevice
            if (try? shell.run()) != nil {
                shell.waitUntilExit()
                if let data = try? pipe.fileHandleForReading.readToEnd(),
                   let text = String(data: data, encoding: .utf8) {
                    for line in text.split(separator: "\n", omittingEmptySubsequences: true) {
                        guard let eqIdx = line.firstIndex(of: "=") else { continue }
                        let key = String(line[line.startIndex..<eqIdx])
                        let value = String(line[line.index(after: eqIdx)...])
                        env[key] = value
                    }
                }
            }
            let home = NSHomeDirectory()
            let pathDirs = [
                "\(home)/.local/bin",
                "/opt/homebrew/bin",
                "/usr/local/bin",
                "/usr/bin",
                "/bin",
            ]
            if let existing = env["PATH"] {
                env["PATH"] = pathDirs.joined(separator: ":") + ":" + existing
            } else {
                env["PATH"] = pathDirs.joined(separator: ":")
            }
            env["HOME"] = home
            cached = env
            return env
        }
    }
}

enum HookRunner {
    static var timeoutSeconds: Int {
        let stored = UserDefaults.standard.integer(forKey: "hookTimeoutSeconds")
        return stored > 0 ? stored : 30
    }

    static let logFileURL = FileManager.default.urls(for: .libraryDirectory, in: .userDomainMask)[0]
        .appendingPathComponent("Logs/Fennec/hooks.log")

    static func fire(_ event: HookEvent, sessionFolder: URL?, extra: [String: String] = [:]) {
        let hooks = HookStore.loadHooks().filter { $0.event == event && $0.enabled }
        guard !hooks.isEmpty else { return }

        var env = MinutesGenerator.shellEnvironment()
        env["FENNEC_EVENT"] = event.rawValue
        if let folder = sessionFolder {
            env["FENNEC_SESSION_ID"] = folder.lastPathComponent
            env["FENNEC_SESSION_DIR"] = folder.path
            let transcript = folder.appendingPathComponent("transcript_\(folder.lastPathComponent).txt")
            if FileManager.default.fileExists(atPath: transcript.path) {
                env["FENNEC_TRANSCRIPT_FILE"] = transcript.path
            }
        }
        for (key, value) in extra {
            env[key] = value
        }
        let timeout = timeoutSeconds

        for hook in hooks {
            Task.detached(priority: .utility) {
                run(hook, event: event, env: env, timeout: timeout)
            }
        }
    }

    private static func run(_ hook: AutomationHook, event: HookEvent, env: [String: String], timeout: Int) {
        let proc = Process()
        proc.executableURL = URL(fileURLWithPath: "/bin/zsh")
        proc.arguments = ["-l", "-c", hook.command]
        proc.environment = env
        let stderrPipe = Pipe()
        proc.standardOutput = FileHandle.nullDevice
        proc.standardError = stderrPipe

        let started = Date()
        do {
            try proc.run()
        } catch {
            log(event: event, command: hook.command, result: "launch failed: \(error.localizedDescription)")
            return
        }

        var timedOut = false
        let killer = DispatchWorkItem {
            if proc.isRunning {
                timedOut = true
                proc.terminate()
            }
        }
        DispatchQueue.global().asyncAfter(deadline: .now() + .seconds(timeout), execute: killer)
        proc.waitUntilExit()
        killer.cancel()

        let stderrText = (try? stderrPipe.fileHandleForReading.readToEnd())
            .flatMap { String(data: $0, encoding: .utf8) }?
            .trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
        let elapsed = String(format: "%.1fs", Date().timeIntervalSince(started))
        var result = timedOut ? "timeout after \(timeout)s" : "exit \(proc.terminationStatus)"
        result += " (\(elapsed))"
        if !stderrText.isEmpty {
            result += " stderr: \(stderrText.prefix(500))"
        }
        log(event: event, command: hook.command, result: result)
    }

    private static let logQueue = DispatchQueue(label: "io.github.krgpi.Fennec.hook-log")

    private static func log(event: HookEvent, command: String, result: String) {
        logQueue.sync {
            let fm = FileManager.default
            try? fm.createDirectory(at: logFileURL.deletingLastPathComponent(), withIntermediateDirectories: true)
            let line = "[\(ISO8601DateFormatter().string(from: Date()))] \(event.rawValue): \(command) → \(result)\n"
            guard let data = line.data(using: .utf8) else { return }
            if !fm.fileExists(atPath: logFileURL.path) {
                fm.createFile(atPath: logFileURL.path, contents: nil)
            }
            if let handle = try? FileHandle(forWritingTo: logFileURL) {
                handle.seekToEndOfFile()
                handle.write(data)
                try? handle.close()
            }
        }
    }
}
