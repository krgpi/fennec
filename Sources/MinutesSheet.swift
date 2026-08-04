import AppKit
import SwiftUI

struct MinutesSheet: View {
    let session: RecordingSession
    let transcript: String
    @ObservedObject var presetStore: MinutesPresetStore
    @ObservedObject var recordingStore: RecordingStore
    @StateObject private var generator = MinutesGenerator()
    @Environment(\.dismiss) private var dismiss

    @State private var selectedPresetId: String = ""
    @State private var contextFolderURL: URL?
    @State private var outputFolderURL: URL?
    @State private var sameAsContext = true
    @State private var backend: MinutesBackend = .claude
    @State private var model = "sonnet"
    @State private var presetName = ""
    @State private var availableBackends: [MinutesBackend] = []

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text("議事録を作成")
                .font(.title2.bold())

            if !presetStore.presets.isEmpty {
                Picker("プリセット", selection: $selectedPresetId) {
                    Text("新規作成").tag("")
                    Divider()
                    ForEach(presetStore.presets) { preset in
                        Text("\(preset.name) (\(preset.backend.displayName))").tag(preset.id.uuidString)
                    }
                }
                .onChange(of: selectedPresetId) { _, newValue in
                    if let preset = presetStore.presets.first(where: { $0.id.uuidString == newValue }) {
                        applyPreset(preset)
                    }
                }
            }

            folderSection(label: "コンテキストフォルダ", url: contextFolderURL) {
                chooseFolder { url in
                    contextFolderURL = url
                    if sameAsContext { outputFolderURL = url }
                    if presetName.isEmpty || selectedPresetId == nil {
                        presetName = url.lastPathComponent
                    }
                }
            }

            Toggle("出力先をコンテキストフォルダと同じにする", isOn: $sameAsContext)
                .onChange(of: sameAsContext) { _, same in
                    if same { outputFolderURL = contextFolderURL }
                }

            if !sameAsContext {
                folderSection(label: "出力先フォルダ", url: outputFolderURL) {
                    chooseFolder { url in outputFolderURL = url }
                }
            }

            if availableBackends.count > 1 {
                Picker("バックエンド", selection: $backend) {
                    ForEach(availableBackends, id: \.self) { b in
                        Text(b.displayName).tag(b)
                    }
                }
                .pickerStyle(.segmented)
                .onChange(of: backend) { _, newBackend in
                    model = newBackend.defaultModel
                }
            }

            Picker("モデル", selection: $model) {
                ForEach(backend.availableModels, id: \.id) { m in
                    Text(m.name).tag(m.id)
                }
            }
            .pickerStyle(.segmented)

            if selectedPresetId == nil {
                TextField("プリセット名", text: $presetName)
                    .textFieldStyle(.roundedBorder)
            }

            if generator.isRunning {
                VStack(alignment: .leading, spacing: 8) {
                    HStack {
                        ProgressView()
                            .controlSize(.small)
                        Text("生成中...")
                            .foregroundStyle(.secondary)
                        Spacer()
                        Button("キャンセル") { generator.cancel() }
                    }
                    if !generator.output.isEmpty {
                        ScrollView {
                            Text(generator.output)
                                .font(.system(.caption, design: .monospaced))
                                .frame(maxWidth: .infinity, alignment: .leading)
                                .textSelection(.enabled)
                        }
                        .frame(maxHeight: 200)
                    }
                }
            }

            if let error = generator.errorMessage {
                ScrollView {
                    Text(error)
                        .foregroundStyle(.red)
                        .font(.system(.caption, design: .monospaced))
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .textSelection(.enabled)
                }
                .frame(maxHeight: 200)
            }

            if let url = generator.outputFileURL {
                HStack {
                    Image(systemName: "checkmark.circle.fill")
                        .foregroundStyle(.green)
                    Text("議事録を保存しました")
                    Spacer()
                    Button("Finder で開く") {
                        NSWorkspace.shared.selectFile(url.path, inFileViewerRootedAtPath: url.deletingLastPathComponent().path)
                    }
                }
            }

            HStack {
                Spacer()
                Button("閉じる") { dismiss() }
                    .keyboardShortcut(.cancelAction)
                Button("生成") { startGeneration() }
                    .keyboardShortcut(.defaultAction)
                    .disabled(!canGenerate)
            }
        }
        .padding(20)
        .frame(width: 500)
        .task {
            let backends = await Task.detached {
                MinutesBackend.availableBackends
            }.value
            availableBackends = backends
            if backend.findCLI() == nil, let first = backends.first {
                backend = first
                model = first.defaultModel
            }
        }
    }

    private var canGenerate: Bool {
        !generator.isRunning && contextFolderURL != nil && outputFolderURL != nil
    }

    @ViewBuilder
    private func folderSection(label: String, url: URL?, action: @escaping () -> Void) -> some View {
        HStack {
            Text(label)
                .frame(width: 140, alignment: .leading)
            Text(url?.path.replacingOccurrences(of: NSHomeDirectory(), with: "~") ?? "未選択")
                .foregroundStyle(url == nil ? .secondary : .primary)
                .lineLimit(1)
                .truncationMode(.middle)
            Spacer()
            Button("選択…", action: action)
        }
    }

    private func chooseFolder(completion: @escaping (URL) -> Void) {
        let panel = NSOpenPanel()
        panel.canChooseFiles = false
        panel.canChooseDirectories = true
        panel.canCreateDirectories = true
        panel.allowsMultipleSelection = false
        guard panel.runModal() == .OK, let url = panel.url else { return }
        completion(url)
    }

    private func applyPreset(_ preset: MinutesPreset) {
        backend = preset.backend
        model = preset.model
        presetName = preset.name
        Task.detached {
            let ctx = preset.resolveContextFolder()
            let out = preset.resolveOutputFolder()
            await MainActor.run {
                contextFolderURL = ctx
                outputFolderURL = out
                if let ctx, let out {
                    sameAsContext = ctx.path == out.path
                } else {
                    sameAsContext = false
                }
            }
        }
    }

    private func startGeneration() {
        guard let contextURL = contextFolderURL,
              let outputURL = outputFolderURL else { return }

        guard let ctxBookmark = MinutesPreset.createBookmark(for: contextURL),
              let outBookmark = MinutesPreset.createBookmark(for: outputURL) else {
            generator.errorMessage = "フォルダのブックマーク作成に失敗しました"
            return
        }

        var preset: MinutesPreset
        if !selectedPresetId.isEmpty, let existing = presetStore.presets.first(where: { $0.id.uuidString == selectedPresetId }) {
            preset = existing
            preset.contextFolderBookmark = ctxBookmark
            preset.outputFolderBookmark = outBookmark
            preset.backend = backend
            preset.model = model
            preset.lastUsedAt = Date()
        } else {
            let name = presetName.isEmpty ? contextURL.lastPathComponent : presetName
            preset = MinutesPreset(
                name: name,
                contextFolderBookmark: ctxBookmark,
                outputFolderBookmark: outBookmark,
                backend: backend,
                model: model
            )
            selectedPresetId = preset.id.uuidString
        }
        presetStore.addOrUpdate(preset)

        Task {
            await generator.generate(
                transcript: transcript,
                preset: preset,
                sessionDate: session.id
            )
            if let outputURL = generator.outputFileURL {
                let minutesName = "minutes.md"
                let dest = session.folderURL.appendingPathComponent(minutesName)
                let fm = FileManager.default
                try? fm.removeItem(at: dest)
                try? fm.copyItem(at: outputURL, to: dest)
                var updated = session
                updated.minutesFile = minutesName
                updated.minutesPresetId = preset.id
                recordingStore.saveMetadata(for: updated)
                recordingStore.loadSessions()
            }
        }
    }
}
