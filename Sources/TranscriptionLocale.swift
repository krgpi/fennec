import Foundation
import Speech

enum TranscriptionLocale {
    static let supported: [Locale] = {
        SFSpeechRecognizer.supportedLocales()
            .sorted { $0.identifier < $1.identifier }
    }()

    static var stored: Locale {
        resolve(UserDefaults.standard.string(forKey: "transcriptionLocale"))
    }

    static func resolve(_ identifier: String?) -> Locale {
        var candidates: [String] = []
        if let identifier { candidates.append(identifier) }
        candidates.append(contentsOf: Locale.preferredLanguages)
        candidates.append(Locale.current.identifier(.bcp47))

        for candidate in candidates {
            let locale = Locale(identifier: candidate)
            // Locale.current は "ja_JP"、supportedLocales は "ja-JP" 形式なので BCP47 に揃えて比較する
            let tag = locale.identifier(.bcp47)
            if let exact = supported.first(where: { $0.identifier(.bcp47).caseInsensitiveCompare(tag) == .orderedSame }) {
                return exact
            }
            if let code = locale.language.languageCode?.identifier {
                let sameLanguage = supported.filter { $0.language.languageCode?.identifier == code }
                // ICU の likely subtags で代表地域を補う（zh-Hant→TW、それも外れたら en→US のように言語だけで再解決）
                let regions = [locale.language.maximalIdentifier, Locale.Language(identifier: code).maximalIdentifier]
                    .compactMap { Locale(identifier: $0).region?.identifier }
                if let match = regions.lazy.compactMap({ region in
                    sameLanguage.first { $0.region?.identifier == region }
                }).first ?? sameLanguage.first {
                    return match
                }
            }
        }
        return supported.first { $0.identifier(.bcp47) == "en-US" } ?? supported.first ?? Locale(identifier: "en-US")
    }
}
