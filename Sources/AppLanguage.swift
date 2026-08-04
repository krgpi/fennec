import AppKit
import Foundation

enum AppLanguage: String, CaseIterable {
    case system
    case japanese = "ja"
    case english = "en"

    private static let defaultsKey = "appLanguage"

    static var current: AppLanguage {
        AppLanguage(rawValue: UserDefaults.standard.string(forKey: defaultsKey) ?? "") ?? .system
    }

    static func apply(_ language: AppLanguage) {
        UserDefaults.standard.set(language.rawValue, forKey: defaultsKey)
        switch language {
        case .system:
            UserDefaults.standard.removeObject(forKey: "AppleLanguages")
        case .japanese:
            UserDefaults.standard.set(["ja"], forKey: "AppleLanguages")
        case .english:
            UserDefaults.standard.set(["en"], forKey: "AppleLanguages")
        }
    }

    var effectiveLanguageCode: String {
        switch self {
        case .system:
            return Locale.current.language.languageCode?.identifier ?? "en"
        case .japanese:
            return "ja"
        case .english:
            return "en"
        }
    }

    var transcriptionLocale: Locale? {
        switch self {
        case .system:
            return nil
        case .japanese:
            return Locale(identifier: "ja-JP")
        case .english:
            return Locale(identifier: "en-US")
        }
    }

    var displayLocale: Locale {
        switch self {
        case .system:
            return Locale.current
        case .japanese:
            return Locale(identifier: "ja_JP")
        case .english:
            return Locale(identifier: "en_US")
        }
    }

    @MainActor
    static func relaunchApp() {
        let url = Bundle.main.bundleURL
        let configuration = NSWorkspace.OpenConfiguration()
        configuration.createsNewApplicationInstance = true
        NSWorkspace.shared.openApplication(at: url, configuration: configuration) { _, _ in
            DispatchQueue.main.async { NSApp.terminate(nil) }
        }
    }
}
