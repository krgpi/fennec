import Foundation

enum MinutesBackend: String, Codable, CaseIterable, Hashable {
    case claude = "claude"
    case codex = "codex"
    case gemini = "gemini"

    var displayName: String {
        switch self {
        case .claude: "Claude Code"
        case .codex: "Codex"
        case .gemini: "Gemini"
        }
    }

    var binaryNames: [String] {
        switch self {
        case .claude: ["claude"]
        case .codex: ["codex"]
        case .gemini: ["gemini"]
        }
    }

    var defaultModel: String {
        switch self {
        case .claude: "sonnet"
        case .codex: ""
        case .gemini: "gemini-2.5-flash"
        }
    }

    var availableModels: [(id: String, name: String)] {
        switch self {
        case .claude: [("sonnet", "Sonnet"), ("opus", "Opus"), ("haiku", "Haiku")]
        case .codex: [("", "デフォルト")]
        case .gemini: [("gemini-2.5-pro", "2.5 Pro"), ("gemini-2.5-flash", "2.5 Flash")]
        }
    }

    var usesStdinPrompt: Bool {
        switch self {
        case .codex: true
        default: false
        }
    }

    func buildArguments(model: String, prompt: String, outputFile: URL? = nil) -> [String] {
        switch self {
        case .claude:
            return ["-p", "--model", model, "--output-format", "text", prompt]
        case .codex:
            var args = ["exec", "--skip-git-repo-check", "--ephemeral"]
            if !model.isEmpty {
                args += ["--model", model]
            }
            if let outputFile {
                args += ["-o", outputFile.path]
            }
            args.append("-")
            return args
        case .gemini:
            return ["-p", prompt, "-m", model]
        }
    }

    func findCLI() -> String? {
        if let cached = Self.cliPathCache[self] {
            return cached
        }
        let path = Self.findBinary(names: binaryNames)
        Self.cliPathCache[self] = path
        return path
    }

    private static var cliPathCache: [MinutesBackend: String?] = [:]

    static var availableBackends: [MinutesBackend] {
        allCases.filter { $0.findCLI() != nil }
    }

    private static func findBinary(names: [String]) -> String? {
        let home = NSHomeDirectory()
        let fm = FileManager.default
        let searchDirs = [
            "\(home)/.local/bin",
            "/usr/local/bin",
            "/opt/homebrew/bin",
            "\(home)/.claude/local",
        ]

        for name in names {
            for dir in searchDirs {
                let path = "\(dir)/\(name)"
                let resolved = (try? fm.destinationOfSymbolicLink(atPath: path)) ?? path
                if fm.isExecutableFile(atPath: resolved) {
                    return resolved
                }
            }
        }

        for name in names {
            let versionsDir = "\(home)/.local/share/\(name)/versions"
            if let entries = try? fm.contentsOfDirectory(atPath: versionsDir) {
                for entry in entries.sorted().reversed() {
                    let path = "\(versionsDir)/\(entry)"
                    if fm.isExecutableFile(atPath: path) {
                        return path
                    }
                }
            }
        }

        for name in names {
            let shell = Process()
            shell.executableURL = URL(fileURLWithPath: "/bin/zsh")
            shell.arguments = ["-l", "-c", "which \(name)"]
            let pipe = Pipe()
            shell.standardOutput = pipe
            try? shell.run()
            shell.waitUntilExit()
            if shell.terminationStatus == 0,
               let data = try? pipe.fileHandleForReading.readToEnd(),
               let path = String(data: data, encoding: .utf8)?.trimmingCharacters(in: .whitespacesAndNewlines),
               !path.isEmpty {
                return path
            }
        }

        return nil
    }
}

@MainActor
final class MinutesGenerator: ObservableObject {
    @Published var isRunning = false
    @Published var output = ""
    @Published var errorMessage: String?
    @Published var outputFileURL: URL?

    private var process: Process?

    func generate(transcript: String, preset: MinutesPreset, sessionDate: String) async {
        guard let contextFolder = preset.resolveContextFolder(),
              let outputFolder = preset.resolveOutputFolder() else {
            errorMessage = "フォルダにアクセスできません"
            return
        }

        guard let cliPath = preset.backend.findCLI() else {
            errorMessage = "\(preset.backend.displayName) CLI が見つかりません"
            return
        }

        isRunning = true
        output = ""
        errorMessage = nil
        outputFileURL = nil

        let claudeMd = Self.loadClaudeMd(in: contextFolder)
        let existingNames = Self.listExistingFileNames(in: outputFolder)
        let previousMinutes = Self.findLatestMinutes(in: outputFolder)

        var fileNameInstruction = ""
        if existingNames.isEmpty {
            fileNameInstruction = "ファイル名は自由に決めてください。"
        } else {
            fileNameInstruction = "出力先フォルダの既存ファイル: \(existingNames.joined(separator: ", "))\n既存ファイルの命名規則に合わせてファイル名を決めてください。"
        }

        var prompt = """
        以下は会議の文字起こしです。これを元に議事録を作成してください。

        # 文字起こし
        \(transcript)

        # 指示
        - **出力の1行目にファイル名のみを記述**（拡張子 .md 付き、それ以外のテキストは不要）
        - 2行目以降に Markdown 形式の議事録を出力
        - 参加者、議題、決定事項、アクションアイテムを整理
        - コンテキストフォルダ内の関連ファイルも参考にしてよい
        - 議事録のみを出力し、余計な説明は不要

        # ファイル名
        \(fileNameInstruction)
        日付情報: \(sessionDate)
        """

        if let claudeMd {
            prompt += "\n\n# コンテキストフォルダの CLAUDE.md\n以下の指示に従って議事録のフォーマットや内容を調整してください。\n\n\(claudeMd)"
        }

        if let previousMinutes {
            prompt += "\n\n# 前回の議事録\n以下は前回の議事録です。構成・フォーマット・見出し構造を踏襲し、内容の連続性（前回のアクションアイテムの進捗、継続議題など）を意識してください。\n\n\(previousMinutes)"
        }

        let backend = preset.backend
        var codexOutputFile: URL?
        if backend == .codex {
            codexOutputFile = URL(fileURLWithPath: NSTemporaryDirectory())
                .appendingPathComponent("codex_minutes_\(UUID().uuidString).md")
        }

        let proc = Process()
        proc.executableURL = URL(fileURLWithPath: cliPath)
        proc.arguments = backend.buildArguments(model: preset.model, prompt: prompt, outputFile: codexOutputFile)
        proc.currentDirectoryURL = contextFolder

        proc.environment = Self.shellEnvironment()

        let stdoutPipe = Pipe()
        let stderrPipe = Pipe()
        proc.standardOutput = stdoutPipe
        proc.standardError = stderrPipe

        if backend.usesStdinPrompt {
            let stdinPipe = Pipe()
            proc.standardInput = stdinPipe
            if let data = prompt.data(using: .utf8) {
                stdinPipe.fileHandleForWriting.write(data)
                stdinPipe.fileHandleForWriting.closeFile()
            }
        }

        self.process = proc

        do {
            try proc.run()
        } catch {
            isRunning = false
            errorMessage = "CLI の起動に失敗しました: \(error.localizedDescription)"
            return
        }

        let (stdoutResult, stderrResult, status) = await withCheckedContinuation { (continuation: CheckedContinuation<(String, String, Int32), Never>) in
            DispatchQueue.global(qos: .userInitiated).async {
                let stdoutData = stdoutPipe.fileHandleForReading.readDataToEndOfFile()
                let stderrData = stderrPipe.fileHandleForReading.readDataToEndOfFile()
                proc.waitUntilExit()
                let out = String(data: stdoutData, encoding: .utf8) ?? ""
                let err = String(data: stderrData, encoding: .utf8) ?? ""
                continuation.resume(returning: (out, err, proc.terminationStatus))
            }
        }

        if let codexOutputFile, let fileContent = try? String(contentsOf: codexOutputFile, encoding: .utf8), !fileContent.isEmpty {
            output = fileContent
            try? FileManager.default.removeItem(at: codexOutputFile)
        } else {
            output = stdoutResult
        }
        self.process = nil

        if status != 0 {
            isRunning = false
            errorMessage = "生成に失敗しました (exit \(status))\(stderrResult.isEmpty ? "" : ": \(stderrResult)")"
            return
        }

        let (fileName, body) = Self.extractFileName(from: output, fallback: "minutes_\(sessionDate).md")
        output = body
        let fileURL = outputFolder.appendingPathComponent(fileName)
        do {
            try output.write(to: fileURL, atomically: true, encoding: .utf8)
            self.outputFileURL = fileURL
        } catch {
            errorMessage = "ファイルの保存に失敗しました: \(error.localizedDescription)"
        }
        isRunning = false
    }

    func cancel() {
        process?.terminate()
    }

    private static var cachedShellEnv: [String: String]?

    private static func shellEnvironment() -> [String: String] {
        if let cached = cachedShellEnv { return cached }
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
        cachedShellEnv = env
        return env
    }

    private static func loadClaudeMd(in folder: URL) -> String? {
        let candidates = ["CLAUDE.md", "claude.md"]
        for name in candidates {
            let url = folder.appendingPathComponent(name)
            if let text = try? String(contentsOf: url, encoding: .utf8), !text.isEmpty {
                return text
            }
        }
        return nil
    }

    private static func findLatestMinutes(in folder: URL) -> String? {
        let fm = FileManager.default
        guard let files = try? fm.contentsOfDirectory(at: folder, includingPropertiesForKeys: [.contentModificationDateKey], options: .skipsHiddenFiles) else { return nil }

        let minutesFiles = files
            .filter { $0.pathExtension == "md" }
            .sorted { a, b in
                let da = (try? a.resourceValues(forKeys: [.contentModificationDateKey]).contentModificationDate) ?? .distantPast
                let db = (try? b.resourceValues(forKeys: [.contentModificationDateKey]).contentModificationDate) ?? .distantPast
                return da > db
            }

        guard let latest = minutesFiles.first,
              let text = try? String(contentsOf: latest, encoding: .utf8), !text.isEmpty else { return nil }
        return text
    }

    private static func listExistingFileNames(in folder: URL) -> [String] {
        let fm = FileManager.default
        guard let files = try? fm.contentsOfDirectory(at: folder, includingPropertiesForKeys: nil, options: .skipsHiddenFiles) else { return [] }
        return files
            .filter { $0.pathExtension == "md" }
            .map(\.lastPathComponent)
            .sorted()
    }

    private static func extractFileName(from output: String, fallback: String) -> (String, String) {
        let trimmed = output.trimmingCharacters(in: .whitespacesAndNewlines)
        guard let newlineIndex = trimmed.firstIndex(where: { $0.isNewline }) else {
            return (fallback, trimmed)
        }
        let firstLine = String(trimmed[trimmed.startIndex..<newlineIndex]).trimmingCharacters(in: .whitespaces)
        let rest = String(trimmed[trimmed.index(after: newlineIndex)...]).trimmingCharacters(in: .whitespacesAndNewlines)

        if firstLine.hasSuffix(".md"), !firstLine.contains(" ") || firstLine.count <= 100 {
            let sanitized = firstLine.replacingOccurrences(of: "/", with: "_")
            return (sanitized, rest)
        }
        return (fallback, trimmed)
    }
}
