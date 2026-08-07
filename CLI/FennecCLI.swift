import AppKit
import ArgumentParser
import Foundation

enum IPCClientError: Error, CustomStringConvertible {
    case notRunning
    case protocolError
    case remote(String)

    var description: String {
        switch self {
        case .notRunning: "\(FennecIPC.appName) is not running (start the app, or use `fennec status --launch`)"
        case .protocolError: "invalid response from Fennec.app"
        case .remote(let message): message
        }
    }
}

enum IPCClient {
    static func request(
        _ command: String,
        _ args: [String: String] = [:],
        onProgress: ((String, Double?) -> Void)? = nil
    ) throws -> Any? {
        let fd = try connect()
        defer { close(fd) }

        guard let payload = FennecIPC.encodeLine(["command": command, "args": args]) else {
            throw IPCClientError.protocolError
        }
        try payload.withUnsafeBytes { buf in
            var offset = 0
            while offset < buf.count {
                let written = write(fd, buf.baseAddress!.advanced(by: offset), buf.count - offset)
                guard written > 0 else { throw IPCClientError.protocolError }
                offset += written
            }
        }

        var buffer = Data()
        var chunk = [UInt8](repeating: 0, count: 65536)
        while true {
            while let newline = buffer.firstIndex(of: 0x0A) {
                let line = buffer.prefix(upTo: newline)
                buffer.removeSubrange(...newline)
                guard let object = (try? JSONSerialization.jsonObject(with: line)) as? [String: Any] else {
                    throw IPCClientError.protocolError
                }
                if let ok = object["ok"] as? Bool {
                    if ok { return object["data"] }
                    throw IPCClientError.remote(object["error"] as? String ?? "unknown error")
                }
                if let progress = object["progress"] as? String {
                    onProgress?(progress, object["fraction"] as? Double)
                }
            }
            let count = read(fd, &chunk, chunk.count)
            guard count > 0 else { throw IPCClientError.protocolError }
            buffer.append(contentsOf: chunk[0..<count])
        }
    }

    static func connect() throws -> Int32 {
        let fd = socket(AF_UNIX, SOCK_STREAM, 0)
        guard fd >= 0 else { throw IPCClientError.notRunning }
        var addr = sockaddr_un()
        addr.sun_family = sa_family_t(AF_UNIX)
        let pathBytes = FennecIPC.socketPath.utf8CString
        guard pathBytes.count <= MemoryLayout.size(ofValue: addr.sun_path) else {
            close(fd)
            throw IPCClientError.notRunning
        }
        withUnsafeMutableBytes(of: &addr.sun_path) { dest in
            pathBytes.withUnsafeBytes { src in
                dest.copyBytes(from: src)
            }
        }
        let size = socklen_t(MemoryLayout<sockaddr_un>.size)
        let result = withUnsafePointer(to: &addr) { ptr in
            ptr.withMemoryRebound(to: sockaddr.self, capacity: 1) { sa in
                Darwin.connect(fd, sa, size)
            }
        }
        guard result == 0 else {
            close(fd)
            throw IPCClientError.notRunning
        }
        return fd
    }

    static func launchAppAndWait(timeout: TimeInterval = 10) -> Bool {
        guard let url = NSWorkspace.shared.urlForApplication(withBundleIdentifier: FennecIPC.appBundleID) else {
            return false
        }
        let config = NSWorkspace.OpenConfiguration()
        config.activates = false
        let sem = DispatchSemaphore(value: 0)
        NSWorkspace.shared.openApplication(at: url, configuration: config) { _, _ in
            sem.signal()
        }
        sem.wait()
        let deadline = Date().addingTimeInterval(timeout)
        while Date() < deadline {
            if let fd = try? connect() {
                close(fd)
                return true
            }
            Thread.sleep(forTimeInterval: 0.3)
        }
        return false
    }
}

func printJSON(_ value: Any?) {
    guard let value else {
        print("{}")
        return
    }
    if let data = try? JSONSerialization.data(withJSONObject: value, options: [.prettyPrinted, .sortedKeys]),
       let text = String(data: data, encoding: .utf8) {
        print(text)
    }
}

func progressPrinter(_ message: String, _ fraction: Double?) {
    if let fraction {
        FileHandle.standardError.write(Data("\r\(message) \(Int(fraction * 100))%   ".utf8))
    } else {
        FileHandle.standardError.write(Data("\(message)\n".utf8))
    }
}

func fail(_ error: Error) -> Never {
    FileHandle.standardError.write(Data("Error: \(error)\n".utf8))
    if let ipcError = error as? IPCClientError, case .notRunning = ipcError {
        exit(2)
    }
    exit(1)
}

@main
struct FennecCLI: ParsableCommand {
    static let configuration = CommandConfiguration(
        commandName: "fennec",
        abstract: "Control the Fennec recording app from the command line.",
        subcommands: [
            Status.self, Record.self, Sessions.self, Transcribe.self,
            Minutes.self, Preset.self, Config.self, Model.self, Hook.self,
        ]
    )
}

struct Status: ParsableCommand {
    static let configuration = CommandConfiguration(abstract: "Show app status.")

    @Flag(help: "Launch Fennec.app if it is not running.")
    var launch = false

    @Flag(name: .customLong("json"), help: "Output JSON.")
    var asJSON = false

    func run() throws {
        var data: Any?
        do {
            data = try IPCClient.request("status")
        } catch IPCClientError.notRunning {
            if launch, IPCClient.launchAppAndWait() {
                data = try IPCClient.request("status")
            } else {
                if asJSON {
                    printJSON(["running": false])
                } else {
                    print("not running")
                }
                return
            }
        }
        if asJSON {
            printJSON(data)
            return
        }
        let dict = data as? [String: Any] ?? [:]
        if dict["recording"] as? Bool == true {
            let duration = Int(dict["duration"] as? Double ?? 0)
            print("recording (\(String(format: "%02d:%02d", duration / 60, duration % 60)), session \(dict["sessionId"] as? String ?? "?"))")
        } else if dict["transcribing"] as? Bool == true {
            print("transcribing (session \(dict["transcribingSessionId"] as? String ?? "?"))")
        } else {
            print("idle")
        }
    }
}

struct Record: ParsableCommand {
    static let configuration = CommandConfiguration(
        abstract: "Start or stop recording.",
        subcommands: [Start.self, Stop.self, RecStatus.self]
    )

    struct Start: ParsableCommand {
        static let configuration = CommandConfiguration(abstract: "Start recording.")
        func run() throws {
            do {
                let data = try IPCClient.request("record.start") as? [String: Any]
                print("recording started: \(data?["sessionId"] as? String ?? "")")
            } catch { fail(error) }
        }
    }

    struct Stop: ParsableCommand {
        static let configuration = CommandConfiguration(abstract: "Stop recording.")
        func run() throws {
            do {
                let data = try IPCClient.request("record.stop") as? [String: Any]
                print("recording stopped: \(data?["sessionId"] as? String ?? "")")
            } catch { fail(error) }
        }
    }

    struct RecStatus: ParsableCommand {
        static let configuration = CommandConfiguration(commandName: "status", abstract: "Show recording status.")
        func run() throws {
            do {
                printJSON(try IPCClient.request("record.status"))
            } catch { fail(error) }
        }
    }
}

struct Sessions: ParsableCommand {
    static let configuration = CommandConfiguration(
        abstract: "Manage recording sessions.",
        subcommands: [List.self, Show.self, Delete.self],
        defaultSubcommand: List.self
    )

    struct List: ParsableCommand {
        static let configuration = CommandConfiguration(abstract: "List sessions.")

        @Flag(name: .customLong("json"), help: "Output JSON.")
        var asJSON = false

        func run() throws {
            do {
                let data = try IPCClient.request("sessions.list") as? [String: Any]
                let sessions = data?["sessions"] as? [[String: Any]] ?? []
                if asJSON {
                    printJSON(sessions)
                    return
                }
                for session in sessions {
                    let id = session["id"] as? String ?? "?"
                    let summary = (session["summary"] as? String ?? "-")
                        .replacingOccurrences(of: "\n", with: " ")
                    let transcript = session["hasTranscript"] as? Bool == true ? "T" : " "
                    let minutes = session["minutesFile"] != nil ? "M" : " "
                    var duration = "--:--"
                    if let seconds = session["duration"] as? Double {
                        let total = Int(seconds.rounded())
                        duration = total >= 3600
                            ? String(format: "%d:%02d:%02d", total / 3600, (total % 3600) / 60, total % 60)
                            : String(format: "%d:%02d", total / 60, total % 60)
                    }
                    duration = String(repeating: " ", count: max(0, 8 - duration.count)) + duration
                    print("\(id)  [\(transcript)\(minutes)]  \(duration)  \(summary)")
                }
            } catch { fail(error) }
        }
    }

    struct Show: ParsableCommand {
        static let configuration = CommandConfiguration(abstract: "Show session details.")

        @Argument(help: "Session ID (or `latest`).")
        var sessionId: String

        func run() throws {
            do {
                printJSON(try IPCClient.request("sessions.show", ["sessionId": sessionId]))
            } catch { fail(error) }
        }
    }

    struct Delete: ParsableCommand {
        static let configuration = CommandConfiguration(abstract: "Move a session to the Trash.")

        @Argument(help: "Session ID.")
        var sessionId: String

        @Flag(help: "Skip confirmation.")
        var force = false

        func run() throws {
            if !force {
                print("Delete session \(sessionId)? [y/N] ", terminator: "")
                guard readLine()?.lowercased() == "y" else {
                    print("cancelled")
                    return
                }
            }
            do {
                _ = try IPCClient.request("sessions.delete", ["sessionId": sessionId])
                print("deleted: \(sessionId)")
            } catch { fail(error) }
        }
    }
}

struct Transcribe: ParsableCommand {
    static let configuration = CommandConfiguration(abstract: "Transcribe a recorded session.")

    @Argument(help: "Session ID (or `latest`).")
    var sessionId: String

    @Option(help: "Engine: apple | whisper (defaults to app setting).")
    var engine: String?

    func run() throws {
        var args = ["sessionId": sessionId]
        if let engine { args["engine"] = engine }
        do {
            let data = try IPCClient.request("transcribe", args, onProgress: progressPrinter) as? [String: Any]
            FileHandle.standardError.write(Data("\n".utf8))
            print("transcript: \(data?["transcriptFile"] as? String ?? "")")
        } catch { fail(error) }
    }
}

struct Minutes: ParsableCommand {
    static let configuration = CommandConfiguration(abstract: "Generate meeting minutes for a session.")

    @Argument(help: "Session ID (or `latest`).")
    var sessionId: String

    @Option(help: "Preset name (see `fennec preset list`). Omit to generate without a preset.")
    var preset: String?

    @Option(help: "Backend when no preset is used (claude | codex | gemini).")
    var backend: String?

    @Option(help: "Model when no preset is used.")
    var model: String?

    func run() throws {
        do {
            var args = ["sessionId": sessionId]
            if let preset { args["preset"] = preset }
            if let backend { args["backend"] = backend }
            if let model { args["model"] = model }
            let data = try IPCClient.request(
                "minutes",
                args,
                onProgress: progressPrinter
            ) as? [String: Any]
            print("minutes: \(data?["outputFile"] as? String ?? "")")
        } catch { fail(error) }
    }
}

struct Preset: ParsableCommand {
    static let configuration = CommandConfiguration(
        abstract: "Manage minutes presets.",
        subcommands: [List.self, Show.self, Create.self, Delete.self],
        defaultSubcommand: List.self
    )

    struct List: ParsableCommand {
        static let configuration = CommandConfiguration(abstract: "List presets.")

        @Flag(name: .customLong("json"), help: "Output JSON.")
        var asJSON = false

        func run() throws {
            do {
                let data = try IPCClient.request("preset.list") as? [String: Any]
                let presets = data?["presets"] as? [[String: Any]] ?? []
                if asJSON {
                    printJSON(presets)
                    return
                }
                for preset in presets {
                    print("\(preset["name"] as? String ?? "?")  (\(preset["backend"] as? String ?? "?") \(preset["model"] as? String ?? ""))  \(preset["contextFolder"] as? String ?? "")")
                }
            } catch { fail(error) }
        }
    }

    struct Show: ParsableCommand {
        static let configuration = CommandConfiguration(abstract: "Show preset details.")

        @Argument(help: "Preset name.")
        var name: String

        func run() throws {
            do {
                printJSON(try IPCClient.request("preset.show", ["name": name]))
            } catch { fail(error) }
        }
    }

    struct Create: ParsableCommand {
        static let configuration = CommandConfiguration(abstract: "Create a preset.")

        @Argument(help: "Preset name.")
        var name: String

        @Option(help: "Context folder path (CLAUDE.md, related docs).")
        var context: String

        @Option(help: "Output folder path (defaults to context folder).")
        var output: String?

        @Option(help: "Backend: claude | codex | gemini.")
        var backend: String?

        @Option(help: "Model ID for the backend.")
        var model: String?

        func run() throws {
            var args = ["name": name, "context": context]
            if let output { args["output"] = output }
            if let backend { args["backend"] = backend }
            if let model { args["model"] = model }
            do {
                printJSON(try IPCClient.request("preset.create", args))
            } catch { fail(error) }
        }
    }

    struct Delete: ParsableCommand {
        static let configuration = CommandConfiguration(abstract: "Delete a preset.")

        @Argument(help: "Preset name.")
        var name: String

        func run() throws {
            do {
                _ = try IPCClient.request("preset.delete", ["name": name])
                print("deleted: \(name)")
            } catch { fail(error) }
        }
    }
}

struct Config: ParsableCommand {
    static let configuration = CommandConfiguration(
        abstract: "Read and write app settings.",
        subcommands: [List.self, Get.self, Set.self],
        defaultSubcommand: List.self
    )

    struct List: ParsableCommand {
        static let configuration = CommandConfiguration(abstract: "List settings.")

        @Flag(name: .customLong("json"), help: "Output JSON.")
        var asJSON = false

        func run() throws {
            do {
                let data = try IPCClient.request("config.list") as? [String: Any]
                let config = data?["config"] as? [String: Any] ?? [:]
                if asJSON {
                    printJSON(config)
                    return
                }
                for key in config.keys.sorted() {
                    print("\(key) = \(config[key] ?? "")")
                }
            } catch { fail(error) }
        }
    }

    struct Get: ParsableCommand {
        static let configuration = CommandConfiguration(abstract: "Get a setting value.")

        @Argument var key: String

        func run() throws {
            do {
                let data = try IPCClient.request("config.get", ["key": key]) as? [String: Any]
                print(data?["value"] as? String ?? "")
            } catch { fail(error) }
        }
    }

    struct Set: ParsableCommand {
        static let configuration = CommandConfiguration(abstract: "Set a setting value.")

        @Argument var key: String
        @Argument var value: String

        func run() throws {
            do {
                _ = try IPCClient.request("config.set", ["key": key, "value": value])
                print("\(key) = \(value)")
            } catch { fail(error) }
        }
    }
}

struct Model: ParsableCommand {
    static let configuration = CommandConfiguration(
        abstract: "Manage Whisper models.",
        subcommands: [List.self, Download.self],
        defaultSubcommand: List.self
    )

    struct List: ParsableCommand {
        static let configuration = CommandConfiguration(abstract: "List Whisper models.")

        @Flag(name: .customLong("json"), help: "Output JSON.")
        var asJSON = false

        func run() throws {
            do {
                let data = try IPCClient.request("model.list") as? [String: Any]
                let models = data?["models"] as? [[String: Any]] ?? []
                if asJSON {
                    printJSON(models)
                    return
                }
                for model in models {
                    let downloaded = model["downloaded"] as? Bool == true ? "✓" : " "
                    let selected = model["selected"] as? Bool == true ? "*" : " "
                    print("\(selected)\(downloaded) \(model["id"] as? String ?? "?")  (\(model["size"] as? String ?? ""))")
                }
            } catch { fail(error) }
        }
    }

    struct Download: ParsableCommand {
        static let configuration = CommandConfiguration(abstract: "Download a Whisper model and select it.")

        @Argument(help: "Model ID (see `fennec model list`).")
        var modelId: String

        func run() throws {
            do {
                let data = try IPCClient.request("model.download", ["model": modelId], onProgress: progressPrinter) as? [String: Any]
                FileHandle.standardError.write(Data("\n".utf8))
                print("downloaded: \(data?["path"] as? String ?? "")")
            } catch { fail(error) }
        }
    }
}

struct Hook: ParsableCommand {
    static let configuration = CommandConfiguration(
        abstract: "Manage automation hooks.",
        subcommands: [List.self, Add.self, Enable.self, Disable.self, Delete.self],
        defaultSubcommand: List.self
    )

    struct List: ParsableCommand {
        static let configuration = CommandConfiguration(abstract: "List hooks.")

        @Flag(name: .customLong("json"), help: "Output JSON.")
        var asJSON = false

        func run() throws {
            do {
                let data = try IPCClient.request("hook.list") as? [String: Any]
                let hooks = data?["hooks"] as? [[String: Any]] ?? []
                if asJSON {
                    printJSON(hooks)
                    return
                }
                for hook in hooks {
                    let id = (hook["id"] as? String ?? "").prefix(8).lowercased()
                    let enabled = hook["enabled"] as? Bool == true ? "on " : "off"
                    print("\(id)  [\(enabled)]  \(hook["event"] as? String ?? "?")  \(hook["command"] as? String ?? "")")
                }
            } catch { fail(error) }
        }
    }

    struct Add: ParsableCommand {
        static let configuration = CommandConfiguration(abstract: "Add a hook.")

        @Argument(help: "Event: recordingStarted | recordingStopped | transcriptionCompleted | minutesGenerated.")
        var event: String

        @Argument(help: "Shell command to run.")
        var command: String

        func run() throws {
            do {
                let data = try IPCClient.request("hook.add", ["event": event, "hookCommand": command]) as? [String: Any]
                print("added: \((data?["id"] as? String ?? "").prefix(8).lowercased())")
            } catch { fail(error) }
        }
    }

    struct Enable: ParsableCommand {
        static let configuration = CommandConfiguration(abstract: "Enable a hook.")

        @Argument(help: "Hook ID (prefix is enough).")
        var id: String

        func run() throws {
            do {
                _ = try IPCClient.request("hook.set-enabled", ["id": id, "enabled": "true"])
                print("enabled: \(id)")
            } catch { fail(error) }
        }
    }

    struct Disable: ParsableCommand {
        static let configuration = CommandConfiguration(abstract: "Disable a hook.")

        @Argument(help: "Hook ID (prefix is enough).")
        var id: String

        func run() throws {
            do {
                _ = try IPCClient.request("hook.set-enabled", ["id": id, "enabled": "false"])
                print("disabled: \(id)")
            } catch { fail(error) }
        }
    }

    struct Delete: ParsableCommand {
        static let configuration = CommandConfiguration(abstract: "Delete a hook.")

        @Argument(help: "Hook ID (prefix is enough).")
        var id: String

        func run() throws {
            do {
                _ = try IPCClient.request("hook.delete", ["id": id])
                print("deleted: \(id)")
            } catch { fail(error) }
        }
    }
}
