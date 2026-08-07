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
    @State private var saveAsPreset = false
    @State private var availableBackends: [MinutesBackend] = []
    @State private var presetBaseline: PresetSnapshot?
    @State private var showUpdatePresetAlert = false
    @State private var isRenamingPreset = false
    @State private var renameText = ""
    @State private var showDeleteConfirm = false

    private struct PresetSnapshot: Equatable {
        var contextPath: String?
        var outputPath: String?
        var backend: MinutesBackend
        var model: String
    }

    var body: some View {
        Group {
            switch phase {
            case .settings: settingsView
            case .running: runningView
            case .finished: finishedView
            }
        }
        .padding(20)
        .frame(width: 500)
        .alert("プリセットを更新しますか？", isPresented: $showUpdatePresetAlert) {
            Button("更新して生成") { startGeneration(persist: true) }
            Button("更新せずに生成") { startGeneration(persist: false) }
            Button("キャンセル", role: .cancel) {}
        } message: {
            Text("現在の設定はプリセット「\(selectedPreset?.name ?? "")」の内容と異なります。")
        }
        .alert("プリセット名を変更", isPresented: $isRenamingPreset) {
            TextField("プリセット名", text: $renameText)
            Button("キャンセル", role: .cancel) {}
            Button("変更") {
                if let preset = selectedPreset {
                    presetStore.rename(preset, to: renameText)
                    presetName = renameText.trimmingCharacters(in: .whitespacesAndNewlines)
                }
            }
        }
        .alert("プリセットを削除しますか？", isPresented: $showDeleteConfirm) {
            Button("キャンセル", role: .cancel) {}
            Button("削除", role: .destructive) {
                if let preset = selectedPreset {
                    presetStore.delete(preset)
                    selectedPresetId = ""
                }
            }
        } message: {
            Text("プリセット「\(selectedPreset?.name ?? "")」を削除します。現在の設定はそのまま残ります。")
        }
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

    private enum Phase {
        case settings
        case running
        case finished
    }

    private var phase: Phase {
        if generator.isRunning { return .running }
        if generator.outputFileURL != nil || generator.errorMessage != nil { return .finished }
        return .settings
    }

    private var settingsView: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text("議事録を作成")
                .font(.title2.bold())

            if !presetStore.presets.isEmpty {
                HStack {
                    Picker("プリセット", selection: $selectedPresetId) {
                        Text("プリセットを使わない").tag("")
                        Divider()
                        ForEach(presetStore.presets) { preset in
                            Text("\(preset.name) (\(preset.backend.displayName))").tag(preset.id.uuidString)
                        }
                    }
                    .onChange(of: selectedPresetId) { _, newValue in
                        if let preset = presetStore.presets.first(where: { $0.id.uuidString == newValue }) {
                            applyPreset(preset)
                        } else {
                            presetBaseline = nil
                        }
                    }

                    Button {
                        renameText = selectedPreset?.name ?? ""
                        isRenamingPreset = true
                    } label: {
                        Image(systemName: "pencil")
                    }
                    .help("プリセット名を変更")
                    .disabled(selectedPreset == nil)

                    Button {
                        showDeleteConfirm = true
                    } label: {
                        Image(systemName: "trash")
                    }
                    .help("プリセットを削除")
                    .disabled(selectedPreset == nil)
                }
            }

            folderSection(label: "コンテキストフォルダ", url: contextFolderURL) {
                chooseFolder { url in
                    contextFolderURL = url
                    if sameAsContext { outputFolderURL = url }
                    if selectedPreset == nil, presetName.isEmpty {
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

            if outputFolderURL == nil {
                Text("フォルダが未設定の場合、コンテキストなしで生成し録音フォルダに保存します")
                    .font(.caption)
                    .foregroundStyle(.secondary)
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

            if selectedPreset == nil {
                Toggle("この設定をプリセットとして保存する", isOn: $saveAsPreset)
                if saveAsPreset {
                    TextField("プリセット名", text: $presetName)
                        .textFieldStyle(.roundedBorder)
                }
            }

            HStack {
                Spacer()
                Button("閉じる") { dismiss() }
                    .keyboardShortcut(.cancelAction)
                Button("生成") { generateTapped() }
                    .keyboardShortcut(.defaultAction)
            }
        }
    }

    private var runningView: some View {
        VStack(spacing: 16) {
            ProgressView()
                .controlSize(.large)
            Text("議事録を生成しています…")
                .font(.headline)
            Text("完了までしばらくかかります")
                .font(.caption)
                .foregroundStyle(.secondary)
            Button("キャンセル") { generator.cancel() }
        }
        .frame(maxWidth: .infinity)
        .padding(.vertical, 32)
    }

    @ViewBuilder
    private var finishedView: some View {
        VStack(alignment: .leading, spacing: 16) {
            if let url = generator.outputFileURL {
                VStack(spacing: 12) {
                    Image(systemName: "checkmark.circle.fill")
                        .font(.system(size: 40))
                        .foregroundStyle(.green)
                    Text("議事録を保存しました")
                        .font(.headline)
                    Text(url.path.replacingOccurrences(of: NSHomeDirectory(), with: "~"))
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .lineLimit(2)
                        .truncationMode(.middle)
                        .textSelection(.enabled)
                    Button("Finder で開く") {
                        NSWorkspace.shared.selectFile(url.path, inFileViewerRootedAtPath: url.deletingLastPathComponent().path)
                    }
                }
                .frame(maxWidth: .infinity)
                .padding(.vertical, 24)
            } else if let error = generator.errorMessage {
                Text("生成に失敗しました")
                    .font(.headline)
                ScrollView {
                    Text(error)
                        .foregroundStyle(.red)
                        .font(.system(.caption, design: .monospaced))
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .textSelection(.enabled)
                }
                .frame(maxHeight: 200)
            }

            HStack {
                Spacer()
                if generator.outputFileURL == nil {
                    Button("設定に戻る") { generator.errorMessage = nil }
                }
                Button("閉じる") { dismiss() }
                    .keyboardShortcut(.defaultAction)
            }
        }
    }

    private var selectedPreset: MinutesPreset? {
        presetStore.presets.first { $0.id.uuidString == selectedPresetId }
    }

    private var currentSnapshot: PresetSnapshot {
        PresetSnapshot(
            contextPath: contextFolderURL?.path,
            outputPath: outputFolderURL?.path,
            backend: backend,
            model: model
        )
    }

    private var hasPresetChanges: Bool {
        guard let presetBaseline else { return false }
        return presetBaseline != currentSnapshot
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
                presetBaseline = currentSnapshot
            }
        }
    }

    private func generateTapped() {
        if selectedPreset != nil, hasPresetChanges {
            showUpdatePresetAlert = true
        } else {
            startGeneration(persist: true)
        }
    }

    private func startGeneration(persist: Bool) {
        var ctxBookmark: Data?
        var outBookmark: Data?
        if let contextURL = contextFolderURL {
            guard let bookmark = MinutesPreset.createBookmark(for: contextURL) else {
                generator.errorMessage = "フォルダのブックマーク作成に失敗しました"
                return
            }
            ctxBookmark = bookmark
        }
        if let outputURL = outputFolderURL {
            guard let bookmark = MinutesPreset.createBookmark(for: outputURL) else {
                generator.errorMessage = "フォルダのブックマーク作成に失敗しました"
                return
            }
            outBookmark = bookmark
        }

        var preset: MinutesPreset
        var savedPresetId: UUID?
        if let existing = selectedPreset {
            preset = existing
            preset.contextFolderBookmark = ctxBookmark
            preset.outputFolderBookmark = outBookmark
            preset.backend = backend
            preset.model = model
            preset.lastUsedAt = Date()
            if persist {
                presetStore.addOrUpdate(preset)
                presetBaseline = currentSnapshot
                savedPresetId = preset.id
            }
        } else {
            let trimmedName = presetName.trimmingCharacters(in: .whitespacesAndNewlines)
            preset = MinutesPreset(
                name: trimmedName.isEmpty ? (contextFolderURL?.lastPathComponent ?? session.id) : trimmedName,
                contextFolderBookmark: ctxBookmark,
                outputFolderBookmark: outBookmark,
                backend: backend,
                model: model
            )
            if saveAsPreset {
                presetStore.addOrUpdate(preset)
                presetBaseline = currentSnapshot
                savedPresetId = preset.id
                selectedPresetId = preset.id.uuidString
                saveAsPreset = false
            }
        }

        Task {
            await generator.generate(
                transcript: transcript,
                preset: preset,
                sessionDate: session.id,
                defaultOutputFolder: session.folderURL
            )
            if let outputURL = generator.outputFileURL {
                let minutesName = "minutes.md"
                let dest = session.folderURL.appendingPathComponent(minutesName)
                if outputURL != dest {
                    let fm = FileManager.default
                    try? fm.removeItem(at: dest)
                    try? fm.copyItem(at: outputURL, to: dest)
                }
                var updated = session
                updated.minutesFile = minutesName
                updated.minutesPresetId = savedPresetId
                recordingStore.saveMetadata(for: updated)
                recordingStore.loadSessions()
            }
        }
    }
}
