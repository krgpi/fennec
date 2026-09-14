import AVFoundation
import Foundation
import Speech

private final class ResumeGuard: @unchecked Sendable {
    private let lock = NSLock()
    private var resumed = false
    var isResumed: Bool { lock.withLock { resumed } }
    /// 呼び出し前に resumed だったかを返す（二重 resume 防止のため呼び出し側で使う）
    func markResumed() -> Bool { lock.withLock { let already = resumed; resumed = true; return already } }
}

private final class ActivityTracker: @unchecked Sendable {
    private let lock = NSLock()
    private var lastActivityAt = Date()

    func touch() {
        lock.withLock { lastActivityAt = Date() }
    }

    func idleSeconds() -> TimeInterval {
        lock.withLock { Date().timeIntervalSince(lastActivityAt) }
    }
}

private final class SFRecognitionState: @unchecked Sendable {
    let resumeGuard = ResumeGuard()
    let activity = ActivityTracker()
    private let lock = NSLock()
    var lastSegments: [SFTranscriptionSegment] {
        get { lock.withLock { _lastSegments } }
        set { lock.withLock { _lastSegments = newValue } }
    }
    private var _lastSegments: [SFTranscriptionSegment] = []

    var isResumed: Bool { resumeGuard.isResumed }

    func touch() { activity.touch() }
    func idleSeconds() -> TimeInterval { activity.idleSeconds() }
    func markResumed() -> Bool { resumeGuard.markResumed() }
}

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

        // ほぼ無音・無音区間が極端に長い音声では SpeechAnalyzer/SFSpeechRecognizer が
        // 応答を返さないまま停止することがある。音声の長さでタイムアウトを決めると
        // 長時間の無音音声ほど猶予が伸びて意味がなくなるため、進捗イベントの間隔
        // （無応答時間）で判定する
        let activity = ActivityTracker()
        let trackedProgress: (Double) -> Void = { fraction in
            activity.touch()
            onProgress(fraction)
        }
        return try await withIdleTimeout(activity: activity, idleSeconds: 90) { [self] in
            if #available(macOS 26.0, *) {
                do {
                    return try await transcribeWithAnalyzer(url: url, audioDuration: audioDuration, onProgress: trackedProgress)
                } catch {
                    logErr("SpeechAnalyzer batch failed (\(errorMessage(error))), falling back to SFSpeechRecognizer")
                }
            }
            return try await transcribeWithSFSpeech(url: url, audioDuration: audioDuration, onProgress: trackedProgress)
        }
    }

    /// operation が idleSeconds の間 activity への touch なしに応答しない場合に打ち切る。
    /// 打ち切っても内部で止まったタスクは残り得るが、呼び出し元をブロックし続けないことを優先する
    private func withIdleTimeout<T: Sendable>(activity: ActivityTracker, idleSeconds: TimeInterval, _ operation: @escaping @Sendable () async throws -> T) async throws -> T {
        let guardBox = ResumeGuard()

        return try await withCheckedThrowingContinuation { cont in
            func finish(_ result: Result<T, Error>) {
                guard !guardBox.markResumed() else { return }
                cont.resume(with: result)
            }

            Task {
                do {
                    finish(.success(try await operation()))
                } catch {
                    finish(.failure(error))
                }
            }

            Task {
                while !guardBox.isResumed {
                    try? await Task.sleep(nanoseconds: 5_000_000_000)
                    if guardBox.isResumed { return }
                    if activity.idleSeconds() > idleSeconds {
                        finish(.failure(HelperError("batch transcription stalled (no progress for \(Int(idleSeconds))s)")))
                        return
                    }
                }
            }
        }
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
        let (inputSequence, builder) = AsyncStream<AnalyzerInput>.makeStream(
            bufferingPolicy: .bufferingOldest(Self.maxPendingBuffers)
        )
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

    private static let maxPendingBuffers = 24

    @available(macOS 26.0, *)
    private static func feed(_ buffer: AVAudioPCMBuffer, to builder: AsyncStream<AnalyzerInput>.Continuation) async -> Bool {
        let input = AnalyzerInput(buffer: buffer)
        while true {
            switch builder.yield(input) {
            case .enqueued:
                return true
            case .terminated:
                return false
            case .dropped:
                // bufferingOldest なので溢れたのは今回の要素。解析が追いつくまで待って再投入する
                try? await Task.sleep(nanoseconds: 20_000_000)
            @unknown default:
                return false
            }
        }
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

            guard await feed(sendBuffer, to: builder) else { break }

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
        if recognizer.supportsOnDeviceRecognition {
            request.requiresOnDeviceRecognition = true
        }

        let state = SFRecognitionState()
        let (text, sfSegments, error): (String, [SFTranscriptionSegment], String?) = await withCheckedContinuation { cont in
            func finish(_ result: (String, [SFTranscriptionSegment], String?)) {
                let alreadyResumed = state.markResumed()
                guard !alreadyResumed else { return }
                cont.resume(returning: result)
            }

            task = recognizer.recognitionTask(with: request) { result, error in
                autoreleasepool {
                    guard !state.isResumed else { return }
                    state.touch()
                    if let result {
                        let transcription = result.bestTranscription
                        state.lastSegments = transcription.segments
                        if audioDuration > 0, let lastSeg = transcription.segments.last {
                            onProgress(min((lastSeg.timestamp + lastSeg.duration) / audioDuration, 0.99))
                        }
                        if result.isFinal {
                            onProgress(1.0)
                            // formattedString は文字起こし全体を組み立てるので確定時だけ呼ぶ
                            finish((transcription.formattedString, transcription.segments, nil))
                        }
                    } else if let error {
                        let text = joinSegmentTexts(state.lastSegments.map(\.substring))
                        finish((text, state.lastSegments, error.localizedDescription))
                    }
                }
            }

            // 無音・無音区間が非常に長い録音では SFSpeechRecognizer の completion handler が
            // 一度も呼ばれずに無応答のままになることがあるため、無応答が続いたら打ち切る
            Task {
                while !state.isResumed {
                    try? await Task.sleep(nanoseconds: 5_000_000_000)
                    if state.isResumed { return }
                    if state.idleSeconds() > 60 {
                        let text = joinSegmentTexts(state.lastSegments.map(\.substring))
                        finish((text, state.lastSegments, "timed out waiting for SFSpeechRecognizer"))
                        return
                    }
                }
            }
        }
        task?.cancel()
        task = nil

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
