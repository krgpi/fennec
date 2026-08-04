import Foundation
import NaturalLanguage
import Translation

@MainActor
final class TranslationService: ObservableObject {
    @Published var isAvailable = false
    @Published var translationEnabled: Bool = UserDefaults.standard.object(forKey: "translationEnabled") == nil
        ? true
        : UserDefaults.standard.bool(forKey: "translationEnabled") {
        didSet { UserDefaults.standard.set(translationEnabled, forKey: "translationEnabled") }
    }

    @Published var sysTranscriptTranslation = ""
    @Published var micTranscriptTranslation = ""

    private var session: TranslationSession?
    private var sessionSourceLanguage: String?
    private var sessionTargetLanguage: String?
    private var debounceTask: Task<Void, Never>?

    private var targetLanguageCode: String {
        AppLanguage.current.effectiveLanguageCode
    }

    init() {
        Task { await checkAvailability() }
    }

    func checkAvailability() async {
        let availability = LanguageAvailability()
        let target = Locale.Language(identifier: targetLanguageCode)
        let languages = await availability.supportedLanguages
        isAvailable = languages.contains(where: { $0.languageCode == target.languageCode })
    }

    func translateEntries(_ entries: [TranscriptEntry]) async -> [TranscriptEntry] {
        guard translationEnabled else { return entries }

        let targetCode = targetLanguageCode
        let needsTranslation = entries.enumerated().filter { !isTargetLanguage($0.element.text, target: targetCode) }
        guard !needsTranslation.isEmpty else { return entries }

        let detectedSource = detectLanguage(needsTranslation.first!.element.text)
        guard let sourceCode = detectedSource, sourceCode != targetCode else { return entries }
        guard let session = try? await getOrCreateSession(source: sourceCode) else { return entries }

        let requests = needsTranslation.map {
            TranslationSession.Request(sourceText: $0.element.text, clientIdentifier: "\($0.offset)")
        }

        var result = entries
        if let responses = try? await session.translations(from: requests) {
            for response in responses {
                if let idStr = response.clientIdentifier, let idx = Int(idStr) {
                    result[idx].translation = response.targetText
                }
            }
        }
        return result
    }

    func debouncedTranslateLive(sysText: String, micText: String) {
        debounceTask?.cancel()
        debounceTask = Task {
            try? await Task.sleep(for: .seconds(1.5))
            guard !Task.isCancelled else { return }

            async let sysResult = translateForeignParts(sysText)
            async let micResult = translateForeignParts(micText)

            let (sys, mic) = await (sysResult, micResult)
            sysTranscriptTranslation = sys
            micTranscriptTranslation = mic
        }
    }

    func resetLiveTranslation() {
        sysTranscriptTranslation = ""
        micTranscriptTranslation = ""
    }

    func languageChanged() {
        session = nil
        sessionSourceLanguage = nil
        sessionTargetLanguage = nil
        resetLiveTranslation()
        Task { await checkAvailability() }
    }

    // MARK: - Private

    private func isTargetLanguage(_ text: String, target: String) -> Bool {
        guard text.count >= 6 else { return true }
        guard let detected = detectLanguage(text) else { return true }
        return detected == target
    }

    private func detectLanguage(_ text: String) -> String? {
        guard text.count >= 6 else { return nil }
        let recognizer = NLLanguageRecognizer()
        recognizer.processString(text)
        guard let lang = recognizer.dominantLanguage else { return nil }
        return lang.rawValue
    }

    private func extractSentences(_ text: String) -> [String] {
        let tokenizer = NLTokenizer(unit: .sentence)
        tokenizer.string = text
        var sentences: [String] = []
        tokenizer.enumerateTokens(in: text.startIndex..<text.endIndex) { range, _ in
            let sentence = String(text[range]).trimmingCharacters(in: .whitespacesAndNewlines)
            if !sentence.isEmpty {
                sentences.append(sentence)
            }
            return true
        }
        return sentences
    }

    private func translateForeignParts(_ text: String) async -> String {
        guard !text.isEmpty, translationEnabled else { return "" }

        let targetCode = targetLanguageCode
        let sentences = extractSentences(text)
        let foreignSentences = sentences.filter { !isTargetLanguage($0, target: targetCode) }
        guard !foreignSentences.isEmpty else { return "" }

        let sourceCode = detectLanguage(foreignSentences.first!) ?? "en"
        guard sourceCode != targetCode else { return "" }
        guard let session = try? await getOrCreateSession(source: sourceCode) else { return "" }

        let requests = foreignSentences.enumerated().map {
            TranslationSession.Request(sourceText: $0.element, clientIdentifier: "\($0.offset)")
        }

        guard let responses = try? await session.translations(from: requests) else { return "" }

        let sorted = responses.sorted {
            (Int($0.clientIdentifier ?? "0") ?? 0) < (Int($1.clientIdentifier ?? "0") ?? 0)
        }
        return sorted.map(\.targetText).joined(separator: " ")
    }

    private func getOrCreateSession(source: String) async throws -> TranslationSession {
        let target = targetLanguageCode
        if let session, sessionSourceLanguage == source, sessionTargetLanguage == target { return session }
        let newSession = TranslationSession(
            installedSource: .init(identifier: source),
            target: .init(identifier: target)
        )
        self.session = newSession
        self.sessionSourceLanguage = source
        self.sessionTargetLanguage = target
        return newSession
    }
}
