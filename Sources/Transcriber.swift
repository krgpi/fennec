import AVFoundation
import Speech

final class Transcriber {
    private let locale: Locale
    private var task: SFSpeechRecognitionTask?
    private var analyzer: SpeechAnalyzer?
    private let contextualStrings: [String]

    init(locale: Locale = .current, contextualStrings: [String] = []) {
        self.locale = locale
        self.contextualStrings = contextualStrings
    }

    static func requestAuthorization() async -> Bool {
        await withCheckedContinuation { cont in
            SFSpeechRecognizer.requestAuthorization { status in
                cont.resume(returning: status == .authorized)
            }
        }
    }

    struct Result {
        var text: String
        var segments: [TimedSegment]
        var error: String?
    }

    func transcribe(url: URL, onUpdate: @escaping (String) -> Void, onProgress: ((Double) -> Void)? = nil) async -> Result {
        let asset = AVURLAsset(url: url)
        let audioDuration = (try? await asset.load(.duration).seconds) ?? 0

        do {
            let result = try await transcribeWithAnalyzer(url: url, audioDuration: audioDuration, onUpdate: onUpdate, onProgress: onProgress)
            return result
        } catch {
            let legacyResult = await transcribeWithSFSpeech(url: url, audioDuration: audioDuration, onUpdate: onUpdate, onProgress: onProgress)
            return legacyResult
        }
    }

    private func transcribeWithAnalyzer(url: URL, audioDuration: Double, onUpdate: @escaping (String) -> Void, onProgress: ((Double) -> Void)?) async throws -> Result {
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
            throw TranscriberError.noFormat
        }
        let analyzer = SpeechAnalyzer(modules: [transcriber])
        self.analyzer = analyzer
        let (inputSequence, builder) = AsyncStream<AnalyzerInput>.makeStream()
        try await analyzer.start(inputSequence: inputSequence)

        let feedTask = Task {
            await self.feedAudioFile(url: url, targetFormat: format, builder: builder, audioDuration: audioDuration, onProgress: onProgress)
        }

        var finalizedText = ""
        var rawTexts: [String] = []

        do {
            for try await result in transcriber.results {
                let text = String(result.text.characters)
                if result.isFinal {
                    rawTexts.append(text)
                    finalizedText += text
                    DispatchQueue.main.async { onUpdate(finalizedText) }
                } else {
                    DispatchQueue.main.async { onUpdate(finalizedText + text) }
                }
            }
        } catch {
            // results stream ended
        }

        await feedTask.value
        self.analyzer = nil

        var segments: [TimedSegment] = []
        let totalChars = rawTexts.reduce(0) { $0 + $1.count }
        var charOffset = 0
        for text in rawTexts {
            let start = totalChars > 0 ? audioDuration * Double(charOffset) / Double(totalChars) : 0
            let endCharOffset = charOffset + text.count
            let end = totalChars > 0 ? audioDuration * Double(endCharOffset) / Double(totalChars) : start + 1.0
            segments.append(TimedSegment(text: text, start: start, end: end))
            charOffset = endCharOffset
        }

        return Result(text: finalizedText, segments: segments, error: nil)
    }

    private func feedAudioFile(url: URL, targetFormat: AVAudioFormat, builder: AsyncStream<AnalyzerInput>.Continuation, audioDuration: Double, onProgress: ((Double) -> Void)?) async {
        guard let audioFile = try? AVAudioFile(forReading: url) else {
            builder.finish()
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
                let progress = min(Double(totalFramesRead) / Double(totalFrames), 0.99)
                DispatchQueue.main.async { onProgress?(progress) }
            }
        }

        builder.finish()
        if let analyzer {
            try? await analyzer.finalizeAndFinishThroughEndOfInput()
        }
        DispatchQueue.main.async { onProgress?(1.0) }
    }

    func cancel() {
        task?.cancel()
        task = nil
        analyzer = nil
    }

    private func transcribeWithSFSpeech(url: URL, audioDuration: Double, onUpdate: @escaping (String) -> Void, onProgress: ((Double) -> Void)?) async -> Result {
        guard let recognizer = SFSpeechRecognizer(locale: locale), recognizer.isAvailable else {
            return Result(text: "", segments: [], error: String(localized: "音声認識エンジンが利用できません"))
        }

        let request = SFSpeechURLRecognitionRequest(url: url)
        request.shouldReportPartialResults = true
        request.taskHint = .dictation
        if !contextualStrings.isEmpty {
            request.contextualStrings = contextualStrings
        }

        return await withCheckedContinuation { cont in
            var resumed = false
            var lastText = ""
            var lastSegments: [SFTranscriptionSegment] = []
            task = recognizer.recognitionTask(with: request) { result, error in
                guard !resumed else { return }
                if let result {
                    lastText = result.bestTranscription.formattedString
                    lastSegments = result.bestTranscription.segments
                    DispatchQueue.main.async { onUpdate(lastText) }
                    if audioDuration > 0, let lastSeg = lastSegments.last {
                        let progress = min((lastSeg.timestamp + lastSeg.duration) / audioDuration, 0.99)
                        DispatchQueue.main.async { onProgress?(progress) }
                    }
                    if result.isFinal {
                        resumed = true
                        DispatchQueue.main.async { onProgress?(1.0) }
                        let segments = Self.groupSegments(lastSegments)
                        cont.resume(returning: Result(text: lastText, segments: segments, error: nil))
                    }
                } else if let error {
                    resumed = true
                    let segments = Self.groupSegments(lastSegments)
                    cont.resume(returning: Result(text: lastText, segments: segments, error: "\(url.lastPathComponent): \(error.localizedDescription)"))
                }
            }
        }
    }

    private static func groupSegments(_ sfSegments: [SFTranscriptionSegment]) -> [TimedSegment] {
        guard !sfSegments.isEmpty else { return [] }

        var result: [TimedSegment] = []
        var currentTexts = [sfSegments[0].substring]
        var currentStart = sfSegments[0].timestamp
        var currentEnd = sfSegments[0].timestamp + sfSegments[0].duration

        for i in 1..<sfSegments.count {
            let seg = sfSegments[i]
            let gap = seg.timestamp - currentEnd
            if gap > 1.5 {
                result.append(TimedSegment(
                    text: joinSegmentTexts(currentTexts),
                    start: currentStart,
                    end: currentEnd
                ))
                currentTexts = [seg.substring]
                currentStart = seg.timestamp
            } else {
                currentTexts.append(seg.substring)
            }
            currentEnd = seg.timestamp + seg.duration
        }

        result.append(TimedSegment(
            text: joinSegmentTexts(currentTexts),
            start: currentStart,
            end: currentEnd
        ))

        return result
    }
}

private enum TranscriberError: Error {
    case noFormat
}
