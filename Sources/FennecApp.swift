import AVFoundation
import ServiceManagement
import Speech
import SwiftUI
import Translation

private func allPermissionsGranted() -> Bool {
    AVCaptureDevice.authorizationStatus(for: .audio) == .authorized
        && SFSpeechRecognizer.authorizationStatus() == .authorized
        && CGPreflightScreenCaptureAccess()
}

extension Notification.Name {
    static let openSettings = Notification.Name("openSettings")
}

@main
struct FennecApp: App {
    @StateObject private var manager = AudioCaptureManager()
    @StateObject private var whisperManager = WhisperModelManager()
    @StateObject private var recordingStore = RecordingStore()
    @StateObject private var audioPlayer = AudioPlayer()
    @StateObject private var translationService = TranslationService()
    @State private var setupComplete = CommandLine.arguments.contains("-uitesting") || allPermissionsGranted()
    @AppStorage("showInMenuBar") private var showInMenuBar = false
    @AppStorage("hideFromDock") private var hideFromDock = false

    var body: some Scene {
        WindowGroup {
            Group {
                if setupComplete {
                    ContentView()
                } else {
                    PermissionSetupView { setupComplete = true }
                }
            }
            .environmentObject(manager)
            .environmentObject(whisperManager)
            .environmentObject(recordingStore)
            .environmentObject(audioPlayer)
            .environmentObject(translationService)
            .onAppear {
                manager.whisperModelManager = whisperManager
                manager.translationService = translationService
                applyActivationPolicy()
                if !CommandLine.arguments.contains("-uitesting") {
                    CommandServer.start(manager: manager, whisperManager: whisperManager, recordingStore: recordingStore)
                }
            }
            .onChange(of: hideFromDock) { _, newValue in
                if newValue { showInMenuBar = true }
                applyActivationPolicy()
            }
            .onReceive(NotificationCenter.default.publisher(for: NSWindow.willCloseNotification)) { notif in
                updateDockVisibility(excluding: notif.object as? NSWindow)
            }
            .onReceive(NotificationCenter.default.publisher(for: NSWindow.didBecomeKeyNotification)) { _ in
                updateDockVisibility()
            }
            .onReceive(NotificationCenter.default.publisher(for: NSWindow.didMiniaturizeNotification)) { _ in
                updateDockVisibility()
            }
        }
        .defaultSize(width: 800, height: 600)
        .commands {
            CommandGroup(after: .appSettings) {
                Button("設定…") {
                    NotificationCenter.default.post(name: .openSettings, object: nil)
                }
                .keyboardShortcut(",", modifiers: .command)
            }
        }

        MenuBarExtra(isInserted: $showInMenuBar) {
            menuBarContent
        } label: {
            menuBarLabel
        }
    }

    @ViewBuilder
    private var menuBarLabel: some View {
        if manager.isRecording {
            Label(menuBarDuration, systemImage: "record.circle.fill")
        } else {
            Image(systemName: "waveform")
        }
    }

    @ViewBuilder
    private var menuBarContent: some View {
        if manager.isRecording {
            Text("録音中 — \(menuBarDuration)")
            Divider()
        }

        Button(manager.isRecording ? "録音停止" : "録音開始") {
            Task {
                if manager.isRecording {
                    await manager.stopRecording()
                    recordingStore.loadSessions()
                } else {
                    await manager.startRecording()
                }
            }
        }
        .disabled(!manager.isRecording && manager.selectedDeviceID == nil)

        Divider()

        Button("ウィンドウを表示") {
            activateMainWindow()
        }

        Divider()

        Button("終了") {
            NSApp.terminate(nil)
        }
    }

    private var menuBarDuration: String {
        let total = Int(manager.duration)
        let h = total / 3600
        let m = (total % 3600) / 60
        let s = total % 60
        if h > 0 {
            return String(format: "%d:%02d:%02d", h, m, s)
        }
        return String(format: "%02d:%02d", m, s)
    }

    private func updateDockVisibility(excluding closingWindow: NSWindow? = nil) {
        guard hideFromDock else {
            NSApp.setActivationPolicy(.regular)
            return
        }
        let hasVisibleWindow = NSApp.windows.contains {
            $0 !== closingWindow && $0.isVisible && !$0.isMiniaturized && $0.canBecomeMain
        }
        NSApp.setActivationPolicy(hasVisibleWindow ? .regular : .accessory)
    }

    private func applyActivationPolicy() {
        updateDockVisibility()
        DispatchQueue.main.async {
            NSApp.activate(ignoringOtherApps: true)
        }
    }

    private func activateMainWindow() {
        for window in NSApp.windows where window.canBecomeMain {
            window.makeKeyAndOrderFront(nil)
            break
        }
        updateDockVisibility()
        NSApp.activate(ignoringOtherApps: true)
    }
}
