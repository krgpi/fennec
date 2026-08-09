import AppKit
import SwiftUI

private extension View {
    func settingsCaption() -> some View {
        font(.caption)
            .foregroundStyle(.secondary)
            .fixedSize(horizontal: false, vertical: true)
    }
}

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
    @State private var availableBackends: [MinutesBackend]?
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
            case .checking: checkingView
            case .noBackend: noBackendView
            case .settings: settingsView
            case .running: runningView
            case .finished: finishedView
            }
        }
        .padding(20)
        .frame(width: 540)
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
            await detectBackends()
        }
    }

    private enum Phase {
        case checking
        case noBackend
        case settings
        case running
        case finished
    }

    private var phase: Phase {
        if generator.isRunning { return .running }
        if generator.outputFileURL != nil || generator.errorMessage != nil { return .finished }
        guard let availableBackends else { return .checking }
        return availableBackends.isEmpty ? .noBackend : .settings
    }

    private var checkingView: some View {
        VStack(spacing: 12) {
            ProgressView()
            Text("利用できる CLI を確認しています…")
                .font(.caption)
                .foregroundStyle(.secondary)
        }
        .frame(maxWidth: .infinity)
        .padding(.vertical, 40)
    }

    private var noBackendView: some View {
        VStack(alignment: .leading, spacing: 16) {
            VStack(spacing: 10) {
                Image(systemName: "terminal")
                    .font(.system(size: 36))
                    .foregroundStyle(.secondary)
                Text("議事録の生成に使える CLI がありません")
                    .font(.headline)
                Text("議事録の生成には \(MinutesBackend.allCases.map(\.displayName).joined(separator: " / ")) のいずれかが必要です。")
                    .settingsCaption()
                    .multilineTextAlignment(.center)
            }
            .frame(maxWidth: .infinity)
            .padding(.vertical, 24)

            HStack {
                Spacer()
                Button("再確認") {
                    availableBackends = nil
                    Task { await detectBackends() }
                }
                Button("閉じる") { dismiss() }
                    .keyboardShortcut(.defaultAction)
            }
        }
    }

    private func detectBackends() async {
        let backends = await Task.detached {
            MinutesBackend.refreshAvailableBackends()
        }.value
        availableBackends = backends
        if !backends.contains(backend), let first = backends.first {
            backend = first
            model = first.defaultModel
        }
    }

    private var settingsView: some View {
        VStack(alignment: .leading, spacing: 20) {
            Text("議事録を作成")
                .font(.title2.bold())
                .frame(maxWidth: .infinity, alignment: .center)

            if !presetStore.presets.isEmpty {
                settingsRow("プリセット:") {
                    HStack(spacing: 6) {
                        Picker("", selection: $selectedPresetId) {
                            Text("プリセットを使わない").tag("")
                            Divider()
                            ForEach(presetStore.presets) { preset in
                                Text("\(preset.name) (\(preset.backend.displayName))").tag(preset.id.uuidString)
                            }
                        }
                        .labelsHidden()
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

                Divider()
            }

            settingsRow("コンテキスト:") {
                folderField(url: contextFolderURL) {
                    chooseFolder { url in
                        contextFolderURL = url
                        if sameAsContext { outputFolderURL = url }
                        if selectedPreset == nil, presetName.isEmpty {
                            presetName = url.lastPathComponent
                        }
                    }
                }
                Text("議事録生成の参考にするドキュメントのフォルダを選びます。")
                    .settingsCaption()
            }

            settingsRow("出力先:") {
                Toggle("コンテキストフォルダと同じ", isOn: $sameAsContext)
                    .onChange(of: sameAsContext) { _, same in
                        if same { outputFolderURL = contextFolderURL }
                    }
                if !sameAsContext {
                    folderField(url: outputFolderURL) {
                        chooseFolder { url in outputFolderURL = url }
                    }
                }
                if outputFolderURL == nil {
                    Text("未設定の場合、コンテキストなしで生成し録音フォルダに保存します。")
                        .settingsCaption()
                }
            }

            Divider()

            let backends = availableBackends ?? []
            if backends.count > 1 {
                settingsRow("バックエンド:") {
                    Picker("", selection: $backend) {
                        ForEach(backends, id: \.self) { b in
                            Text(b.displayName).tag(b)
                        }
                    }
                    .labelsHidden()
                    .pickerStyle(.segmented)
                    .onChange(of: backend) { _, newBackend in
                        model = newBackend.defaultModel
                    }
                }
            }

            settingsRow("モデル:") {
                Picker("", selection: $model) {
                    ForEach(backend.availableModels, id: \.id) { m in
                        Text(m.name).tag(m.id)
                    }
                }
                .labelsHidden()
                .pickerStyle(.segmented)
            }

            if selectedPreset == nil {
                Divider()

                settingsRow("プリセット保存:") {
                    Toggle("この設定をプリセットとして保存", isOn: $saveAsPreset)
                    if saveAsPreset {
                        TextField("プリセット名", text: $presetName)
                            .textFieldStyle(.roundedBorder)
                    }
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

    @ViewBuilder
    private func settingsRow<Content: View>(
        _ label: LocalizedStringKey,
        @ViewBuilder content: () -> Content
    ) -> some View {
        HStack(alignment: .firstTextBaseline, spacing: 10) {
            Text(label)
                .frame(width: settingsLabelWidth, alignment: .trailing)
            VStack(alignment: .leading, spacing: 6) {
                content()
            }
            .frame(maxWidth: .infinity, alignment: .leading)
        }
    }

    private var settingsLabelWidth: CGFloat { 110 }

    private func folderField(url: URL?, action: @escaping () -> Void) -> some View {
        HStack(spacing: 8) {
            Group {
                if let url {
                    Text(url.path.replacingOccurrences(of: NSHomeDirectory(), with: "~"))
                } else {
                    Text("未選択")
                }
            }
            .foregroundStyle(url == nil ? .secondary : .primary)
                .lineLimit(1)
                .truncationMode(.middle)
            Spacer(minLength: 0)
            Button("選択…", action: action)
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
