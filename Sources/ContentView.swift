import AVFoundation
import CoreAudio
import SwiftUI

private final class CancellationFlag: @unchecked Sendable {
    var isCancelled = false
}

struct ContentView: View {
    @EnvironmentObject var manager: AudioCaptureManager
    @EnvironmentObject var whisperManager: WhisperModelManager
    @EnvironmentObject var recordingStore: RecordingStore
    @EnvironmentObject var translationService: TranslationService
    @EnvironmentObject var audioPlayer: AudioPlayer
    @State private var selectedSession: RecordingSession?
    @State private var showEngineSettings = false
    @State private var transcriptText: String?
    @State private var parsedEntries: [TranscriptEntry] = []
    @State private var showCopied = false
    @State private var retranscribeTarget: RecordingSession?
    @State private var transcribingId: String?
    @State private var retranscribePhase: String?
    @State private var retranscribeProgress: Double?
    @State private var retranscribeTask: Task<Void, Never>?
    @State private var activeTranscriber: Transcriber?
    @State private var cancelFlag = CancellationFlag()
    @State private var deleteTarget: RecordingSession?
    @State private var minutesTarget: RecordingSession?
    @StateObject private var minutesGenerator = MinutesGenerator()
    @ObservedObject private var minutesPresetStore = MinutesPresetStore.shared
    @State private var isHoveringRecordButton = false
    @State private var selectedDetailTab: DetailTab = .minutes
    @State private var minutesText: String?
    @State private var showMinutesCopied = false
    @State private var editingTitle: String = ""
    @FocusState private var isTitleFocused: Bool
    @State private var titleSaveTask: Task<Void, Never>?
    @State private var noteText = ""
    @State private var savedNoteText = ""
    @State private var noteSaveTask: Task<Void, Never>?

    private enum DetailTab: Hashable {
        case note
        case transcript
        case minutes
    }

    private var defaultMicLabel: String {
        if let name = manager.defaultInputDeviceName {
            return String(localized: "デフォルト (\(name))")
        }
        return String(localized: "デフォルト")
    }

    private var micPickerBinding: Binding<AudioDeviceID?> {
        Binding(
            get: { manager.useSystemDefaultMic ? nil : manager.selectedDeviceID },
            set: { newValue in
                if let deviceID = newValue {
                    manager.useSystemDefaultMic = false
                    manager.selectedDeviceID = deviceID
                } else {
                    manager.useSystemDefaultMic = true
                }
            }
        )
    }

    var body: some View {
        navigationView
            .onChange(of: manager.stoppedBySilenceDetection) { _, stopped in
                if stopped {
                    recordingStore.loadSessions()
                    manager.stoppedBySilenceDetection = false
                }
            }
            .onChange(of: manager.selectedDeviceID) { _, newValue in
                if manager.isRecording, let deviceID = newValue {
                    manager.switchMicrophone(to: deviceID)
                }
            }
            .onChange(of: manager.isRecording) { _, recording in
                if recording {
                    recordingStore.loadSessions()
                    if let id = manager.recordingSessionId,
                       let session = recordingStore.sessions.first(where: { $0.id == id }) {
                        selectedSession = session
                    }
                } else {
                    flushNote(for: selectedSession)
                    recordingStore.loadSessions()
                    updateSelectedSession()
                }
            }
            .onChange(of: selectedSession) { oldSession, newSession in
                saveTitleChange(for: oldSession)
                flushNote(for: oldSession)
                updateSelectedSession()
                editingTitle = selectedSession?.summary ?? ""
                loadTranscript()
                loadNote()
                showCopied = false
                showMinutesCopied = false
                if isLiveRecording(newSession) {
                    selectedDetailTab = .note
                } else if transcribingId == newSession?.id || manager.autoTranscribeSessionId == newSession?.id {
                    selectedDetailTab = .transcript
                } else if minutesText != nil {
                    selectedDetailTab = .minutes
                } else if transcriptText != nil {
                    selectedDetailTab = .transcript
                } else {
                    selectedDetailTab = .minutes
                }
            }
            .onChange(of: manager.autoTranscribeSessionId) { _, newValue in
                if newValue == nil {
                    recordingStore.loadSessions()
                    updateSelectedSession()
                    loadTranscript()
                    if selectedDetailTab != .note {
                        if minutesText != nil {
                            selectedDetailTab = .minutes
                        } else if transcriptText != nil {
                            selectedDetailTab = .transcript
                        }
                    }
                }
            }
    }

    private var navigationView: some View {
        HSplitView {
            RecordingListView(
                selectedSession: $selectedSession,
                retranscribeTarget: $retranscribeTarget,
                deleteTarget: $deleteTarget,
                onRename: {
                    Task { @MainActor in
                        isTitleFocused = true
                    }
                },
                transcribingId: transcribingId,
                retranscribePhase: retranscribePhase
            )
            .frame(minWidth: 140, idealWidth: 190, maxWidth: 400)
            detailPane
                .frame(minWidth: 350)
                .background {
                    Color.clear
                        .contentShape(Rectangle())
                        .onTapGesture { isTitleFocused = false }
                }
        }
        .frame(minWidth: 700, minHeight: 500)
        .toolbar { toolbarContent }
        .sheet(isPresented: $showEngineSettings) {
            #if DEBUG
            EngineSettingsView(diarizer: manager.speakerDiarizer)
                .environmentObject(whisperManager)
                .environmentObject(manager)
            #else
            EngineSettingsView()
                .environmentObject(whisperManager)
                .environmentObject(manager)
            #endif
        }
        .sheet(item: $retranscribeTarget) { session in
            RetranscribeSheet(session: session) { engineType, modelId, diarize, locale in
                retranscribeTarget = nil
                cancelFlag = CancellationFlag()
                retranscribeTask = Task {
                    await retranscribe(session: session, engineType: engineType, modelId: modelId, diarize: diarize, locale: locale)
                }
            }
            .environmentObject(whisperManager)
            .environmentObject(manager)
        }
        .sheet(item: $minutesTarget, onDismiss: {
            updateSelectedSession()
            loadMinutes()
            if minutesText != nil {
                selectedDetailTab = .minutes
            }
        }) { session in
            if let url = session.transcriptURL,
               let text = try? String(contentsOf: url, encoding: .utf8) {
                MinutesSheet(session: session, transcript: text, generator: minutesGenerator, presetStore: minutesPresetStore, recordingStore: recordingStore)
            }
        }
        .onChange(of: minutesGenerator.isRunning) { _, running in
            if !running, minutesGenerator.outputFileURL != nil {
                recordingStore.loadSessions()
                updateSelectedSession()
                loadMinutes()
                selectedDetailTab = .minutes
            }
        }
        .onReceive(NotificationCenter.default.publisher(for: .openSettings)) { _ in
            showEngineSettings = true
        }
        .onReceive(NotificationCenter.default.publisher(for: NSWindow.didResignKeyNotification)) { _ in
            saveTitleChange(for: selectedSession)
            flushNote(for: selectedSession)
        }
        .onReceive(NotificationCenter.default.publisher(for: NSApplication.willTerminateNotification)) { _ in
            saveTitleChange(for: selectedSession)
            flushNote(for: selectedSession)
        }
        .task {
            if CommandLine.arguments.contains("--snapshot-settings") {
                try? await Task.sleep(for: .milliseconds(500))
                showEngineSettings = true
            }
        }
        .alert("マイク入力なし", isPresented: $manager.micNoInputWarning) {
            Button("OK", role: .cancel) {}
        } message: {
            Text("マイクからの入力が検出されていません。マイクの接続や選択を確認してください")
        }
        .alert("マイク使用を検知", isPresented: $manager.showMicStartAlert) {
            Button("録音開始") {
                Task { await manager.startRecording() }
            }
            Button("無視", role: .cancel) {}
        } message: {
            Text("他のアプリがマイクを使用し始めました。\n録音を開始しますか？")
        }
        .alert("マイク使用の終了を検知", isPresented: $manager.showMicStopAlert) {
            Button("録音停止") {
                Task {
                    await manager.stopRecording()
                    recordingStore.loadSessions()
                }
            }
            Button("続行", role: .cancel) {}
        } message: {
            Text("他のアプリがマイクの使用を停止しました。\n録音を停止しますか？")
        }
        .alert("この録音を削除しますか？", isPresented: .init(
            get: { deleteTarget != nil },
            set: { if !$0 { deleteTarget = nil } }
        )) {
            Button("削除", role: .destructive) {
                if let target = deleteTarget {
                    if selectedSession?.id == target.id {
                        selectedSession = nil
                        transcriptText = nil
                    }
                    recordingStore.deleteSession(target)
                    deleteTarget = nil
                }
            }
            Button("キャンセル", role: .cancel) { deleteTarget = nil }
        } message: {
            Text("音声ファイルと文字起こしデータがゴミ箱に移動されます。")
        }
    }

    // MARK: - Toolbar

    @ToolbarContentBuilder
    private var toolbarContent: some ToolbarContent {
        ToolbarItemGroup(placement: .primaryAction) {
            Button {
                Task {
                    if manager.isRecording {
                        await manager.stopRecording()
                        recordingStore.loadSessions()
                        selectedSession = recordingStore.sessions.first
                    } else {
                        await manager.startRecording()
                    }
                }
            } label: {
                HStack(spacing: 4) {
                    if manager.isRecording {
                        Image(systemName: isHoveringRecordButton ? "stop.circle.fill" : "record.circle.fill")
                            .symbolEffect(.pulse, options: .repeating, isActive: !isHoveringRecordButton)
                        Text(isHoveringRecordButton ? "停止" : "録音中")
                    } else {
                        Image(systemName: "record.circle")
                        Text("録音")
                    }
                }
                .foregroundStyle(manager.isRecording ? Color.red : Color.accentColor)
            }
            .disabled(manager.selectedDeviceID == nil)
            .onHover { isHoveringRecordButton = $0 }
        }

        ToolbarItemGroup(placement: .automatic) {
            Picker("マイク", selection: micPickerBinding) {
                Text(defaultMicLabel)
                    .tag(nil as AudioDeviceID?)
                Divider()
                ForEach(manager.inputDevices) { device in
                    Text(device.name).tag(device.id as AudioDeviceID?)
                }
            }
            .pickerStyle(.menu)
            .labelsHidden()

            Button { showEngineSettings = true } label: {
                Image(systemName: "gearshape")
            }
        }
    }

    // MARK: - Detail Pane

    @ViewBuilder
    private var detailPane: some View {
        Group {
            if manager.isRecording || selectedSession != nil {
                ScrollView {
                    VStack(alignment: .leading, spacing: 16) {
                        if manager.isRecording {
                            recordingStatusSection
                        }
                        if let session = selectedSession {
                            sessionDetailSection(session)
                        }
                    }
                    .padding(20)
                    .frame(maxWidth: .infinity, alignment: .leading)
                }
            } else {
                VStack(spacing: 12) {
                    Image(systemName: "waveform")
                        .font(.system(size: 48))
                        .foregroundStyle(.quaternary)
                    Text("録音を選択するか、録音を開始してください")
                        .foregroundStyle(.secondary)
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity)
            }
        }
        .navigationTitle("")
    }

    // MARK: - Recording Status

    @ViewBuilder
    private var recordingStatusSection: some View {
        VStack(alignment: .leading, spacing: 12) {
            microphoneModeView

            HStack {
                Text(formatDuration(manager.duration))
                    .font(.system(.title2, design: .monospaced))
                    .foregroundStyle(.red)
                Spacer()
                VStack(spacing: 4) {
                    LevelMeter(level: CGFloat(manager.sysAudioLevel), icon: "speaker.wave.2")
                    LevelMeter(level: CGFloat(manager.micAudioLevel), icon: "mic")
                }
                .frame(width: 200)
            }

            if manager.micNoInputWarning {
                HStack(spacing: 8) {
                    Image(systemName: "exclamationmark.triangle.fill")
                        .foregroundStyle(.orange)
                    Text("マイクからの入力が検出されていません。マイクの接続や選択を確認してください")
                        .font(.callout)
                        .foregroundStyle(.orange)
                }
            }

            liveTranscriptView

            if let error = manager.errorMessage {
                HStack(alignment: .top, spacing: 6) {
                    Text(error)
                        .foregroundStyle(.red)
                        .font(.callout)
                        .textSelection(.enabled)
                    Button {
                        NSPasteboard.general.clearContents()
                        NSPasteboard.general.setString(error, forType: .string)
                    } label: {
                        Image(systemName: "doc.on.doc")
                            .foregroundStyle(.secondary)
                    }
                    .buttonStyle(.plain)
                    .help("エラーメッセージをコピー")
                }
            }
        }
        .padding()
        .background(.regularMaterial, in: RoundedRectangle(cornerRadius: 8))

        if selectedSession != nil {
            Divider()
        }
    }

    // MARK: - Session Detail

    @ViewBuilder
    private func sessionDetailSection(_ session: RecordingSession) -> some View {
        VStack(alignment: .leading, spacing: 2) {
            TextField(session.displayDate, text: $editingTitle)
                .font(.title2.bold())
                .textFieldStyle(.plain)
                .focused($isTitleFocused)
                .onSubmit { isTitleFocused = false }
                .onChange(of: editingTitle) { _, _ in scheduleTitleSave(for: session) }
                .onChange(of: isTitleFocused) { _, focused in
                    if !focused { saveTitleChange(for: session) }
                }
            HStack(spacing: 8) {
                Text(session.displayDate)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                Text(session.engineLabel)
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
        }

        if !isLiveRecording(session) {
            HStack(spacing: 6) {
                playButton(for: session)

                Spacer()

                Menu {
                    if session.hasTranscript && session.minutesFile != nil {
                        Button { minutesTarget = session } label: {
                            Label("議事録を再生成", systemImage: "doc.text")
                        }
                    }

                    Button { retranscribeTarget = session } label: {
                        Label(session.hasTranscript ? "再文字起こし" : "文字起こし作成", systemImage: "arrow.clockwise")
                    }

                    Button {
                        NSWorkspace.shared.selectFile(nil, inFileViewerRootedAtPath: session.folderURL.path)
                    } label: {
                        Label("フォルダを開く", systemImage: "folder")
                    }

                    Divider()

                    Button(role: .destructive) { deleteTarget = session } label: {
                        Label("削除", systemImage: "trash")
                    }
                } label: {
                    Image(systemName: "ellipsis.circle")
                }
                .menuStyle(.borderlessButton)
                .fixedSize()
            }
            .buttonStyle(.accessoryBar)
        }

        Divider()

        playbackBar(for: session)

        if transcribingId == session.id || manager.autoTranscribeSessionId == session.id {
            retranscriptionProgress(for: session)
        }

        HStack {
            Spacer()
            Picker("", selection: $selectedDetailTab) {
                Text("メモ").tag(DetailTab.note)
                Text("議事録").tag(DetailTab.minutes)
                Text("文字起こし").tag(DetailTab.transcript)
            }
            .pickerStyle(.segmented)
            .labelsHidden()
            .frame(maxWidth: 360)
            Spacer()
        }

        switch selectedDetailTab {
        case .note:
            noteContent(for: session)
        case .minutes:
            minutesContent(for: session)
        case .transcript:
            transcriptContent(for: session)
        }
    }

    // MARK: - Playback

    @ViewBuilder
    private func playButton(for session: RecordingSession) -> some View {
        let isThisPlaying = audioPlayer.playingSessionId == session.id
        if session.systemAudioURL != nil || session.micAudioURL != nil {
            Button {
                if isThisPlaying {
                    audioPlayer.togglePlayPause()
                } else {
                    audioPlayer.play(systemURL: session.systemAudioURL, micURL: session.micAudioURL, sessionId: session.id)
                }
            } label: {
                Label(isThisPlaying && audioPlayer.isPlaying ? "一時停止" : "再生",
                      systemImage: isThisPlaying && audioPlayer.isPlaying ? "pause.fill" : "play.fill")
            }
        }
    }

    @ViewBuilder
    private func playbackBar(for session: RecordingSession) -> some View {
        if audioPlayer.playingSessionId == session.id {
            VStack(spacing: 4) {
                Slider(
                    value: Binding(
                        get: { audioPlayer.currentTime },
                        set: { audioPlayer.seek(to: $0) }
                    ),
                    in: 0...max(audioPlayer.duration, 1)
                )
                HStack {
                    Text(formatPlaybackTime(audioPlayer.currentTime))
                        .font(.caption2)
                        .foregroundStyle(.secondary)
                        .monospacedDigit()
                    Spacer()
                    if session.systemAudioURL != nil && session.micAudioURL != nil {
                        Toggle("システム", isOn: Binding(
                            get: { audioPlayer.systemEnabled },
                            set: { audioPlayer.setSystemEnabled($0) }
                        ))
                        .toggleStyle(.checkbox)
                        .font(.caption2)
                        Toggle("マイク", isOn: Binding(
                            get: { audioPlayer.micEnabled },
                            set: { audioPlayer.setMicEnabled($0) }
                        ))
                        .toggleStyle(.checkbox)
                        .font(.caption2)
                    }
                    Text(formatPlaybackTime(audioPlayer.duration))
                        .font(.caption2)
                        .foregroundStyle(.secondary)
                        .monospacedDigit()
                    Button {
                        audioPlayer.stop()
                    } label: {
                        Image(systemName: "stop.fill")
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }
                    .buttonStyle(.plain)
                }
            }
        }
    }

    @ViewBuilder
    private func retranscriptionProgress(for session: RecordingSession) -> some View {
        let isAuto = manager.autoTranscribeSessionId == session.id
        let phase = isAuto ? manager.autoTranscribePhase : retranscribePhase
        let progress = isAuto ? manager.autoTranscribeProgress : retranscribeProgress

        HStack(spacing: 8) {
            VStack(alignment: .leading, spacing: 3) {
                Text(phase ?? String(localized: "準備中..."))
                    .font(.caption)
                    .foregroundStyle(.secondary)
                if let progress {
                    ProgressView(value: progress)
                        .progressViewStyle(.linear)
                } else {
                    ProgressView()
                        .progressViewStyle(.linear)
                }
            }
            Button {
                if isAuto {
                    manager.cancelAutoTranscription()
                } else {
                    cancelRetranscription()
                }
            } label: {
                Image(systemName: "xmark.circle.fill")
                    .foregroundStyle(.secondary)
            }
            .buttonStyle(.plain)
        }
    }

    // MARK: - Microphone Mode

    @ViewBuilder
    private var microphoneModeView: some View {
        let isVoiceIsolation = manager.microphoneMode == .voiceIsolation
        HStack(spacing: 8) {
            Image(systemName: isVoiceIsolation ? "checkmark.circle.fill" : "exclamationmark.triangle.fill")
                .foregroundStyle(isVoiceIsolation ? .green : .orange)
            let label: String = switch manager.microphoneMode {
            case .standard: String(localized: "標準")
            case .wideSpectrum: String(localized: "ワイドスペクトル")
            case .voiceIsolation: String(localized: "声を分離")
            @unknown default: String(localized: "不明")
            }
            Text("マイクモード: \(label)")
                .font(.callout)
            if !isVoiceIsolation {
                Text("メニューバーのマイクアイコンから変更できます")
                    .font(.callout)
                    .foregroundStyle(.secondary)
            }
        }
        .onAppear {
            manager.refreshMicrophoneMode()
        }
    }

    // MARK: - Live Transcript

    @ViewBuilder
    private var liveTranscriptView: some View {
        if manager.liveTranscriptionEnabled {
            Divider()
            ScrollViewReader { proxy in
                ScrollView {
                    VStack(alignment: .leading, spacing: 8) {
                        if manager.micTranscript.isEmpty && manager.sysTranscript.isEmpty {
                            Text("リアルタイム文字起こし待機中...")
                                .font(.callout)
                                .foregroundStyle(.secondary)
                        }
                        if !manager.micTranscript.isEmpty {
                            Label("マイク", systemImage: "mic")
                                .font(.caption.bold())
                                .foregroundStyle(.green)
                            liveTranscriptText(manager.micTranscript)
                            if !translationService.micTranscriptTranslation.isEmpty {
                                Text("→ " + translationService.micTranscriptTranslation)
                                    .font(.callout)
                                    .foregroundStyle(.secondary)
                                    .italic()
                            }
                        }
                        if !manager.sysTranscript.isEmpty {
                            Label("PC音声", systemImage: "speaker.wave.2")
                                .font(.caption.bold())
                                .foregroundStyle(.blue)
                            liveTranscriptText(manager.sysTranscript, secondary: true)
                            if !translationService.sysTranscriptTranslation.isEmpty {
                                Text("→ " + translationService.sysTranscriptTranslation)
                                    .font(.callout)
                                    .foregroundStyle(.secondary)
                                    .italic()
                            }
                        }
                        Color.clear.frame(height: 1).id("bottom")
                    }
                    .frame(maxWidth: .infinity, alignment: .leading)
                }
                .onChange(of: manager.micTranscript) { _, _ in
                    proxy.scrollTo("bottom")
                }
                .onChange(of: manager.sysTranscript) { _, _ in
                    proxy.scrollTo("bottom")
                }
            }
            .frame(maxHeight: 200)
            .task(id: manager.micTranscript + manager.sysTranscript) {
                translationService.debouncedTranslateLive(sysText: manager.sysTranscript, micText: manager.micTranscript)
            }
        }
    }

    @ViewBuilder
    private func liveTranscriptText(_ text: String, secondary: Bool = false) -> some View {
        let paragraphs = Self.splitIntoParagraphs(text)
        VStack(alignment: .leading, spacing: 8) {
            ForEach(Array(paragraphs.enumerated()), id: \.offset) { _, para in
                Text(para)
                    .font(.callout)
                    .lineSpacing(3)
                    .foregroundStyle(secondary ? .secondary : .primary)
                    .textSelection(.enabled)
            }
        }
    }

    // MARK: - Helpers

    private func loadTranscript() {
        guard let url = selectedSession?.transcriptURL else {
            transcriptText = nil
            parsedEntries = []
            minutesText = nil
            return
        }
        let text = try? String(contentsOf: url, encoding: .utf8)
        transcriptText = text
        parsedEntries = text.map { Self.parseTranscriptText($0) } ?? []
        loadMinutes()
    }

    private func loadMinutes() {
        guard let session = selectedSession,
              let minutesFile = session.minutesFile else {
            minutesText = nil
            return
        }
        let url = session.folderURL.appendingPathComponent(minutesFile)
        minutesText = try? String(contentsOf: url, encoding: .utf8)
    }

    private static let transcriptLineRegex = /^\[(\d{2}:\d{2})\]\s+(.+?):\s+(.+)$/

    private static func parseTranscriptText(_ text: String) -> [TranscriptEntry] {
        text.split(separator: "\n", omittingEmptySubsequences: false).compactMap { line in
            guard let match = String(line).wholeMatch(of: transcriptLineRegex) else { return nil }
            let timeParts = match.1.split(separator: ":")
            let seconds = (Double(timeParts[0]) ?? 0) * 60 + (Double(timeParts[1]) ?? 0)
            let label = String(match.2)
            let entryText = String(match.3)

            var speakerId: Int?
            var source: TranscriptSource = .system
            if label.hasPrefix("話者"), let num = Int(label.dropFirst(2)) {
                speakerId = num - 1
            } else if label.hasPrefix("Speaker "), let num = Int(label.dropFirst(8)) {
                speakerId = num - 1
            } else if label == "マイク" || label == "Mic" {
                source = .mic
            }

            return TranscriptEntry(source: source, text: entryText, startTime: seconds, speakerId: speakerId)
        }
    }

    private func updateSelectedSession() {
        guard let selected = selectedSession,
              let updated = recordingStore.sessions.first(where: { $0.id == selected.id })
        else { return }
        selectedSession = updated
        if !isTitleFocused {
            editingTitle = updated.summary ?? ""
        }
    }

    private func scheduleTitleSave(for session: RecordingSession) {
        titleSaveTask?.cancel()
        titleSaveTask = Task { @MainActor in
            try? await Task.sleep(for: .seconds(1))
            guard !Task.isCancelled else { return }
            saveTitleChange(for: session)
        }
    }

    private func saveTitleChange(for session: RecordingSession?) {
        titleSaveTask?.cancel()
        titleSaveTask = nil
        guard var session, session.folderURL != RecordingStore.baseDirectory else { return }
        let newTitle = editingTitle.trimmingCharacters(in: .whitespacesAndNewlines)
        let effectiveTitle = newTitle.isEmpty ? nil : newTitle
        guard session.summary != effectiveTitle else { return }
        session.summary = effectiveTitle
        recordingStore.saveMetadata(for: session)
        recordingStore.loadSessions()
        if selectedSession?.id == session.id {
            selectedSession = recordingStore.sessions.first(where: { $0.id == session.id })
        }
    }

    private func copyTranscript() {
        guard let text = transcriptText else { return }
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(text, forType: .string)
        showCopied = true
        DispatchQueue.main.asyncAfter(deadline: .now() + 2) { showCopied = false }
    }

    @ViewBuilder
    private func transcriptEntryView(_ entry: TranscriptEntry) -> some View {
        VStack(alignment: .leading, spacing: 2) {
            HStack(spacing: 6) {
                if let speakerId = entry.speakerId {
                    Image(systemName: "person.fill")
                        .font(.caption)
                        .foregroundStyle(TranscriptResultView.speakerColor(for: speakerId))
                    Text(TranscriptResultView.speakerLabel(for: speakerId))
                        .font(.caption.bold())
                        .foregroundStyle(TranscriptResultView.speakerColor(for: speakerId))
                } else {
                    Image(systemName: entry.source == .system ? "speaker.wave.2" : "mic")
                        .font(.caption)
                        .foregroundStyle(entry.source == .system ? .blue : .green)
                    Text(entry.source.label)
                        .font(.caption.bold())
                        .foregroundStyle(entry.source == .system ? .blue : .green)
                }
                Text(TranscriptResultView.formatTime(entry.startTime))
                    .font(.caption.monospacedDigit())
                    .foregroundStyle(.tertiary)
            }
            Text(entry.text)
                .font(.body)
                .lineSpacing(4)
                .textSelection(.enabled)
        }
        .padding(.vertical, 6)
    }

    // MARK: - Note

    @ViewBuilder
    private func noteContent(for session: RecordingSession) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            TextEditor(text: $noteText)
                .font(.body)
                .lineSpacing(4)
                .scrollContentBackground(.hidden)
                .padding(6)
                .frame(minHeight: 260)
                .background(Color(nsColor: .textBackgroundColor), in: RoundedRectangle(cornerRadius: 8))
                .overlay {
                    RoundedRectangle(cornerRadius: 8)
                        .strokeBorder(.quaternary)
                }
                .overlay(alignment: .topLeading) {
                    if noteText.isEmpty {
                        Text("会議中のメモを Markdown で入力できます")
                            .foregroundStyle(.tertiary)
                            .padding(.horizontal, 11)
                            .padding(.vertical, 12)
                            .allowsHitTesting(false)
                    }
                }
                .onChange(of: noteText) { _, _ in scheduleNoteSave(for: session) }

            HStack {
                Spacer()
                Text(noteText == savedNoteText ? "保存済み" : "自動保存されます")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
        }
        .frame(maxWidth: 700, alignment: .leading)
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    private func loadNote() {
        let text = selectedSession.flatMap { try? String(contentsOf: $0.noteURL, encoding: .utf8) } ?? ""
        noteText = text
        savedNoteText = text
    }

    private func scheduleNoteSave(for session: RecordingSession) {
        noteSaveTask?.cancel()
        noteSaveTask = Task { @MainActor in
            try? await Task.sleep(for: .seconds(1))
            guard !Task.isCancelled else { return }
            writeNote(for: session)
        }
    }

    private func flushNote(for session: RecordingSession?) {
        noteSaveTask?.cancel()
        noteSaveTask = nil
        writeNote(for: session)
    }

    private func writeNote(for session: RecordingSession?) {
        guard let session, noteText != savedNoteText else { return }
        if noteText.isEmpty {
            try? FileManager.default.removeItem(at: session.noteURL)
        } else {
            try? noteText.write(to: session.noteURL, atomically: true, encoding: .utf8)
        }
        savedNoteText = noteText
    }

    private func isLiveRecording(_ session: RecordingSession?) -> Bool {
        guard let session else { return false }
        return manager.isRecording && manager.recordingSessionId == session.id
    }

    @ViewBuilder
    private func transcriptContent(for session: RecordingSession) -> some View {
        if !parsedEntries.isEmpty {
            HStack {
                Spacer()
                Button {
                    copyTranscript()
                } label: {
                    Label(showCopied ? "コピー済み" : "文字起こしをコピー", systemImage: showCopied ? "checkmark" : "doc.on.doc")
                }
                .buttonStyle(.accessoryBar)
            }

            VStack(alignment: .leading, spacing: 12) {
                ForEach(parsedEntries) { entry in
                    transcriptEntryView(entry)
                }
            }
            .frame(maxWidth: 700, alignment: .leading)
            .frame(maxWidth: .infinity, alignment: .leading)
        } else if let text = transcriptText, !text.isEmpty {
            HStack {
                Spacer()
                Button {
                    copyTranscript()
                } label: {
                    Label(showCopied ? "コピー済み" : "文字起こしをコピー", systemImage: showCopied ? "checkmark" : "doc.on.doc")
                }
                .buttonStyle(.accessoryBar)
            }

            VStack(alignment: .leading, spacing: 14) {
                ForEach(Array(Self.splitIntoParagraphs(text).enumerated()), id: \.offset) { _, para in
                    Text(para)
                        .font(.body)
                        .lineSpacing(4)
                        .textSelection(.enabled)
                }
            }
            .frame(maxWidth: 700, alignment: .leading)
            .frame(maxWidth: .infinity, alignment: .leading)
        } else if isLiveRecording(session) {
            VStack(spacing: 12) {
                Image(systemName: "waveform")
                    .font(.system(size: 32))
                    .foregroundStyle(.quaternary)
                Text("録音中です。停止後に文字起こしが作成されます")
                    .foregroundStyle(.secondary)
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .padding(.vertical, 40)
        } else if !session.hasTranscript && transcribingId != session.id && manager.autoTranscribeSessionId != session.id {
            VStack(spacing: 12) {
                Image(systemName: "text.word.spacing")
                    .font(.system(size: 32))
                    .foregroundStyle(.quaternary)
                Text("文字起こしデータがありません")
                    .foregroundStyle(.secondary)
                Button { retranscribeTarget = session } label: {
                    Label("文字起こしを作成", systemImage: "arrow.clockwise")
                }
                .buttonStyle(.borderedProminent)
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .padding(.vertical, 40)
        }
    }

    @ViewBuilder
    private func minutesContent(for session: RecordingSession) -> some View {
        if isLiveRecording(session) {
            VStack(spacing: 12) {
                Image(systemName: "doc.text")
                    .font(.system(size: 32))
                    .foregroundStyle(.quaternary)
                Text("録音を停止して文字起こしを作成すると議事録を生成できます")
                    .foregroundStyle(.secondary)
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .padding(.vertical, 40)
        } else if let text = minutesText, !text.isEmpty {
            HStack {
                Spacer()
                Button {
                    NSPasteboard.general.clearContents()
                    NSPasteboard.general.setString(text, forType: .string)
                    showMinutesCopied = true
                    DispatchQueue.main.asyncAfter(deadline: .now() + 2) { showMinutesCopied = false }
                } label: {
                    Label(showMinutesCopied ? "コピー済み" : "議事録をコピー", systemImage: showMinutesCopied ? "checkmark" : "doc.on.doc")
                }
                .buttonStyle(.accessoryBar)
            }

            VStack(alignment: .leading, spacing: 14) {
                ForEach(Array(Self.splitIntoParagraphs(text).enumerated()), id: \.offset) { _, para in
                    Text(para)
                        .font(.body)
                        .lineSpacing(4)
                        .textSelection(.enabled)
                }
            }
            .frame(maxWidth: 700, alignment: .leading)
            .frame(maxWidth: .infinity, alignment: .leading)
        } else if minutesGenerator.isRunning && minutesGenerator.generatingSessionId == session.id {
            VStack(spacing: 16) {
                ProgressView()
                    .controlSize(.large)
                Text("議事録を生成しています…")
                    .font(.headline)
                Text("完了までしばらくかかります")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                Button("キャンセル") { minutesGenerator.cancel() }
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .padding(.vertical, 40)
        } else if let error = minutesGenerator.errorMessage, minutesGenerator.generatingSessionId == session.id {
            VStack(spacing: 12) {
                Text("生成に失敗しました")
                    .font(.headline)
                Text(error)
                    .foregroundStyle(.red)
                    .font(.system(.caption, design: .monospaced))
                    .textSelection(.enabled)
                Button { minutesTarget = session } label: {
                    Label("再試行", systemImage: "arrow.clockwise")
                }
                .buttonStyle(.borderedProminent)
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .padding(.vertical, 40)
        } else if session.hasTranscript {
            VStack(spacing: 12) {
                Image(systemName: "doc.text")
                    .font(.system(size: 32))
                    .foregroundStyle(.quaternary)
                Text("議事録がまだ作成されていません")
                    .foregroundStyle(.secondary)
                Button { minutesTarget = session } label: {
                    Label("議事録を作成", systemImage: "doc.text.below.ecg")
                }
                .buttonStyle(.borderedProminent)
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .padding(.vertical, 40)
        } else {
            VStack(spacing: 12) {
                Image(systemName: "doc.text")
                    .font(.system(size: 32))
                    .foregroundStyle(.quaternary)
                Text("文字起こしを先に実行してください")
                    .foregroundStyle(.secondary)
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .padding(.vertical, 40)
        }
    }

    private static func splitIntoParagraphs(_ text: String) -> [String] {
        let sentenceEndings: [Character] = ["。", "？", "！", ".", "?", "!"]
        var paragraphs: [String] = []
        var current = ""
        var sentenceCount = 0
        let maxChars = 300
        let sentencesPerParagraph = 4

        for char in text {
            current.append(char)
            if sentenceEndings.contains(char) {
                sentenceCount += 1
                if sentenceCount >= sentencesPerParagraph {
                    paragraphs.append(current)
                    current = ""
                    sentenceCount = 0
                }
            } else if current.count >= maxChars {
                let breakChars: [Character] = ["、", ",", " ", "　"]
                if let lastBreak = current.lastIndex(where: { breakChars.contains($0) }) {
                    let afterBreak = current.index(after: lastBreak)
                    let para = String(current[...lastBreak])
                    let remainder = String(current[afterBreak...])
                    paragraphs.append(para)
                    current = remainder
                } else {
                    paragraphs.append(current)
                    current = ""
                }
                sentenceCount = 0
            }
        }
        if !current.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            paragraphs.append(current)
        }
        return paragraphs.isEmpty ? [text] : paragraphs
    }

    private func formatDuration(_ t: TimeInterval) -> String {
        let m = Int(t) / 60
        let s = Int(t) % 60
        return String(format: "%02d:%02d", m, s)
    }

    private func formatPlaybackTime(_ time: TimeInterval) -> String {
        let total = Int(time)
        let m = total / 60
        let s = total % 60
        return String(format: "%d:%02d", m, s)
    }

    // MARK: - Retranscription

    private func cancelRetranscription() {
        activeTranscriber?.cancel()
        cancelFlag.isCancelled = true
        retranscribeTask?.cancel()
        retranscribeTask = nil
        activeTranscriber = nil
        transcribingId = nil
        retranscribePhase = nil
        retranscribeProgress = nil
    }

    private func retranscribe(session: RecordingSession, engineType: TranscriptionEngineType, modelId: String?, diarize: Bool = false, locale: Locale = .current) async {
        transcribingId = session.id
        retranscribePhase = nil
        retranscribeProgress = nil

        var sysSegments: [TimedSegment] = []
        var micSegments: [TimedSegment] = []

        let whisperTranscriber = WhisperTranscriber()
        let progressBinding = $retranscribeProgress
        let flag = cancelFlag

        for (url, isSys) in [(session.systemAudioURL, true), (session.micAudioURL, false)] {
            guard let url else { continue }
            retranscribePhase = isSys ? String(localized: "システム音声") : String(localized: "マイク")
            retranscribeProgress = 0

            if engineType == .whisper {
                guard let path = modelId.flatMap({ whisperManager.modelPath(for: $0) }) else { continue }
                let lang = locale.language.languageCode?.identifier ?? "ja"
                let result = await whisperTranscriber.transcribe(url: url, modelPath: path, language: lang, onUpdate: { _ in }, onProgress: { progress in
                    DispatchQueue.main.async { progressBinding.wrappedValue = progress }
                }, isCancelled: { flag.isCancelled })
                if Task.isCancelled { return }
                if isSys { sysSegments = result.segments }
                else { micSegments = result.segments }
            } else {
                let tx = Transcriber(locale: locale)
                activeTranscriber = tx
                let result = await tx.transcribe(url: url, onUpdate: { _ in }, onProgress: { progress in
                    DispatchQueue.main.async { progressBinding.wrappedValue = progress }
                })
                activeTranscriber = nil
                if Task.isCancelled { return }
                if isSys { sysSegments = result.segments }
                else { micSegments = result.segments }
            }
        }

        let filteredMic = AudioCaptureManager.deduplicateMicEcho(system: sysSegments, mic: micSegments)
        var merged: [TranscriptEntry]

        #if DEBUG
        if diarize {
            retranscribePhase = String(localized: "話者識別")
            retranscribeProgress = nil
            let allSegments = sysSegments + filteredMic
            let audioURL = session.systemAudioURL ?? session.micAudioURL
            if let audioURL, let diarization = try? await manager.speakerDiarizer.diarize(url: audioURL) {
                let labeled = AudioCaptureManager.assignSpeakers(to: allSegments, using: diarization)
                merged = AudioCaptureManager.mergeSegmentsWithSpeakers(labeled)
            } else {
                merged = AudioCaptureManager.mergeSegments(system: sysSegments, mic: filteredMic)
            }
            if Task.isCancelled { return }
        } else {
            merged = AudioCaptureManager.mergeSegments(system: sysSegments, mic: filteredMic)
        }
        #else
        merged = AudioCaptureManager.mergeSegments(system: sysSegments, mic: filteredMic)
        #endif

        let text = TranscriptResultView.buildText(entries: merged)

        let transcriptName = "transcript_\(session.id).txt"
        let transcriptURL = session.folderURL.appendingPathComponent(transcriptName)
        try? text.write(to: transcriptURL, atomically: true, encoding: .utf8)

        var updated = session
        updated.transcriptFile = transcriptName
        updated.engineType = engineType.rawValue
        updated.modelId = modelId
        updated.localeIdentifier = locale.identifier

        retranscribePhase = String(localized: "概要生成")
        retranscribeProgress = nil
        updated.summary = await SummaryGenerator.generate(from: text)

        if session.folderURL != RecordingStore.baseDirectory {
            recordingStore.saveMetadata(for: updated)
        }

        recordingStore.loadSessions()

        if selectedSession?.id == session.id {
            updateSelectedSession()
            loadTranscript()
            selectedDetailTab = .transcript
        }

        transcribingId = nil
        retranscribePhase = nil
        retranscribeProgress = nil
    }
}

struct LevelMeter: View {
    var level: CGFloat
    var icon: String

    var body: some View {
        HStack(spacing: 8) {
            Image(systemName: icon)
                .foregroundStyle(.secondary)
                .frame(width: 20)
            GeometryReader { geo in
                ZStack(alignment: .leading) {
                    Capsule()
                        .fill(.quaternary)
                    Capsule()
                        .fill(
                            LinearGradient(
                                stops: [
                                    .init(color: .green, location: 0),
                                    .init(color: .yellow, location: 0.6),
                                    .init(color: .red, location: 1.0),
                                ],
                                startPoint: .leading,
                                endPoint: .trailing
                            )
                        )
                        .mask(alignment: .leading) {
                            Rectangle()
                                .frame(width: geo.size.width * min(level, 1))
                        }
                        .animation(.linear(duration: 0.2), value: level)
                }
            }
            .frame(height: 6)
        }
    }
}
