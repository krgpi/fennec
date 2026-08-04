import AVFoundation
import Speech

final class StreamingTranscriber {
    private let locale: Locale
    private var transcriber: SpeechTranscriber?
    private var analyzer: SpeechAnalyzer?
    private var inputBuilder: AsyncStream<AnalyzerInput>.Continuation?
    private var analyzerFormat: AVAudioFormat?
    private var converter: AVAudioConverter?
    private var converterSourceFormat: AVAudioFormat?
    private var resultTask: Task<Void, Never>?
    private var setupTask: Task<Void, Never>?
    private var onUpdate: ((String) -> Void)?
    private var timeProvider: (() -> TimeInterval)?
    private var finalizedText = ""
    private var volatileText = ""
    private var isStopped = false
    private(set) var segments: [TimedSegment] = []
    private var segmentStart: TimeInterval = 0

    private let lock = NSLock()
    private(set) var appendCount = 0
    private(set) var resultCount = 0
    private(set) var errorCount = 0
    private(set) var sessionCount = 0
    private(set) var lastError: String?
    private(set) var lastResultLength = 0

    init(locale: Locale = .current, contextualStrings: [String] = []) {
        self.locale = locale
    }

    var diagnosticStatus: String {
        "SpeechAnalyzer locale=\(locale.identifier)"
    }

    func start(timeProvider: @escaping () -> TimeInterval = { 0 }, onUpdate: @escaping (String) -> Void) {
        self.onUpdate = onUpdate
        self.timeProvider = timeProvider
        finalizedText = ""
        volatileText = ""
        segments = []
        isStopped = false

        setupTask = Task { [weak self] in
            await self?.startSession()
        }
    }

    private func startSession() async {
        do {
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
                lock.withLock { lastError = "no analyzer format" }
                return
            }
            let analyzer = SpeechAnalyzer(modules: [transcriber])
            let (inputSequence, builder) = AsyncStream<AnalyzerInput>.makeStream()
            try await analyzer.start(inputSequence: inputSequence)

            let stopped = lock.withLock { () -> Bool in
                guard !isStopped else { return true }
                self.transcriber = transcriber
                self.analyzer = analyzer
                self.inputBuilder = builder
                self.analyzerFormat = format
                self.sessionCount += 1
                return false
            }
            if stopped {
                builder.finish()
                try? await analyzer.finalizeAndFinishThroughEndOfInput()
                return
            }

            resultTask = Task { [weak self] in
                await self?.consumeResults(from: transcriber)
            }
        } catch {
            let ns = error as NSError
            lock.withLock {
                errorCount += 1
                lastError = "\(ns.domain)#\(ns.code)"
            }
        }
    }

    private func consumeResults(from transcriber: SpeechTranscriber) async {
        do {
            for try await result in transcriber.results {
                let text = String(result.text.characters)
                let fullText: String = lock.withLock {
                    resultCount += 1
                    lastResultLength = text.count
                    if result.isFinal {
                        let now = timeProvider?() ?? 0
                        segments.append(TimedSegment(text: text, start: segmentStart, end: now))
                        segmentStart = now
                        finalizedText += text
                        volatileText = ""
                    } else {
                        volatileText = text
                    }
                    return finalizedText + volatileText
                }
                if !fullText.isEmpty {
                    DispatchQueue.main.async { [weak self] in
                        self?.onUpdate?(fullText)
                    }
                }
            }
        } catch {
            let ns = error as NSError
            lock.withLock {
                errorCount += 1
                lastError = "\(ns.domain)#\(ns.code)"
            }
        }
    }

    func append(_ sampleBuffer: CMSampleBuffer) {
        guard let formatDesc = CMSampleBufferGetFormatDescription(sampleBuffer),
              let asbd = CMAudioFormatDescriptionGetStreamBasicDescription(formatDesc),
              let sourceFormat = AVAudioFormat(streamDescription: asbd),
              let sourcePCM = Self.convertToPCMBuffer(sampleBuffer, format: sourceFormat) else { return }

        lock.withLock {
            guard let inputBuilder, let analyzerFormat else { return }

            let sendBuffer: AVAudioPCMBuffer
            if sourceFormat == analyzerFormat {
                sendBuffer = sourcePCM
            } else {
                if converter == nil || converterSourceFormat != sourceFormat {
                    converter = AVAudioConverter(from: sourceFormat, to: analyzerFormat)
                    converterSourceFormat = sourceFormat
                }
                guard let converter else { return }
                let capacity = AVAudioFrameCount(Double(sourcePCM.frameLength) * analyzerFormat.sampleRate / sourceFormat.sampleRate) + 16
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
                    return sourcePCM
                }
                guard error == nil, out.frameLength > 0 else { return }
                sendBuffer = out
            }

            inputBuilder.yield(AnalyzerInput(buffer: sendBuffer))
            appendCount += 1
        }
    }

    func stop() async {
        let (builder, activeAnalyzer): (AsyncStream<AnalyzerInput>.Continuation?, SpeechAnalyzer?) = lock.withLock {
            guard !isStopped else { return (nil, nil) }
            isStopped = true
            let b = inputBuilder
            let a = analyzer
            inputBuilder = nil
            converter = nil
            converterSourceFormat = nil
            return (b, a)
        }
        await setupTask?.value
        builder?.finish()
        if let activeAnalyzer {
            try? await activeAnalyzer.finalizeAndFinishThroughEndOfInput()
        }
        await resultTask?.value
        analyzer = nil
        transcriber = nil
    }

    private static func convertToPCMBuffer(_ sampleBuffer: CMSampleBuffer, format: AVAudioFormat) -> AVAudioPCMBuffer? {
        let numSamples = CMSampleBufferGetNumSamples(sampleBuffer)
        guard numSamples > 0,
              let pcmBuffer = AVAudioPCMBuffer(pcmFormat: format, frameCapacity: AVAudioFrameCount(numSamples)) else { return nil }
        pcmBuffer.frameLength = AVAudioFrameCount(numSamples)

        let status = CMSampleBufferCopyPCMDataIntoAudioBufferList(
            sampleBuffer,
            at: 0,
            frameCount: Int32(numSamples),
            into: pcmBuffer.mutableAudioBufferList
        )
        guard status == noErr else { return nil }

        return pcmBuffer
    }
}
