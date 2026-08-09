#if DEBUG
import AppKit
import SwiftUI

enum ScreenshotHarness {
    private struct MinutesScreenshot: View {
        @StateObject private var generator = MinutesGenerator()
        let session: RecordingSession
        let transcript: String
        let presetStore: MinutesPresetStore
        let recordingStore: RecordingStore
        var body: some View {
            MinutesSheet(session: session, transcript: transcript, generator: generator, presetStore: presetStore, recordingStore: recordingStore)
        }
    }

    enum Screen: String {
        case minutes

        @ViewBuilder
        var view: some View {
            switch self {
            case .minutes:
                MinutesScreenshot(
                    session: Self.sampleSession,
                    transcript: Self.sampleTranscript,
                    presetStore: Self.samplePresetStore,
                    recordingStore: RecordingStore()
                )
            }
        }

        private static let sampleSession = RecordingSession(
            id: "2026-08-09_10-30-00",
            createdAt: Date(timeIntervalSince1970: 1_786_000_000),
            folderURL: URL(fileURLWithPath: NSHomeDirectory())
                .appendingPathComponent("Documents/Fennec/2026-08-09_10-30-00")
        )

        private static let sampleTranscript = "Sample transcript for screenshots."

        private static let samplePresetStore: MinutesPresetStore = {
            let store = MinutesPresetStore()
            store.presets = [
                MinutesPreset(name: "Weekly Sync", backend: .claude, model: "sonnet"),
                MinutesPreset(name: "Design Review", backend: .gemini, model: "gemini-2.5-flash"),
            ]
            return store
        }()
    }

    private static var window: NSWindow?

    static func runIfNeeded() -> Bool {
        let args = CommandLine.arguments
        guard let screen = value(of: "-screenshot", in: args).flatMap(Screen.init(rawValue:)) else { return false }
        let output = value(of: "-screenshot-output", in: args) ?? NSTemporaryDirectory() + "\(screen.rawValue).png"
        let appearance = value(of: "-screenshot-appearance", in: args)
        DispatchQueue.main.async {
            if let appearance {
                NSApp.appearance = NSAppearance(named: appearance == "dark" ? .darkAqua : .aqua)
            }
            capture(screen, to: output)
        }
        return true
    }

    private static func value(of flag: String, in args: [String]) -> String? {
        guard let index = args.firstIndex(of: flag), index + 1 < args.count else { return nil }
        return args[index + 1]
    }

    private static func capture(_ screen: Screen, to path: String) {
        let hosting = NSHostingView(
            rootView: screen.view.background(Color(nsColor: .windowBackgroundColor))
        )
        hosting.frame = NSRect(origin: .zero, size: hosting.fittingSize)

        let win = NSWindow(
            contentRect: hosting.frame,
            styleMask: [.titled, .fullSizeContentView],
            backing: .buffered,
            defer: false
        )
        win.titlebarAppearsTransparent = true
        win.titleVisibility = .hidden
        win.contentView = hosting
        win.center()
        win.makeKeyAndOrderFront(nil)
        window = win
        NSApp.activate(ignoringOtherApps: true)

        DispatchQueue.main.asyncAfter(deadline: .now() + 3) {
            win.setContentSize(hosting.fittingSize)
            guard let content = win.contentView,
                  let rep = content.bitmapImageRepForCachingDisplay(in: content.bounds) else {
                NSApp.terminate(nil)
                return
            }
            content.cacheDisplay(in: content.bounds, to: rep)
            if let data = rep.representation(using: .png, properties: [:]) {
                try? data.write(to: URL(fileURLWithPath: path))
                FileHandle.standardError.write(Data("screenshot written: \(path)\n".utf8))
            }
            NSApp.terminate(nil)
        }
    }
}
#endif
