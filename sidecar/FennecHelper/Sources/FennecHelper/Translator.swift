import Foundation
import NaturalLanguage
import Translation

@available(macOS 26.0, *)
enum Translator {
    @MainActor
    static func translate(texts: [String], target: String) async -> [String?] {
        let targetCode = Locale.Language(identifier: target).languageCode?.identifier ?? target
        var results: [String?] = Array(repeating: nil, count: texts.count)

        var groups: [String: [Int]] = [:]
        for (index, text) in texts.enumerated() {
            guard let source = detectLanguage(text) else { continue }
            let sourceCode = Locale.Language(identifier: source).languageCode?.identifier ?? source
            guard sourceCode != targetCode else { continue }
            groups[source, default: []].append(index)
        }

        for (source, indices) in groups {
            let session = TranslationSession(
                installedSource: .init(identifier: source),
                target: .init(identifier: target)
            )
            let requests = indices.map {
                TranslationSession.Request(sourceText: texts[$0], clientIdentifier: "\($0)")
            }
            do {
                let responses = try await session.translations(from: requests)
                for response in responses {
                    if let idStr = response.clientIdentifier, let index = Int(idStr) {
                        results[index] = response.targetText
                    }
                }
            } catch {
                logErr("translate \(source)->\(target) failed: \(errorMessage(error))")
            }
        }
        return results
    }

    static func hasSupportedLanguages() async -> Bool {
        let languages = await LanguageAvailability().supportedLanguages
        return !languages.isEmpty
    }

    private static func detectLanguage(_ text: String) -> String? {
        guard text.count >= 6 else { return nil }
        let recognizer = NLLanguageRecognizer()
        recognizer.processString(text)
        return recognizer.dominantLanguage?.rawValue
    }
}
