import Foundation
import FoundationModels

struct SummaryGenerator {
    static func generate(from transcript: String) async -> String? {
        let trimmed = String(transcript.prefix(2000))
        let prompt = String(localized: "以下の文字起こしテキストの内容を、20文字以内の短いタイトルにまとめてください。\nタイトルのみを出力し、それ以外は何も出力しないでください。\n\n\(trimmed)")

        do {
            let session = LanguageModelSession(instructions: String(localized: "あなたは要約アシスタントです。与えられたテキストの内容を端的に表すタイトルを生成します。"))
            let response = try await session.respond(to: prompt)
            let summary = sanitize(response.content)
            return summary.isEmpty ? nil : summary
        } catch {
            return nil
        }
    }

    private static let maxLength = 30

    private static func sanitize(_ raw: String) -> String {
        var text = raw.trimmingCharacters(in: .whitespacesAndNewlines)

        // Strip markdown code fences (```json ... ``` etc.)
        if text.hasPrefix("```") {
            let lines = text.components(separatedBy: .newlines)
                .drop { $0.hasPrefix("```") }
                .reversed().drop { $0.hasPrefix("```") }.reversed()
            text = lines.joined(separator: "\n").trimmingCharacters(in: .whitespacesAndNewlines)
        }

        // Extract title from JSON like {"title": "..."}
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
