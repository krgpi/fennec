import AppKit
import ServiceManagement
import Speech
import SwiftUI
import UniformTypeIdentifiers

private enum SettingsTab: Int, CaseIterable {
    case general, recording, transcription

    var label: LocalizedStringKey {
        switch self {
        case .general: return "一般"
        case .recording: return "settings.tab.recording"
        case .transcription: return "文字起こし"
        }
    }
}

struct EngineSettingsView: View {
    @EnvironmentObject var modelManager: WhisperModelManager
    @EnvironmentObject var captureManager: AudioCaptureManager
    @EnvironmentObject var translationService: TranslationService
    #if DEBUG
    @ObservedObject var diarizer: SpeakerDiarizer
    #endif
    @Environment(\.dismiss) private var dismiss
    @AppStorage("showInMenuBar") private var showInMenuBar = false
    @AppStorage("hideFromDock") private var hideFromDock = false
    @AppStorage("launchAtLogin") private var launchAtLogin = false
    @State private var saveLocationDisplay = StorageLocation.displayPath
    @State private var selectedTab: SettingsTab = CommandLine.arguments.contains("--snapshot-settings") ? .transcription : .general
    @State private var showWhisperDeleteConfirmation = false
    #if DEBUG
    @State private var showDiarizationDeleteConfirmation = false
    #endif
    @State private var showWhisperDownloadSuccess = false
    #if DEBUG
    @State private var showDiarizationDownloadSuccess = false
    #endif
    @State private var selectedLanguage = AppLanguage.current
    @State private var showLanguageRestartHint = false

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
        VStack(alignment: .leading, spacing: 16) {
            HStack {
                Text("設定")
                    .font(.title2.bold())
                Spacer()
                Button("閉じる") { dismiss() }
                    .keyboardShortcut(.cancelAction)
            }

            Picker("", selection: $selectedTab) {
                ForEach(SettingsTab.allCases, id: \.self) { tab in
                    Text(tab.label).tag(tab)
                }
            }
            .pickerStyle(.segmented)
            .labelsHidden()

            Group {
                switch selectedTab {
                case .general:
                    generalTab
                case .recording:
                    recordingTab
                case .transcription:
                    transcriptionTab
                }
            }
            .frame(maxHeight: .infinity, alignment: .top)
        }
        .padding(24)
        .frame(width: 460, height: 550)
    }

    private var launchAtLoginBinding: Binding<Bool> {
        Binding(
            get: { launchAtLogin },
            set: { newValue in
                do {
                    if newValue {
                        try SMAppService.mainApp.register()
                    } else {
                        try SMAppService.mainApp.unregister()
                    }
                    launchAtLogin = newValue
                } catch {}
            }
        )
    }

    private var languageBinding: Binding<AppLanguage> {
        Binding(
            get: { selectedLanguage },
            set: { newValue in
                guard newValue != selectedLanguage else { return }
                selectedLanguage = newValue
                AppLanguage.apply(newValue)
                translationService.languageChanged()
                showLanguageRestartHint = true
            }
        )
    }

    private var generalTab: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 16) {
                GroupBox {
                    VStack(alignment: .leading, spacing: 4) {
                        Picker("言語", selection: languageBinding) {
                            Text("システムに従う").tag(AppLanguage.system)
                            Text(verbatim: "日本語").tag(AppLanguage.japanese)
                            Text(verbatim: "English").tag(AppLanguage.english)
                        }
                        Text("翻訳先の言語も選択した言語に切り替わります。")
                            .font(.callout)
                            .foregroundStyle(.secondary)
                        if showLanguageRestartHint {
                            HStack(spacing: 8) {
                                Text("表示言語の変更はアプリの再起動後に反映されます。")
                                    .font(.callout)
                                    .foregroundStyle(.orange)
                                Button("今すぐ再起動") {
                                    AppLanguage.relaunchApp()
                                }
                                .font(.callout)
                                .buttonStyle(.bordered)
                                .controlSize(.small)
                            }
                        }
                    }
                    .padding(4)
                    .frame(maxWidth: .infinity, alignment: .leading)
                }

                GroupBox {
                    VStack(alignment: .leading, spacing: 12) {
                        VStack(alignment: .leading, spacing: 4) {
                            Toggle("ログイン時に起動", isOn: launchAtLoginBinding)
                            Text("Macの起動時に自動的にアプリを起動します。")
                                .font(.callout)
                                .foregroundStyle(.secondary)
                        }
                        Divider()
                        VStack(alignment: .leading, spacing: 4) {
                            Toggle("メニューバーに表示", isOn: $showInMenuBar)
                                .disabled(hideFromDock)
                            Text("メニューバーからすばやく録音の開始・停止ができます。録音中は経過時間が表示されます。")
                                .font(.callout)
                                .foregroundStyle(.secondary)
                            if hideFromDock {
                                Text("Dockアイコンを非表示にしている場合、メニューバー表示は必須です。")
                                    .font(.callout)
                                    .foregroundStyle(.orange)
                            }
                        }
                        Divider()
                        VStack(alignment: .leading, spacing: 4) {
                            Toggle("Dockアイコンを非表示", isOn: Binding(
                                get: { hideFromDock },
                                set: { newValue in
                                    if newValue { showInMenuBar = true }
                                    hideFromDock = newValue
                                }
                            ))
                            Text("ウィンドウが表示されていないときはDockアイコンを非表示にします。")
                                .font(.callout)
                                .foregroundStyle(.secondary)
                        }
                    }
                    .padding(4)
                    .frame(maxWidth: .infinity, alignment: .leading)
                }

                #if DEBUG
                VStack(alignment: .leading, spacing: 6) {
                    Text("マイク検知")
                        .foregroundStyle(.secondary)
                    GroupBox {
                        VStack(alignment: .leading, spacing: 12) {
                            VStack(alignment: .leading, spacing: 4) {
                                Toggle("他のアプリのマイク使用を検知", isOn: $captureManager.monitorMicUsage)
                                Text("他のアプリがマイクを使い始めたとき、録音の開始を確認するアラートを表示します。")
                                    .font(.callout)
                                    .foregroundStyle(.secondary)
                            }
                            if captureManager.monitorMicUsage {
                                Divider()
                                VStack(alignment: .leading, spacing: 8) {
                                    VStack(alignment: .leading, spacing: 4) {
                                        Text("検知から除外するアプリ")
                                            .font(.callout)
                                        Text("ここに登録したアプリがマイクを使用してもアラートを表示しません。一部の再生専用アプリはマイクを使用中と誤判定されるため、あらかじめ登録されています。")
                                            .font(.callout)
                                            .foregroundStyle(.secondary)
                                    }
                                    ForEach(captureManager.micUsageExcludedBundleIDs, id: \.self) { bundleID in
                                        HStack(spacing: 6) {
                                            ExcludedAppLabel(bundleID: bundleID)
                                            Spacer()
                                            Button {
                                                captureManager.micUsageExcludedBundleIDs.removeAll { $0 == bundleID }
                                            } label: {
                                                Image(systemName: "xmark.circle.fill")
                                                    .foregroundStyle(.secondary)
                                            }
                                            .buttonStyle(.plain)
                                            .help("除外リストから削除")
                                        }
                                    }
                                    Button("アプリを追加...") {
                                        addMicExclusionApp()
                                    }
                                    .font(.callout)
                                    .buttonStyle(.bordered)
                                    .controlSize(.small)
                                }
                            }
                        }
                        .padding(4)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    }
                }
                #endif
            }
        }
        .onAppear {
            launchAtLogin = SMAppService.mainApp.status == .enabled
        }
    }

    private var recordingTab: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 16) {
                GroupBox {
                    VStack(alignment: .leading, spacing: 4) {
                        Toggle("無音で自動停止", isOn: $captureManager.silenceAutoStopEnabled)
                        Text("一定時間音声が検出されないと自動的に録音を停止し、通知でお知らせします。")
                            .font(.callout)
                            .foregroundStyle(.secondary)
                        if captureManager.silenceAutoStopEnabled {
                            Stepper("停止までの時間: \(captureManager.silenceAutoStopMinutes)分",
                                    value: $captureManager.silenceAutoStopMinutes,
                                    in: 1...60)
                                .font(.callout)
                            Text("通知スタイルをシステム設定 > 通知 > ローカルAI文字起こし で「通知」に変更すると、画面に残り続けるため離席中の自動停止にも気づきやすくなります。")
                                .font(.callout)
                                .foregroundStyle(.orange)
                        }
                    }
                    .padding(4)
                    .frame(maxWidth: .infinity, alignment: .leading)
                }

                VStack(alignment: .leading, spacing: 6) {
                    Text("会議リマインダー")
                        .foregroundStyle(.secondary)
                    GroupBox {
                        VStack(alignment: .leading, spacing: 12) {
                            VStack(alignment: .leading, spacing: 4) {
                                Toggle("会議リマインダー", isOn: $captureManager.meetingReminderEnabled)
                                Text("カレンダーの会議予定に合わせて録音開始を提案します。")
                                    .font(.callout)
                                    .foregroundStyle(.secondary)
                            }
                            if captureManager.meetingReminderEnabled {
                                Divider()
                                Stepper(captureManager.meetingReminderMinutesBefore == 0
                                            ? "開始時刻に通知"
                                            : "開始 \(captureManager.meetingReminderMinutesBefore)分前から通知",
                                        value: $captureManager.meetingReminderMinutesBefore,
                                        in: 0...30)
                                    .font(.callout)
                                Divider()
                                VStack(alignment: .leading, spacing: 4) {
                                    Toggle("会議URLを含むイベントのみ", isOn: $captureManager.meetingReminderRequireMeetingURL)
                                        .font(.callout)
                                    Toggle("マイク使用中のみ通知", isOn: $captureManager.meetingReminderRequireMic)
                                        .font(.callout)
                                }
                                Divider()
                                HStack(spacing: 16) {
                                    CalendarSelectionView(captureManager: captureManager)
                                    Button("テスト通知を送信") {
                                        captureManager.sendMeetingReminderNotification(eventTitle: String(localized: "テスト会議"))
                                    }
                                    .font(.callout)
                                    .buttonStyle(.bordered)
                                    .controlSize(.small)
                                }
                            }
                        }
                        .padding(4)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    }
                }

                VStack(alignment: .leading, spacing: 6) {
                    Text("保存先")
                        .foregroundStyle(.secondary)
                    GroupBox {
                        VStack(alignment: .leading, spacing: 8) {
                            Text("変更すると録音一覧の表示も新しい保存先に切り替わります。既存の録音ファイルは自動では移動しません。")
                                .font(.callout)
                                .foregroundStyle(.secondary)
                            HStack {
                                Button {
                                    NSWorkspace.shared.open(StorageLocation.baseDirectory)
                                } label: {
                                    Text(saveLocationDisplay)
                                        .font(.callout)
                                        .lineLimit(1)
                                        .truncationMode(.middle)
                                }
                                .buttonStyle(.link)
                                Spacer()
                                Button("変更…") {
                                    if StorageLocation.chooseDirectory() != nil {
                                        saveLocationDisplay = StorageLocation.displayPath
                                    }
                                }
                                .font(.callout)
                                .buttonStyle(.bordered)
                                .controlSize(.small)
                            }
                        }
                        .padding(4)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    }
                }
            }
        }
    }

    private var transcriptionTab: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 16) {
                GroupBox {
                    VStack(alignment: .leading, spacing: 4) {
                        Picker("文字起こし言語", selection: $captureManager.transcriptionLocale) {
                            ForEach(TranscriptionLocale.supported, id: \.identifier) { locale in
                                Text("\(Self.flagEmoji(for: locale)) \(AppLanguage.current.displayLocale.localizedString(forIdentifier: locale.identifier) ?? locale.identifier)")
                                    .tag(locale)
                            }
                        }
                        Text("音声認識に使用する言語を選択します。")
                            .font(.callout)
                            .foregroundStyle(.secondary)
                    }
                    .padding(4)
                    .frame(maxWidth: .infinity, alignment: .leading)
                }

                GroupBox {
                    VStack(alignment: .leading, spacing: 12) {
                        VStack(alignment: .leading, spacing: 4) {
                            Toggle("録音中にリアルタイム文字起こし", isOn: $captureManager.liveTranscriptionEnabled)
                            Text("Apple音声認識で録音中にリアルタイム文字起こしを表示します。Whisperモデルはリアルタイム文字起こしには使用できません。")
                                .font(.callout)
                                .foregroundStyle(.secondary)
                        }
                    }
                    .padding(4)
                    .frame(maxWidth: .infinity, alignment: .leading)
                }

                GroupBox {
                    VStack(alignment: .leading, spacing: 4) {
                        Toggle("外国語をアプリの言語に翻訳", isOn: $translationService.translationEnabled)
                        Text("アプリの言語以外のテキストを検出すると翻訳を表示します。オンデバイス処理のためデータは外部に送信されません。")
                            .font(.callout)
                            .foregroundStyle(.secondary)
                        if !translationService.isAvailable {
                            HStack(spacing: 4) {
                                Image(systemName: "exclamationmark.triangle")
                                    .foregroundStyle(.orange)
                                Text("翻訳モデルが未インストールです。システム設定 > 一般 > 言語と地域 > 翻訳言語 でダウンロードしてください。")
                                    .font(.callout)
                                    .foregroundStyle(.orange)
                            }
                        }
                    }
                    .padding(4)
                    .frame(maxWidth: .infinity, alignment: .leading)
                }

                VStack(alignment: .leading, spacing: 6) {
                    Text("録音後の自動文字起こし")
                        .foregroundStyle(.secondary)
                GroupBox {
                    VStack(alignment: .leading, spacing: 12) {
                        VStack(alignment: .leading, spacing: 4) {
                            Toggle("録音後に自動で文字起こし", isOn: $captureManager.autoTranscribeEnabled)
                            Text("録音終了後に自動的に文字起こしを実行します。")
                                .font(.callout)
                                .foregroundStyle(.secondary)
                        }

                        if captureManager.autoTranscribeEnabled {
                            Divider()
                            VStack(alignment: .leading, spacing: 4) {
                                Picker("エンジン", selection: $captureManager.autoTranscribeEngine) {
                                    Text("Apple（標準）").tag(TranscriptionEngineType.apple)
                                    if modelManager.isEnabled && modelManager.isModelReady {
                                        Text("Whisper（高精度）").tag(TranscriptionEngineType.whisper)
                                    }
                                }
                            }

                            #if DEBUG
                            Divider()
                            VStack(alignment: .leading, spacing: 4) {
                                Toggle("話者識別", isOn: Binding(
                                    get: { captureManager.diarizationEnabled },
                                    set: { newValue in
                                        if newValue {
                                            captureManager.diarizationEnabled = true
                                        } else if diarizer.isModelReady {
                                            showDiarizationDeleteConfirmation = true
                                        } else {
                                            captureManager.diarizationEnabled = false
                                        }
                                    }
                                ))
                                    .disabled(!diarizer.isModelReady && !diarizer.isDownloading)
                                Text("文字起こしと合わせて話者を自動識別します。")
                                    .font(.callout)
                                    .foregroundStyle(.secondary)

                                if diarizer.isDownloading {
                                    HStack(spacing: 6) {
                                        ProgressView()
                                            .controlSize(.small)
                                        Text("話者識別モデルをダウンロード中...")
                                            .font(.callout)
                                            .foregroundStyle(.secondary)
                                    }
                                } else if !diarizer.isModelReady {
                                    Button {
                                        Task { await diarizer.downloadModel() }
                                    } label: {
                                        HStack {
                                            Image(systemName: "arrow.down.circle")
                                            Text("話者識別モデルをダウンロード")
                                        }
                                    }
                                    .buttonStyle(.borderedProminent)
                                    .controlSize(.small)
                                }

                                if showDiarizationDownloadSuccess {
                                    HStack(spacing: 6) {
                                        Image(systemName: "checkmark.circle.fill")
                                            .foregroundStyle(.green)
                                        Text("ダウンロード完了")
                                            .font(.callout)
                                    }
                                    .transition(.opacity)
                                }

                                if let error = diarizer.downloadError {
                                    Text(error)
                                        .foregroundStyle(.red)
                                        .font(.callout)
                                }
                            }
                            #endif
                        }
                    }
                    .padding(4)
                    .frame(maxWidth: .infinity, alignment: .leading)
                }
                #if DEBUG
                .onChange(of: diarizer.isDownloading) {
                    if !diarizer.isDownloading && diarizer.isModelReady {
                        withAnimation { showDiarizationDownloadSuccess = true }
                        DispatchQueue.main.asyncAfter(deadline: .now() + 3) {
                            withAnimation { showDiarizationDownloadSuccess = false }
                        }
                    }
                }
                .alert("話者識別モデルの削除", isPresented: $showDiarizationDeleteConfirmation) {
                    Button("削除", role: .destructive) {
                        diarizer.deleteModel()
                        captureManager.diarizationEnabled = false
                    }
                    Button("キャンセル", role: .cancel) {}
                } message: {
                    Text("ダウンロード済みの話者識別モデルを削除します。再度使用するにはモデルのダウンロードが必要です。")
                }
                #endif
                }

                VStack(alignment: .leading, spacing: 6) {
                    Text("Whisper")
                        .foregroundStyle(.secondary)
                GroupBox {
                    VStack(alignment: .leading, spacing: 12) {
                        VStack(alignment: .leading, spacing: 4) {
                            Toggle("Whisperモデルを使用", isOn: Binding(
                                get: { modelManager.isEnabled },
                                set: { newValue in
                                    if newValue {
                                        Task { await modelManager.enableAndDownload() }
                                    } else {
                                        showWhisperDeleteConfirmation = true
                                    }
                                }
                            ))
                            .disabled(modelManager.isDownloading)
                        }

                        if modelManager.isDownloading {
                            VStack(alignment: .leading, spacing: 4) {
                                ProgressView(value: modelManager.downloadProgress)
                                Text("モデルをダウンロード中… \(String(format: "%d%%", Int(modelManager.downloadProgress * 100)))")
                                    .font(.callout)
                                    .foregroundStyle(.secondary)
                            }
                        }

                        if let error = modelManager.downloadError {
                            HStack(alignment: .top, spacing: 4) {
                                Text(error)
                                    .foregroundStyle(.red)
                                    .font(.callout)
                                    .textSelection(.enabled)
                                Button {
                                    NSPasteboard.general.clearContents()
                                    NSPasteboard.general.setString(error, forType: .string)
                                } label: {
                                    Image(systemName: "doc.on.doc")
                                        .font(.callout)
                                        .foregroundStyle(.secondary)
                                }
                                .buttonStyle(.plain)
                                .help("エラーメッセージをコピー")
                            }
                        }

                        if modelManager.isEnabled && !modelManager.isDownloading {
                            Divider()
                            Picker("モデル", selection: $modelManager.selectedModelId) {
                                ForEach(WhisperModelInfo.all) { model in
                                    HStack {
                                        Text(model.name)
                                        Text(model.size)
                                            .foregroundStyle(.secondary)
                                    }
                                    .tag(model.id)
                                }
                            }

                            if !modelManager.isModelReady {
                                Button {
                                    Task { await modelManager.downloadModel() }
                                } label: {
                                    HStack {
                                        Image(systemName: "arrow.down.circle")
                                        Text("このモデルをダウンロード（\(modelManager.selectedModel.size)）")
                                    }
                                    .frame(maxWidth: .infinity)
                                }
                                .buttonStyle(.borderedProminent)
                                .controlSize(.large)
                            }

                            if showWhisperDownloadSuccess {
                                HStack(spacing: 6) {
                                    Image(systemName: "checkmark.circle.fill")
                                        .foregroundStyle(.green)
                                    Text("ダウンロード完了")
                                        .font(.callout)
                                }
                                .transition(.opacity)
                            }
                        }
                    }
                    .padding(4)
                    .frame(maxWidth: .infinity, alignment: .leading)
                }
                .onChange(of: modelManager.isDownloading) {
                    if !modelManager.isDownloading && modelManager.isModelReady {
                        withAnimation { showWhisperDownloadSuccess = true }
                        DispatchQueue.main.asyncAfter(deadline: .now() + 3) {
                            withAnimation { showWhisperDownloadSuccess = false }
                        }
                    }
                }
                .alert("Whisperモデルの削除", isPresented: $showWhisperDeleteConfirmation) {
                    Button("削除", role: .destructive) {
                        modelManager.disableAndDeleteAllModels()
                    }
                    Button("キャンセル", role: .cancel) {}
                } message: {
                    Text("ダウンロード済みのWhisperモデルをすべて削除します。再度使用するにはモデルのダウンロードが必要です。")
                }
                }

            }
        }
    }

    #if DEBUG
    private func addMicExclusionApp() {
        let panel = NSOpenPanel()
        panel.canChooseFiles = true
        panel.canChooseDirectories = false
        panel.allowsMultipleSelection = false
        panel.allowedContentTypes = [.application]
        panel.directoryURL = URL(fileURLWithPath: "/Applications")
        panel.message = String(localized: "マイク検知から除外するアプリを選択してください")
        panel.prompt = String(localized: "追加")

        guard panel.runModal() == .OK, let url = panel.url,
              let bundleID = Bundle(url: url)?.bundleIdentifier else { return }
        if !captureManager.micUsageExcludedBundleIDs.contains(bundleID) {
            captureManager.micUsageExcludedBundleIDs.append(bundleID)
        }
    }
    #endif
}

#if DEBUG
private struct ExcludedAppLabel: View {
    let bundleID: String

    var body: some View {
        if let url = NSWorkspace.shared.urlForApplication(withBundleIdentifier: bundleID) {
            HStack(spacing: 6) {
                Image(nsImage: NSWorkspace.shared.icon(forFile: url.path))
                    .resizable()
                    .frame(width: 18, height: 18)
                Text(Self.displayName(for: url))
                    .font(.callout)
                Text(bundleID)
                    .font(.caption)
                    .foregroundStyle(.tertiary)
            }
        } else {
            Text(bundleID)
                .font(.callout)
                .foregroundStyle(.secondary)
        }
    }

    private static func displayName(for url: URL) -> String {
        if let localized = (try? url.resourceValues(forKeys: [.localizedNameKey]))?.localizedName {
            return localized.hasSuffix(".app") ? String(localized.dropLast(4)) : localized
        }
        return (Bundle(url: url)?.infoDictionary?["CFBundleName"] as? String)
            ?? url.deletingPathExtension().lastPathComponent
    }
}
#endif

private struct CalendarSelectionView: View {
    @ObservedObject var captureManager: AudioCaptureManager
    @State private var calendars: [CalendarInfo] = []
    @State private var showSheet = false

    private var selectedIDs: Set<String>? { captureManager.meetingReminderCalendarIdentifiers }

    private var groupedCalendars: [(source: String, calendars: [CalendarInfo])] {
        let grouped = Dictionary(grouping: calendars, by: \.source)
        return grouped.keys.sorted().map { key in (source: key, calendars: grouped[key]!) }
    }

    private func isSelected(_ id: String) -> Bool {
        guard let ids = selectedIDs else { return true }
        return ids.contains(id)
    }

    var body: some View {
        Button("対象カレンダー…") {
            showSheet = true
        }
        .font(.callout)
        .buttonStyle(.bordered)
        .controlSize(.small)
        .sheet(isPresented: $showSheet) {
            VStack(alignment: .leading, spacing: 12) {
                HStack {
                    Text("対象カレンダー")
                        .font(.headline)
                    Spacer()
                    Button("完了") { showSheet = false }
                        .keyboardShortcut(.defaultAction)
                }

                Divider()

                if calendars.isEmpty {
                    Text("カレンダーを読み込み中…")
                        .font(.callout)
                        .foregroundStyle(.secondary)
                } else {
                    HStack(spacing: 12) {
                        Button("すべて選択") {
                            captureManager.meetingReminderCalendarIdentifiers = nil
                        }
                        Button("すべて外す") {
                            captureManager.meetingReminderCalendarIdentifiers = []
                        }
                    }
                    .font(.callout)
                    .buttonStyle(.link)

                    ScrollView {
                        VStack(alignment: .leading, spacing: 6) {
                            ForEach(groupedCalendars, id: \.source) { group in
                                Text(group.source)
                                    .font(.callout)
                                    .foregroundStyle(.secondary)
                                    .padding(.top, 4)
                                ForEach(group.calendars) { cal in
                                    Toggle(isOn: Binding(
                                        get: { isSelected(cal.id) },
                                        set: { enabled in
                                            var ids = selectedIDs ?? Set(calendars.map(\.id))
                                            if enabled {
                                                ids.insert(cal.id)
                                            } else {
                                                ids.remove(cal.id)
                                            }
                                            captureManager.meetingReminderCalendarIdentifiers = ids.count == calendars.count ? nil : ids
                                        }
                                    )) {
                                        HStack(spacing: 6) {
                                            Circle()
                                                .fill(Color(cgColor: cal.color))
                                                .frame(width: 10, height: 10)
                                            Text(cal.title)
                                        }
                                    }
                                    .font(.callout)
                                }
                            }
                        }
                        .frame(maxWidth: .infinity, alignment: .leading)
                    }
                }
            }
            .padding(20)
            .frame(width: 320, height: 400)
            .task {
                if !captureManager.calendarMonitor.hasAccess {
                    _ = await captureManager.calendarMonitor.requestAccess()
                }
                calendars = captureManager.calendarMonitor.availableCalendars()
            }
        }
    }
}
