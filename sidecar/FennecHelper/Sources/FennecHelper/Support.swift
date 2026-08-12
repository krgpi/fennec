import Foundation

func logErr(_ message: String) {
    FileHandle.standardError.write(Data("[fennec-helper] \(message)\n".utf8))
}

struct HelperError: Error, CustomStringConvertible {
    let message: String
    init(_ message: String) { self.message = message }
    var description: String { message }
}

func errorMessage(_ error: Error) -> String {
    if let helperError = error as? HelperError { return helperError.message }
    let ns = error as NSError
    return "\(ns.localizedDescription) (\(ns.domain)#\(ns.code))"
}

final class StdoutWriter {
    private let lock = NSLock()
    private let handle = FileHandle.standardOutput

    func send(_ object: [String: Any]) {
        guard JSONSerialization.isValidJSONObject(object),
              let data = try? JSONSerialization.data(withJSONObject: object) else {
            logErr("failed to encode response: \(object.keys.sorted())")
            return
        }
        lock.lock()
        defer { lock.unlock() }
        handle.write(data)
        handle.write(Data([0x0A]))
    }
}

struct TimedSeg {
    var text: String
    var start: Double
    var end: Double

    var json: [String: Any] { ["text": text, "start": start, "end": end] }
}

extension Character {
    var isCJK: Bool {
        unicodeScalars.contains { scalar in
            let v = scalar.value
            return (0x3000...0x9FFF).contains(v) ||
                   (0xAC00...0xD7AF).contains(v) ||
                   (0xF900...0xFAFF).contains(v) ||
                   (0x20000...0x2FA1F).contains(v) ||
                   (0xFF01...0xFF60).contains(v) ||
                   (0xFFE0...0xFFEF).contains(v)
        }
    }
}

func joinSegmentTexts(_ texts: [String]) -> String {
    guard let first = texts.first else { return "" }
    var result = first
    for i in 1..<texts.count {
        let next = texts[i]
        if next.isEmpty { continue }
        if next.hasPrefix(" ") || result.isEmpty {
            result += next
        } else if let lastChar = result.last, !lastChar.isCJK, let firstChar = next.first, !firstChar.isCJK {
            result += " " + next
        } else {
            result += next
        }
    }
    return result
}

func splitIntoSentences(_ text: String) -> [String] {
    let endings: [Character] = ["。", "？", "！", ".", "?", "!"]
    var sentences: [String] = []
    var current = ""
    var sentenceCount = 0
    let groupSize = 3
    let maxChars = 250

    for char in text {
        current.append(char)
        if endings.contains(char) {
            sentenceCount += 1
            if sentenceCount >= groupSize {
                sentences.append(current)
                current = ""
                sentenceCount = 0
            }
        } else if current.count >= maxChars {
            let breakChars: [Character] = ["、", ",", " ", "　"]
            if let lastBreak = current.lastIndex(where: { breakChars.contains($0) }) {
                let afterBreak = current.index(after: lastBreak)
                sentences.append(String(current[...lastBreak]))
                current = String(current[afterBreak...])
            } else {
                sentences.append(current)
                current = ""
            }
            sentenceCount = 0
        }
    }
    if !current.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
        sentences.append(current)
    }
    return sentences.isEmpty ? [text] : sentences
}

func distributeByCharCount(_ texts: [String], audioDuration: Double) -> [TimedSeg] {
    var segments: [TimedSeg] = []
    let totalChars = texts.reduce(0) { $0 + $1.count }
    var charOffset = 0
    for text in texts {
        let start = totalChars > 0 ? audioDuration * Double(charOffset) / Double(totalChars) : 0
        let endCharOffset = charOffset + text.count
        let end = totalChars > 0 ? audioDuration * Double(endCharOffset) / Double(totalChars) : start + 1.0
        segments.append(TimedSeg(text: text, start: start, end: end))
        charOffset = endCharOffset
    }
    return segments
}
