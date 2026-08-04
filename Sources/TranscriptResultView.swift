import SwiftUI
import UniformTypeIdentifiers

struct TranscriptResultView: View {
    @EnvironmentObject var manager: AudioCaptureManager
    @State private var showCopied = false

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            if manager.isTranscribing || manager.isDiarizing {
                HStack(spacing: 8) {
                    ProgressView()
                        .controlSize(.small)
                    Text(manager.isDiarizing ? "話者識別中..." : "文字起こし中...")
                        .foregroundStyle(.secondary)
                }
            }

            ScrollView {
                VStack(alignment: .leading, spacing: 12) {
                    if manager.isTranscribing {
                        if !manager.sysTranscript.isEmpty {
                            progressSection(label: "システム音声", icon: "speaker.wave.2", text: manager.sysTranscript)
                        }
                        if !manager.micTranscript.isEmpty {
                            progressSection(label: "マイク", icon: "mic", text: manager.micTranscript)
                        }
                        if manager.sysTranscript.isEmpty && manager.micTranscript.isEmpty {
                            Text("処理中...")
                                .foregroundStyle(.secondary)
                                .italic()
                        }
                    } else if !manager.mergedTranscript.isEmpty {
                        ForEach(manager.mergedTranscript) { entry in
                            entryView(entry)
                        }
                    } else if !manager.sysTranscript.isEmpty || !manager.micTranscript.isEmpty {
                        if !manager.micTranscript.isEmpty {
                            progressSection(label: "マイク", icon: "mic", text: manager.micTranscript)
                        }
                        if !manager.sysTranscript.isEmpty {
                            progressSection(label: "システム音声", icon: "speaker.wave.2", text: manager.sysTranscript)
                        }
                    } else {
                        Text("（文字起こし結果がありません）")
                            .foregroundStyle(.secondary)
                            .italic()
                    }
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(4)
            }

            Divider()

            HStack {
                if let url = manager.transcriptFileURL {
                    Label(url.lastPathComponent, systemImage: "doc.text")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
                Spacer()
                Button {
                    copyTranscript()
                    showCopied = true
                    DispatchQueue.main.asyncAfter(deadline: .now() + 2) { showCopied = false }
                } label: {
                    Label(showCopied ? "コピー済み" : "コピー",
                          systemImage: showCopied ? "checkmark" : "doc.on.doc")
                }
                .disabled(manager.isTranscribing || (manager.mergedTranscript.isEmpty && manager.sysTranscript.isEmpty && manager.micTranscript.isEmpty))
                Button("別名で保存...") {
                    saveTranscript()
                }
                .disabled(manager.isTranscribing || (manager.mergedTranscript.isEmpty && manager.sysTranscript.isEmpty && manager.micTranscript.isEmpty))
            }
        }
        .padding(20)
        .frame(minWidth: 500, idealWidth: 600, minHeight: 400, idealHeight: 500)
    }

    @ViewBuilder
    private func progressSection(label: LocalizedStringKey, icon: String, text: String) -> some View {
        VStack(alignment: .leading, spacing: 4) {
            Label(label, systemImage: icon)
                .font(.headline)
            Text(text)
                .font(.body)
                .foregroundStyle(.secondary)
        }
    }

    static let speakerColors: [Color] = [.blue, .green, .orange, .purple, .pink, .cyan, .mint, .indigo]

    static func speakerColor(for id: Int) -> Color {
        speakerColors[abs(id) % speakerColors.count]
    }

    static func speakerLabel(for id: Int) -> String {
        String(localized: "話者\(id + 1)")
    }

    @ViewBuilder
    private func entryView(_ entry: TranscriptEntry) -> some View {
        VStack(alignment: .leading, spacing: 2) {
            HStack(spacing: 6) {
                if let speakerId = entry.speakerId {
                    Image(systemName: "person.fill")
                        .font(.caption)
                        .foregroundStyle(Self.speakerColor(for: speakerId))
                    Text(Self.speakerLabel(for: speakerId))
                        .font(.caption.bold())
                        .foregroundStyle(Self.speakerColor(for: speakerId))
                } else {
                    Image(systemName: entry.source == .system ? "speaker.wave.2" : "mic")
                        .font(.caption)
                        .foregroundStyle(entry.source == .system ? .blue : .green)
                    Text(entry.source.label)
                        .font(.caption.bold())
                        .foregroundStyle(entry.source == .system ? .blue : .green)
                }
                Text(Self.formatTime(entry.startTime))
                    .font(.caption.monospacedDigit())
                    .foregroundStyle(.tertiary)
            }
            Text(entry.text)
                .font(.body)
                .textSelection(.enabled)
            if let translation = entry.translation {
                Text("→ " + translation)
                    .font(.callout)
                    .foregroundStyle(.secondary)
                    .italic()
            }
        }
        .padding(.vertical, 4)
    }

    static func formatTime(_ t: TimeInterval) -> String {
        let m = Int(t) / 60
        let s = Int(t) % 60
        return String(format: "%02d:%02d", m, s)
    }

    private func copyTranscript() {
        let content: String
        if manager.mergedTranscript.isEmpty {
            var lines: [String] = []
            if !manager.micTranscript.isEmpty { lines.append("[\(TranscriptSource.mic.label)]\n\(manager.micTranscript)") }
            if !manager.sysTranscript.isEmpty { lines.append("[\(TranscriptSource.system.label)]\n\(manager.sysTranscript)") }
            content = lines.joined(separator: "\n\n")
        } else {
            content = Self.buildText(entries: manager.mergedTranscript)
        }
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(content, forType: .string)
    }

    private func saveTranscript() {
        let panel = NSSavePanel()
        panel.allowedContentTypes = [.plainText]
        panel.nameFieldStringValue = transcriptFilename()
        panel.canCreateDirectories = true
        if let sysURL = manager.systemAudioURL {
            panel.directoryURL = sysURL.deletingLastPathComponent()
        }
        guard panel.runModal() == .OK, let url = panel.url else { return }
        let content: String
        if manager.mergedTranscript.isEmpty {
            var lines: [String] = []
            if !manager.micTranscript.isEmpty { lines.append("[\(TranscriptSource.mic.label)]\n\(manager.micTranscript)") }
            if !manager.sysTranscript.isEmpty { lines.append("[\(TranscriptSource.system.label)]\n\(manager.sysTranscript)") }
            content = lines.joined(separator: "\n\n")
        } else {
            content = Self.buildText(entries: manager.mergedTranscript)
        }
        try? content.write(to: url, atomically: true, encoding: .utf8)
    }

    static func buildText(entries: [TranscriptEntry]) -> String {
        entries.map { entry in
            let time = formatTime(entry.startTime)
            let label = entry.speakerId.map { speakerLabel(for: $0) } ?? entry.source.label
            return "[\(time)] \(label): \(entry.text)"
        }.joined(separator: "\n")
    }

    private func transcriptFilename() -> String {
        if let sysURL = manager.systemAudioURL {
            let name = sysURL.deletingPathExtension().lastPathComponent
                .replacingOccurrences(of: "system_", with: "transcript_")
            return "\(name).txt"
        }
        return "transcript.txt"
    }
}
