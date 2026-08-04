import Foundation
import FoundationModels

struct SummaryGenerator {
    static func generate(from transcript: String) async -> String? {
        let trimmed = String(transcript.prefix(2000))
        let prompt = String(localized: "以下の文字起こしテキストの内容を、20文字以内の短いタイトルにまとめてください。\nタイトルのみを出力し、それ以外は何も出力しないでください。\n\n\(trimmed)")

        do {
            let session = LanguageModelSession(instructions: String(localized: "あなたは要約アシスタントです。与えられたテキストの内容を端的に表すタイトルを生成します。"))
            let response = try await session.respond(to: prompt)
            let summary = response.content.trimmingCharacters(in: .whitespacesAndNewlines)
            return summary.isEmpty ? nil : summary
        } catch {
            return nil
        }
    }
}
