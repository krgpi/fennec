import Foundation

struct SessionMetadata: Codable {
    var createdAt: Date
    var systemAudioFile: String?
    var micAudioFile: String?
    var transcriptFile: String?
    var engineType: String?
    var modelId: String?
    var summary: String?
    var localeIdentifier: String?
    var minutesFile: String?
    var minutesPresetId: UUID?
}

struct RecordingSession: Identifiable, Hashable {
    let id: String
    var createdAt: Date
    var folderURL: URL
    var systemAudioFile: String?
    var micAudioFile: String?
    var transcriptFile: String?
    var engineType: String?
    var modelId: String?
    var summary: String?
    var localeIdentifier: String?
    var minutesFile: String?
    var minutesPresetId: UUID?
    var transcriptPreview: String?

    static func == (lhs: RecordingSession, rhs: RecordingSession) -> Bool { lhs.id == rhs.id }
    func hash(into hasher: inout Hasher) { hasher.combine(id) }

    var systemAudioURL: URL? {
        guard let f = systemAudioFile else { return nil }
        return folderURL.appendingPathComponent(f)
    }

    var micAudioURL: URL? {
        guard let f = micAudioFile else { return nil }
        return folderURL.appendingPathComponent(f)
    }

    var transcriptURL: URL? {
        guard let f = transcriptFile else { return nil }
        return folderURL.appendingPathComponent(f)
    }

    var displayDate: String {
        let formatter = DateFormatter()
        formatter.dateFormat = "yyyy/MM/dd HH:mm"
        return formatter.string(from: createdAt)
    }

    var engineLabel: String {
        guard let engine = engineType else { return "—" }
        switch engine {
        case "whisper":
            if let modelId,
               let model = WhisperModelInfo.all.first(where: { $0.id == modelId }) {
                return "Whisper (\(model.name))"
            }
            return "Whisper"
        case "apple":
            return String(localized: "Apple 音声認識")
        default:
            return engine
        }
    }

    var hasTranscript: Bool {
        guard let url = transcriptURL else { return false }
        return FileManager.default.fileExists(atPath: url.path)
    }
}

final class RecordingStore: ObservableObject {
    @Published var sessions: [RecordingSession] = []

    static var baseDirectory: URL {
        StorageLocation.baseDirectory
    }

    private static let isSnapshotMode = CommandLine.arguments.contains("--snapshot-mode")

    func loadSessions() {
        if Self.isSnapshotMode {
            loadSnapshotSessions()
            return
        }

        var results: [RecordingSession] = []
        let fm = FileManager.default

        try? fm.createDirectory(at: Self.baseDirectory, withIntermediateDirectories: true)

        guard let contents = try? fm.contentsOfDirectory(
            at: Self.baseDirectory,
            includingPropertiesForKeys: [.isDirectoryKey],
            options: [.skipsHiddenFiles]
        ) else { return }

        var folderSessionIds = Set<String>()

        for url in contents {
            var isDir: ObjCBool = false
            guard fm.fileExists(atPath: url.path, isDirectory: &isDir), isDir.boolValue else { continue }

            let metadataURL = url.appendingPathComponent("session.json")
            guard let data = try? Data(contentsOf: metadataURL) else { continue }

            let decoder = JSONDecoder()
            decoder.dateDecodingStrategy = .iso8601
            guard let metadata = try? decoder.decode(SessionMetadata.self, from: data) else { continue }

            var session = RecordingSession(
                id: url.lastPathComponent,
                createdAt: metadata.createdAt,
                folderURL: url,
                systemAudioFile: metadata.systemAudioFile,
                micAudioFile: metadata.micAudioFile,
                transcriptFile: metadata.transcriptFile,
                engineType: metadata.engineType,
                modelId: metadata.modelId,
                summary: metadata.summary,
                localeIdentifier: metadata.localeIdentifier,
                minutesFile: metadata.minutesFile,
                minutesPresetId: metadata.minutesPresetId
            )
            session.transcriptPreview = Self.loadTranscriptPreview(url: session.transcriptURL)
            results.append(session)
            folderSessionIds.insert(url.lastPathComponent)
        }

        for url in contents {
            guard url.pathExtension == "m4a",
                  url.lastPathComponent.hasPrefix("system_") else { continue }

            let ts = url.deletingPathExtension().lastPathComponent
                .replacingOccurrences(of: "system_", with: "")
            guard !folderSessionIds.contains(ts) else { continue }

            let micFile = "mic_\(ts).m4a"
            let transcriptFile = "transcript_\(ts).txt"

            var session = RecordingSession(
                id: ts,
                createdAt: Self.parseTimestamp(ts) ?? Date(),
                folderURL: Self.baseDirectory,
                systemAudioFile: url.lastPathComponent,
                micAudioFile: fm.fileExists(atPath: Self.baseDirectory.appendingPathComponent(micFile).path) ? micFile : nil,
                transcriptFile: fm.fileExists(atPath: Self.baseDirectory.appendingPathComponent(transcriptFile).path) ? transcriptFile : nil
            )
            session.transcriptPreview = Self.loadTranscriptPreview(url: session.transcriptURL)
            results.append(session)
        }

        results.sort { $0.createdAt > $1.createdAt }
        sessions = results
    }

    func saveMetadata(for session: RecordingSession) {
        let metadata = SessionMetadata(
            createdAt: session.createdAt,
            systemAudioFile: session.systemAudioFile,
            micAudioFile: session.micAudioFile,
            transcriptFile: session.transcriptFile,
            engineType: session.engineType,
            modelId: session.modelId,
            summary: session.summary,
            localeIdentifier: session.localeIdentifier,
            minutesFile: session.minutesFile,
            minutesPresetId: session.minutesPresetId
        )
        let encoder = JSONEncoder()
        encoder.dateEncodingStrategy = .iso8601
        encoder.outputFormatting = .prettyPrinted
        guard let data = try? encoder.encode(metadata) else { return }
        try? data.write(to: session.folderURL.appendingPathComponent("session.json"), options: .atomic)
    }

    func deleteSession(_ session: RecordingSession) {
        let fm = FileManager.default
        if session.folderURL != Self.baseDirectory {
            try? fm.trashItem(at: session.folderURL, resultingItemURL: nil)
        } else {
            if let url = session.systemAudioURL { try? fm.trashItem(at: url, resultingItemURL: nil) }
            if let url = session.micAudioURL { try? fm.trashItem(at: url, resultingItemURL: nil) }
            if let url = session.transcriptURL { try? fm.trashItem(at: url, resultingItemURL: nil) }
        }
        loadSessions()
    }

    private static func parseTimestamp(_ ts: String) -> Date? {
        let formatter = DateFormatter()
        formatter.dateFormat = "yyyyMMdd_HHmmss"
        return formatter.date(from: ts)
    }

    private func loadSnapshotSessions() {
        let fm = FileManager.default
        let base = fm.temporaryDirectory.appendingPathComponent("snapshot_sessions")
        try? fm.removeItem(at: base)
        try? fm.createDirectory(at: base, withIntermediateDirectories: true)

        let now = Date()
        let cal = Calendar.current

        let defs: [(String, String, Date, String, String?)] = [
            ("Zoomチーム定例 - Q3ロードマップ", "apple", cal.date(byAdding: .hour, value: -3, to: now)!,
             """
             [00:00] PC音声: それでは定例を始めましょう。まずQ3のロードマップ確認からいきます。
             [00:12] マイク: はい、UIリニューアルは予定通り進んでいます。来週デザインレビューの準備ができます。
             [00:28] PC音声: いいですね。あと海外展開のスケジュールはどうですか？
             [00:38] マイク: APAC地域は9月リリースを目標にしています。ローカライゼーションは8割完了です。
             [00:55] PC音声: 了解です。次にパフォーマンス改善について山田さんお願いします。
             [01:05] PC音声: APIレスポンスタイムを200ms以下にするのが目標です。現在のボトルネックはデータベースクエリで、インデックス最適化で改善できそうです。
             [01:30] マイク: テスト環境では50%の改善が確認できています。来週本番に適用する予定です。
             [01:45] PC音声: 素晴らしい。ユーザーからのフィードバックも良好で、NPSが先月から5ポイント上がっています。
             [02:00] マイク: 引き続き品質を維持していきましょう。
             [02:15] PC音声: はい、では次回は来週水曜日の同じ時間で。お疲れさまでした。
             """, nil),
            ("Discord ゲーム実況の企画会議", "apple", cal.date(byAdding: .hour, value: -27, to: now)!, "", nil),
            ("英語ミーティング - プロダクト戦略", "whisper", cal.date(byAdding: .hour, value: -32, to: now)!, "", "openai_whisper-large-v3-v20240930_626MB"),
            ("LINE通話 - 来週の予定相談", "apple", cal.date(byAdding: .hour, value: -52, to: now)!, "", nil),
            ("Google Meet デザインレビュー", "whisper", cal.date(byAdding: .day, value: -3, to: now)!, "", "openai_whisper-large-v3-v20240930_626MB"),
            ("クライアント提案レビュー", "apple", cal.date(byAdding: .day, value: -4, to: now)!, "", nil),
            ("1on1 キャリア相談", "apple", cal.date(byAdding: .day, value: -5, to: now)!, "", nil),
            ("FaceTime 家族会議", "apple", cal.date(byAdding: .day, value: -6, to: now)!, "", nil),
            ("Teams 全社ミーティング", "whisper", cal.date(byAdding: .day, value: -8, to: now)!, "", "openai_whisper-large-v3-v20240930_626MB"),
            ("WebEx パートナー商談", "apple", cal.date(byAdding: .day, value: -10, to: now)!, "", nil),
        ]

        var results: [RecordingSession] = []
        let formatter = DateFormatter()
        formatter.dateFormat = "yyyyMMdd_HHmmss"
        let encoder = JSONEncoder()
        encoder.dateEncodingStrategy = .iso8601
        encoder.outputFormatting = .prettyPrinted

        for (summary, engine, date, transcript, modelId) in defs {
            let folderId = formatter.string(from: date)
            let folder = base.appendingPathComponent(folderId)
            try? fm.createDirectory(at: folder, withIntermediateDirectories: true)

            let meta = SessionMetadata(
                createdAt: date,
                systemAudioFile: "system.m4a",
                micAudioFile: "mic.m4a",
                transcriptFile: transcript.isEmpty ? nil : "transcript.txt",
                engineType: engine,
                modelId: modelId,
                summary: summary,
                localeIdentifier: "ja_JP"
            )
            if let data = try? encoder.encode(meta) {
                try? data.write(to: folder.appendingPathComponent("session.json"))
            }

            if !transcript.isEmpty {
                try? transcript.write(to: folder.appendingPathComponent("transcript.txt"), atomically: true, encoding: .utf8)
            }

            var session = RecordingSession(
                id: folderId,
                createdAt: date,
                folderURL: folder,
                systemAudioFile: "system.m4a",
                micAudioFile: "mic.m4a",
                transcriptFile: transcript.isEmpty ? nil : "transcript.txt",
                engineType: engine,
                modelId: modelId,
                summary: summary,
                localeIdentifier: "ja_JP"
            )
            session.transcriptPreview = Self.loadTranscriptPreview(url: session.transcriptURL)
            results.append(session)
        }

        results.sort { $0.createdAt > $1.createdAt }
        sessions = results
    }

    private static func loadTranscriptPreview(url: URL?) -> String? {
        guard let url, let handle = try? FileHandle(forReadingFrom: url) else { return nil }
        defer { try? handle.close() }
        guard let data = try? handle.read(upToCount: 500),
              let text = String(data: data, encoding: .utf8) else { return nil }
        for line in text.split(separator: "\n", omittingEmptySubsequences: true) {
            let str = String(line)
            if let range = str.range(of: #"^\[\d{2}:\d{2}\]\s+.+?:\s+"#, options: .regularExpression) {
                let content = String(str[range.upperBound...])
                if !content.isEmpty { return String(content.prefix(80)) }
            } else {
                return String(str.prefix(80))
            }
        }
        return nil
    }
}
