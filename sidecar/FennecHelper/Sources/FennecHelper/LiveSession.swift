import AVFoundation
import Foundation
import Speech

@available(macOS 26.0, *)
final class LiveSession {
    private let locale: Locale
    private let sampleRate: Double
    private let channels: Int
    private let sourceFormat: AVAudioFormat
    private let onPartial: (String) -> Void
    private let onFinal: (TimedSeg) -> Void

    private let lock = NSLock()
    private var analyzer: SpeechAnalyzer?
    private var inputBuilder: AsyncStream<AnalyzerInput>.Continuation?
    private var analyzerFormat: AVAudioFormat?
    private var converter: AVAudioConverter?
    private var resultTask: Task<Void, Never>?
    private var totalFrames: Int64 = 0
    private var segmentStart: Double = 0
    private var segments: [TimedSeg] = []
    private var stopped = false

    init(locale: Locale, sampleRate: Double, channels: Int, onPartial: @escaping (String) -> Void, onFinal: @escaping (TimedSeg) -> Void) throws {
        guard sampleRate > 0, channels > 0,
              let format = AVAudioFormat(commonFormat: .pcmFormatFloat32, sampleRate: sampleRate, channels: AVAudioChannelCount(channels), interleaved: true) else {
            throw HelperError("unsupported audio format: sampleRate=\(sampleRate) channels=\(channels)")
        }
        self.locale = locale
        self.sampleRate = sampleRate
        self.channels = channels
        self.sourceFormat = format
        self.onPartial = onPartial
        self.onFinal = onFinal
    }

    func start() async throws {
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

        let alreadyStopped: Bool = lock.withLock {
            guard !stopped else { return true }
            self.analyzer = analyzer
            self.inputBuilder = builder
            self.analyzerFormat = format
            return false
        }
        if alreadyStopped {
            builder.finish()
            try? await analyzer.finalizeAndFinishThroughEndOfInput()
            return
        }

        resultTask = Task { [weak self] in
            await self?.consumeResults(from: transcriber)
        }
    }

    private func consumeResults(from transcriber: SpeechTranscriber) async {
        do {
            for try await result in transcriber.results {
                let text = String(result.text.characters)
                if result.isFinal {
                    guard !text.isEmpty else { continue }
                    let segment: TimedSeg = lock.withLock {
                        let now = Double(totalFrames) / sampleRate
                        let seg = TimedSeg(text: text, start: segmentStart, end: now)
                        segmentStart = now
                        segments.append(seg)
                        return seg
                    }
                    onFinal(segment)
                } else if !text.isEmpty {
                    onPartial(text)
                }
            }
        } catch {
            logErr("live results stream error: \(errorMessage(error))")
        }
    }

    func append(base64 pcm: String) {
        guard let data = Data(base64Encoded: pcm) else {
            logErr("liveAudio: invalid base64 payload")
            return
        }
        let frameCount = data.count / 4 / channels
        guard frameCount > 0 else { return }

        lock.lock()
        defer { lock.unlock() }
        guard !stopped, let inputBuilder, let analyzerFormat else { return }

        guard let sourceBuffer = AVAudioPCMBuffer(pcmFormat: sourceFormat, frameCapacity: AVAudioFrameCount(frameCount)) else { return }
        sourceBuffer.frameLength = AVAudioFrameCount(frameCount)
        let abl = sourceBuffer.mutableAudioBufferList
        let byteCount = min(data.count, Int(abl.pointee.mBuffers.mDataByteSize))
        data.withUnsafeBytes { raw in
            if let dst = abl.pointee.mBuffers.mData, let src = raw.baseAddress {
                memcpy(dst, src, byteCount)
            }
        }

        let sendBuffer: AVAudioPCMBuffer
        if sourceFormat == analyzerFormat {
            sendBuffer = sourceBuffer
        } else {
            if converter == nil {
                converter = AVAudioConverter(from: sourceFormat, to: analyzerFormat)
            }
            guard let converter else { return }
            let capacity = AVAudioFrameCount(Double(sourceBuffer.frameLength) * analyzerFormat.sampleRate / sourceFormat.sampleRate) + 16
            guard let out = AVAudioPCMBuffer(pcmFormat: analyzerFormat, frameCapacity: capacity) else { return }
            var consumed = false
            var error: NSError?
            converter.convert(to: out, error: &error) { _, outStatus in
                if consumed {
                    outStatus.pointee = .noDataNow
                    return nil
                }
                consumed = true
                outStatus.pointee = .haveData
                return sourceBuffer
            }
            guard error == nil, out.frameLength > 0 else { return }
            sendBuffer = out
        }

        inputBuilder.yield(AnalyzerInput(buffer: sendBuffer))
        totalFrames += Int64(frameCount)
    }

    func stop() async -> [TimedSeg] {
        let (builder, activeAnalyzer): (AsyncStream<AnalyzerInput>.Continuation?, SpeechAnalyzer?) = lock.withLock {
            guard !stopped else { return (nil, nil) }
            stopped = true
            let b = inputBuilder
            let a = analyzer
            inputBuilder = nil
            converter = nil
            return (b, a)
        }
        builder?.finish()
        if let activeAnalyzer {
            try? await activeAnalyzer.finalizeAndFinishThroughEndOfInput()
        }
        await resultTask?.value
        return lock.withLock {
            analyzer = nil
            return segments
        }
    }
}
