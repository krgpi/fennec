import AVFoundation
import Foundation
import Speech

final class BatchTranscriber {
    struct Output {
        var text: String
        var segments: [TimedSeg]
    }

    private let locale: Locale
    private var task: SFSpeechRecognitionTask?

    init(locale: Locale) {
        self.locale = locale
    }

    func transcribe(url: URL, onProgress: @escaping (Double) -> Void) async throws -> Output {
        guard FileManager.default.fileExists(atPath: url.path) else {
            throw HelperError("file not found: \(url.path)")
        }
        let asset = AVURLAsset(url: url)
        let audioDuration = (try? await asset.load(.duration).seconds) ?? 0

        if #available(macOS 26.0, *) {
            do {
                return try await transcribeWithAnalyzer(url: url, audioDuration: audioDuration, onProgress: onProgress)
            } catch {
                logErr("SpeechAnalyzer batch failed (\(errorMessage(error))), falling back to SFSpeechRecognizer")
            }
        }
        return try await transcribeWithSFSpeech(url: url, audioDuration: audioDuration, onProgress: onProgress)
    }

    @available(macOS 26.0, *)
    private func transcribeWithAnalyzer(url: URL, audioDuration: Double, onProgress: @escaping (Double) -> Void) async throws -> Output {
        let transcriber = SpeechTranscriber(
            locale: locale,
            transcriptionOptions: [],
            reportingOptions: [.volatileResults],
            attributeOptions: []
        )
        if let request = try await AssetInventory.assetInstallationRequest(supporting: [transcriber]) {
            try await request.downloadAndInstall()
        }
        guard let format = await SpeechAnalyzer.bestAvailableAudioFormat(compatibleWith: [transcriber]) else {
            throw HelperError("no analyzer audio format for locale \(locale.identifier)")
        }
        let analyzer = SpeechAnalyzer(modules: [transcriber])
        let (inputSequence, builder) = AsyncStream<AnalyzerInput>.makeStream()
        try await analyzer.start(inputSequence: inputSequence)

        let feedTask = Task {
            await Self.feedAudioFile(url: url, targetFormat: format, builder: builder, analyzer: analyzer, audioDuration: audioDuration, onProgress: onProgress)
        }

        var finalizedText = ""
        var rawResults: [(text: String, range: CMTimeRange)] = []

        do {
            for try await result in transcriber.results {
                if result.isFinal {
                    let text = String(result.text.characters)
                    rawResults.append((text, result.range))
                    finalizedText += text
                }
            }
        } catch {
            // results stream ended
        }

        await feedTask.value

        let segments = Self.buildSegments(from: rawResults, audioDuration: audioDuration)
        return Output(text: finalizedText, segments: segments)
    }

    private static func buildSegments(from results: [(text: String, range: CMTimeRange)], audioDuration: Double) -> [TimedSeg] {
        let hasRealTimings = results.contains { $0.range.duration.isNumeric && $0.range.duration.seconds > 0 }
        if hasRealTimings {
            return results.map { result in
                let start = result.range.start.isNumeric ? result.range.start.seconds : 0
                let end = result.range.duration.isNumeric ? start + result.range.duration.seconds : start
                return TimedSeg(text: result.text, start: start, end: max(end, start))
            }
        }
        return distributeByCharCount(results.map(\.text), audioDuration: audioDuration)
    }

    @available(macOS 26.0, *)
    private static func feedAudioFile(url: URL, targetFormat: AVAudioFormat, builder: AsyncStream<AnalyzerInput>.Continuation, analyzer: SpeechAnalyzer, audioDuration: Double, onProgress: @escaping (Double) -> Void) async {
        guard let audioFile = try? AVAudioFile(forReading: url) else {
            builder.finish()
            try? await analyzer.finalizeAndFinishThroughEndOfInput()
            return
        }

        let sourceFormat = audioFile.processingFormat
        let converter: AVAudioConverter? = sourceFormat != targetFormat ? AVAudioConverter(from: sourceFormat, to: targetFormat) : nil

        let bufferFrameCount: AVAudioFrameCount = 16384
        var totalFramesRead: AVAudioFrameCount = 0
        let totalFrames = AVAudioFrameCount(audioFile.length)

        while totalFramesRead < totalFrames {
            guard let readBuffer = AVAudioPCMBuffer(pcmFormat: sourceFormat, frameCapacity: bufferFrameCount) else { break }
            do {
                try audioFile.read(into: readBuffer)
            } catch {
                break
            }
            guard readBuffer.frameLength > 0 else { break }
            totalFramesRead += readBuffer.frameLength

            let sendBuffer: AVAudioPCMBuffer
            if let converter {
                let capacity = AVAudioFrameCount(Double(readBuffer.frameLength) * targetFormat.sampleRate / sourceFormat.sampleRate) + 16
                guard let outBuffer = AVAudioPCMBuffer(pcmFormat: targetFormat, frameCapacity: capacity) else { break }
                var consumed = false
                var error: NSError?
                converter.convert(to: outBuffer, error: &error) { _, outStatus in
                    if consumed {
                        outStatus.pointee = .noDataNow
                        return nil
                    }
                    consumed = true
                    outStatus.pointee = .haveData
                    return readBuffer
                }
                guard error == nil, outBuffer.frameLength > 0 else { break }
                sendBuffer = outBuffer
            } else {
                sendBuffer = readBuffer
            }

            builder.yield(AnalyzerInput(buffer: sendBuffer))

            if audioDuration > 0 {
                onProgress(min(Double(totalFramesRead) / Double(totalFrames), 0.99))
            }
        }

        builder.finish()
        try? await analyzer.finalizeAndFinishThroughEndOfInput()
        onProgress(1.0)
    }

    private static func ensureSpeechAuthorization() async -> Bool {
        switch SFSpeechRecognizer.authorizationStatus() {
        case .authorized:
            return true
        case .notDetermined:
            return await withCheckedContinuation { cont in
                SFSpeechRecognizer.requestAuthorization { status in
                    cont.resume(returning: status == .authorized)
                }
            }
        default:
            return false
        }
    }

    private func transcribeWithSFSpeech(url: URL, audioDuration: Double, onProgress: @escaping (Double) -> Void) async throws -> Output {
        guard await Self.ensureSpeechAuthorization() else {
            throw HelperError("speech recognition not authorized")
        }
        guard let recognizer = SFSpeechRecognizer(locale: locale), recognizer.isAvailable else {
            throw HelperError("speech recognizer unavailable for \(locale.identifier)")
        }

        let request = SFSpeechURLRecognitionRequest(url: url)
        request.shouldReportPartialResults = true
        request.taskHint = .dictation

        let (text, sfSegments, error): (String, [SFTranscriptionSegment], String?) = await withCheckedContinuation { cont in
            var resumed = false
            var lastText = ""
            var lastSegments: [SFTranscriptionSegment] = []
            task = recognizer.recognitionTask(with: request) { result, error in
                guard !resumed else { return }
                if let result {
                    lastText = result.bestTranscription.formattedString
                    lastSegments = result.bestTranscription.segments
                    if audioDuration > 0, let lastSeg = lastSegments.last {
                        onProgress(min((lastSeg.timestamp + lastSeg.duration) / audioDuration, 0.99))
                    }
                    if result.isFinal {
                        resumed = true
                        onProgress(1.0)
                        cont.resume(returning: (lastText, lastSegments, nil))
                    }
                } else if let error {
                    resumed = true
                    cont.resume(returning: (lastText, lastSegments, error.localizedDescription))
                }
            }
        }

        if let error, text.isEmpty {
            throw HelperError("SFSpeechRecognizer failed: \(error)")
        }
        return Output(text: text, segments: Self.groupSegments(sfSegments, audioDuration: audioDuration))
    }

    private static func groupSegments(_ sfSegments: [SFTranscriptionSegment], audioDuration: Double) -> [TimedSeg] {
        guard !sfSegments.isEmpty else { return [] }

        // SFSpeechURLRecognitionRequest はセグメントの timestamp を全て 0 で返すことがある
        if !sfSegments.contains(where: { $0.timestamp > 0 || $0.duration > 0 }) {
            let sentences = splitIntoSentences(joinSegmentTexts(sfSegments.map(\.substring)))
            return distributeByCharCount(sentences, audioDuration: audioDuration)
        }

        var result: [TimedSeg] = []
        var currentTexts = [sfSegments[0].substring]
        var currentStart = sfSegments[0].timestamp
        var currentEnd = sfSegments[0].timestamp + sfSegments[0].duration

        for i in 1..<sfSegments.count {
            let seg = sfSegments[i]
            let gap = seg.timestamp - currentEnd
            if gap > 1.5 {
                result.append(TimedSeg(text: joinSegmentTexts(currentTexts), start: currentStart, end: currentEnd))
                currentTexts = [seg.substring]
                currentStart = seg.timestamp
            } else {
                currentTexts.append(seg.substring)
            }
            currentEnd = seg.timestamp + seg.duration
        }

        result.append(TimedSeg(text: joinSegmentTexts(currentTexts), start: currentStart, end: currentEnd))
        return result
    }
}
