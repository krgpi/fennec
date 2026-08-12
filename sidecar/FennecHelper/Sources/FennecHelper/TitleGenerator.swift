import Foundation
import FoundationModels

@available(macOS 26.0, *)
enum TitleGenerator {
    static var isModelAvailable: Bool {
        if case .available = SystemLanguageModel.default.availability { return true }
        return false
    }

    static func generate(text: String, lang: String) async throws -> String? {
        guard isModelAvailable else {
            throw HelperError("language model unavailable")
        }
        let trimmed = String(text.prefix(2000))
        let instructions: String
        let prompt: String
        if lang.hasPrefix("ja") {
            instructions = "あなたは要約アシスタントです。与えられたテキストの内容を端的に表すタイトルを生成します。"
            prompt = "以下の文字起こしテキストの内容を、20文字以内の短いタイトルにまとめてください。\nタイトルのみを出力し、それ以外は何も出力しないでください。\n\n\(trimmed)"
        } else {
            instructions = "You are a summarization assistant. You generate a title that concisely captures the content of the given text."
            prompt = "Summarize the following transcript into a short title of at most 8 words.\nOutput only the title and nothing else.\n\n\(trimmed)"
        }

        let session = LanguageModelSession(instructions: instructions)
        let response = try await session.respond(to: prompt)
        let title = sanitize(response.content)
        return title.isEmpty ? nil : title
    }

    private static let maxLength = 30

    private static func sanitize(_ raw: String) -> String {
        var text = raw.trimmingCharacters(in: .whitespacesAndNewlines)

        if text.hasPrefix("```") {
            let lines = text.components(separatedBy: .newlines)
                .drop { $0.hasPrefix("```") }
                .reversed().drop { $0.hasPrefix("```") }.reversed()
            text = lines.joined(separator: "\n").trimmingCharacters(in: .whitespacesAndNewlines)
        }

        if text.contains("{"),
           let data = text.data(using: .utf8),
           let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
           let title = json["title"] as? String {
            text = title.trimmingCharacters(in: .whitespacesAndNewlines)
        }

        text = text.trimmingCharacters(in: CharacterSet(charactersIn: "\"'"))

        if text.count > maxLength {
            text = String(text.prefix(maxLength))
        }

        return text
    }
}
