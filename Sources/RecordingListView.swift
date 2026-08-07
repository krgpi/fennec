import Speech
import SwiftUI

struct RecordingListView: View {
    @Binding var selectedSession: RecordingSession?
    @Binding var retranscribeTarget: RecordingSession?
    @Binding var deleteTarget: RecordingSession?
    var transcribingId: String?
    var retranscribePhase: String?

    @EnvironmentObject var store: RecordingStore
    @EnvironmentObject var manager: AudioCaptureManager

    private var groupedSessions: [(title: String, sessions: [RecordingSession])] {
        let calendar = Calendar.current
        let now = Date()
        guard let sevenDaysAgo = calendar.date(byAdding: .day, value: -7, to: now),
              let thirtyDaysAgo = calendar.date(byAdding: .day, value: -30, to: now)
        else { return [] }
        let currentYear = calendar.component(.year, from: now)

        var pastWeek: [RecordingSession] = []
        var pastMonth: [RecordingSession] = []
        var monthBuckets: [(year: Int, month: Int, sessions: [RecordingSession])] = []
        var monthIndex: [Int: Int] = [:]

        for session in store.sessions {
            if session.createdAt >= sevenDaysAgo {
                pastWeek.append(session)
            } else if session.createdAt >= thirtyDaysAgo {
                pastMonth.append(session)
            } else {
                let comps = calendar.dateComponents([.year, .month], from: session.createdAt)
                let key = comps.year! * 100 + comps.month!
                if let idx = monthIndex[key] {
                    monthBuckets[idx].sessions.append(session)
                } else {
                    monthIndex[key] = monthBuckets.count
                    monthBuckets.append((year: comps.year!, month: comps.month!, sessions: [session]))
                }
            }
        }

        var result: [(title: String, sessions: [RecordingSession])] = []
        if !pastWeek.isEmpty {
            result.append((String(localized: "過去7日間"), pastWeek))
        }
        if !pastMonth.isEmpty {
            result.append((String(localized: "過去30日間"), pastMonth))
        }
        for bucket in monthBuckets {
            result.append((Self.monthTitle(year: bucket.year, month: bucket.month, currentYear: currentYear), bucket.sessions))
        }
        return result
    }

    private static func monthTitle(year: Int, month: Int, currentYear: Int) -> String {
        var comps = DateComponents()
        comps.year = year
        comps.month = month
        comps.day = 1
        guard let date = Calendar.current.date(from: comps) else {
            return year == currentYear ? "\(month)月" : "\(year)年\(month)月"
        }
        let formatter = DateFormatter()
        formatter.locale = Locale.current
        formatter.setLocalizedDateFormatFromTemplate(year == currentYear ? "MMMM" : "yyyyMMMM")
        return formatter.string(from: date)
    }

    var body: some View {
        List(selection: $selectedSession) {
            ForEach(groupedSessions, id: \.title) { group in
                Section(group.title) {
                    ForEach(group.sessions) { session in
                        sessionRow(session)
                            .tag(session)
                    }
                }
            }
        }
        .listStyle(.plain)
        .overlay {
            if store.sessions.isEmpty {
                VStack(spacing: 8) {
                    Image(systemName: "waveform.slash")
                        .font(.largeTitle)
                        .foregroundStyle(.secondary)
                    Text("録音データがありません")
                        .foregroundStyle(.secondary)
                }
            }
        }
        .onAppear {
            store.loadSessions()
            if CommandLine.arguments.contains("--snapshot-mode") {
                selectedSession = store.sessions.first
            }
        }
    }

    @ViewBuilder
    private func sessionRow(_ session: RecordingSession) -> some View {
        VStack(alignment: .leading, spacing: 4) {
            Text(session.summary ?? session.displayDate)
                .font(.headline)
                .lineLimit(1)

            HStack(spacing: 6) {
                Text(Self.relativeDate(session.createdAt))
                    .foregroundStyle(.secondary)
                if let preview = session.transcriptPreview {
                    Text(preview)
                        .foregroundStyle(.tertiary)
                        .lineLimit(1)
                }
            }
            .font(.subheadline)

            if transcribingId == session.id || manager.autoTranscribeSessionId == session.id {
                let isAuto = manager.autoTranscribeSessionId == session.id
                let phase = isAuto ? manager.autoTranscribePhase : retranscribePhase

                HStack(spacing: 6) {
                    ProgressView()
                        .controlSize(.small)
                    Text(phase ?? String(localized: "文字起こし中..."))
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                }
            }
        }
        .padding(.vertical, 2)
        .contextMenu {
            if session.hasTranscript {
                Button("再文字起こし") { retranscribeTarget = session }
            } else {
                Button("文字起こし作成") { retranscribeTarget = session }
            }
            Button("Finderで開く") {
                NSWorkspace.shared.selectFile(nil, inFileViewerRootedAtPath: session.folderURL.path)
            }
            Divider()
            Button("削除", role: .destructive) { deleteTarget = session }
        }
    }

    private static func relativeDate(_ date: Date) -> String {
        let calendar = Calendar.current
        if calendar.isDateInToday(date) {
            let formatter = DateFormatter()
            formatter.locale = Locale.current
            formatter.dateFormat = "HH:mm"
            return formatter.string(from: date)
        }
        if calendar.isDateInYesterday(date) {
            return String(localized: "昨日")
        }
        let daysAgo = calendar.dateComponents([.day], from: calendar.startOfDay(for: date), to: calendar.startOfDay(for: Date())).day ?? 0
        if daysAgo < 7 {
            let formatter = DateFormatter()
            formatter.locale = Locale.current
            formatter.dateFormat = "EEEE"
            return formatter.string(from: date)
        }
        let formatter = DateFormatter()
        formatter.locale = Locale.current
        formatter.dateFormat = "yyyy/MM/dd"
        return formatter.string(from: date)
    }
}

struct RetranscribeSheet: View {
    let session: RecordingSession
    let onStart: (TranscriptionEngineType, String?, Bool, Locale) -> Void
    @EnvironmentObject var whisperManager: WhisperModelManager
    @EnvironmentObject var manager: AudioCaptureManager
    @Environment(\.dismiss) private var dismiss
    @State private var selectedEngine: TranscriptionEngineType = .apple
    @State private var selectedModelId: String = ""
    @State private var diarizationEnabled: Bool = false
    @State private var selectedLocale: Locale = .current

    private static func flagEmoji(for locale: Locale) -> String {
        let regionCode: String? = if let region = locale.region?.identifier {
            region
        } else if let lang = locale.language.languageCode?.identifier {
            Locale.Region.isoRegions.first { region in
                Locale(identifier: "\(lang)_\(region.identifier)").language.languageCode?.identifier == lang
            }?.identifier
        } else {
            nil
        }
        guard let code = regionCode, code.count == 2 else { return "🌐" }
        let base: UInt32 = 0x1F1E6 - 65
        return String(code.uppercased().unicodeScalars.compactMap {
            UnicodeScalar(base + $0.value)
        }.map { Character($0) })
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 20) {
            HStack {
                Text("再文字起こし")
                    .font(.title2.bold())
                Spacer()
                Button("キャンセル") { dismiss() }
                    .buttonStyle(.borderless)
            }

            Text(session.displayDate)
                .foregroundStyle(.secondary)

            Picker("言語", selection: $selectedLocale) {
                ForEach(TranscriptionLocale.supported, id: \.identifier) { locale in
                    Text("\(Self.flagEmoji(for: locale)) \(AppLanguage.current.displayLocale.localizedString(forIdentifier: locale.identifier) ?? locale.identifier)")
                        .tag(locale)
                }
            }

            Picker("エンジン", selection: $selectedEngine) {
                Text("Apple（標準）").tag(TranscriptionEngineType.apple)
                Text("Whisper（ローカルAI）").tag(TranscriptionEngineType.whisper)
            }
            .pickerStyle(.segmented)

            if selectedEngine == .whisper {
                let downloaded = whisperManager.downloadedModelIds
                if downloaded.isEmpty {
                    HStack {
                        Image(systemName: "exclamationmark.triangle")
                            .foregroundStyle(.orange)
                        Text("Whisperモデルがダウンロードされていません。設定からダウンロードしてください。")
                            .font(.caption)
                    }
                } else {
                    Picker("モデル", selection: $selectedModelId) {
                        ForEach(WhisperModelInfo.all.filter { downloaded.contains($0.id) }) { model in
                            Text("\(model.name) (\(model.size))").tag(model.id)
                        }
                    }
                }
            }

            #if DEBUG
            Divider()

            VStack(alignment: .leading, spacing: 4) {
                Toggle("話者識別", isOn: $diarizationEnabled)
                Text("AIが話者を自動識別し、誰が話したかをラベル表示します。初回はモデルのダウンロードが必要です。")
                    .font(.callout)
                    .foregroundStyle(.secondary)
            }
            #endif

            Spacer()

            HStack {
                Spacer()
                Button("開始") {
                    onStart(selectedEngine, selectedEngine == .whisper ? selectedModelId : nil, diarizationEnabled, selectedLocale)
                }
                .buttonStyle(.borderedProminent)
                .disabled(selectedEngine == .whisper && (whisperManager.downloadedModelIds.isEmpty || selectedModelId.isEmpty))
            }
        }
        .padding(24)
        .frame(width: 400, height: 380)
        .onAppear {
            selectedEngine = TranscriptionEngineType(rawValue: session.engineType ?? "")
                ?? (whisperManager.isModelReady ? .whisper : .apple)
            selectedModelId = session.modelId ?? whisperManager.selectedModelId
            if let id = session.localeIdentifier {
                selectedLocale = Locale(identifier: id)
            } else {
                selectedLocale = manager.transcriptionLocale
            }
        }
    }
}
