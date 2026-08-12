import Foundation
import FoundationModels
import Speech

final class HelperServer {
    private let writer = StdoutWriter()
    private let sessionsLock = NSLock()
    private var liveSessions: [String: AnyObject] = [:]
    private let inflight = DispatchGroup()

    func drainAndExit() {
        inflight.notify(queue: .main) { exit(0) }
    }

    func handle(line: String) {
        guard let data = line.data(using: .utf8),
              let obj = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any],
              let method = obj["method"] as? String else {
            logErr("ignoring unparsable request line")
            return
        }
        let id = (obj["id"] as? NSNumber)?.intValue
        let params = obj["params"] as? [String: Any] ?? [:]

        if method == "liveAudio" {
            handleLiveAudio(params)
            return
        }

        inflight.enter()
        Task {
            await self.handleRequest(id: id, method: method, params: params)
            self.inflight.leave()
        }
    }

    private func handleRequest(id: Int?, method: String, params: [String: Any]) async {
        switch method {
        case "capabilities":
            reply(id: id, data: await capabilitiesData())
        case "supportedLocales":
            reply(id: id, data: ["locales": await supportedLocaleIdentifiers()])
        case "batchTranscribe":
            await handleBatchTranscribe(id: id, params: params)
        case "liveStart":
            await handleLiveStart(id: id, params: params)
        case "liveStop":
            await handleLiveStop(id: id, params: params)
        case "generateTitle":
            await handleGenerateTitle(id: id, params: params)
        case "translate":
            await handleTranslate(id: id, params: params)
        default:
            replyError(id: id, error: "unknown method")
        }
    }

    // MARK: - Handlers

    private func capabilitiesData() async -> [String: Any] {
        var speechAnalyzer = false
        var foundationModels = false
        var translation = false
        if #available(macOS 26.0, *) {
            speechAnalyzer = true
            foundationModels = TitleGenerator.isModelAvailable
            translation = await Translator.hasSupportedLanguages()
        }
        let sfSpeech = SFSpeechRecognizer(locale: Locale(identifier: "en-US")) != nil
        let v = ProcessInfo.processInfo.operatingSystemVersion
        return [
            "speechAnalyzer": speechAnalyzer,
            "sfSpeech": sfSpeech,
            "foundationModels": foundationModels,
            "translation": translation,
            "os": "\(v.majorVersion).\(v.minorVersion)",
        ]
    }

    private func supportedLocaleIdentifiers() async -> [String] {
        var ids = Set(SFSpeechRecognizer.supportedLocales().map { $0.identifier(.bcp47) })
        if #available(macOS 26.0, *) {
            let analyzerLocales = await SpeechTranscriber.supportedLocales
            ids.formUnion(analyzerLocales.map { $0.identifier(.bcp47) })
        }
        return ids.sorted()
    }

    private func handleBatchTranscribe(id: Int?, params: [String: Any]) async {
        guard let file = params["file"] as? String else {
            replyError(id: id, error: "missing param: file")
            return
        }
        let locale = LocaleResolver.resolve(params["locale"] as? String)
        let transcriber = BatchTranscriber(locale: locale)
        let progressLock = NSLock()
        var lastReported = -1.0
        do {
            let output = try await transcriber.transcribe(url: URL(fileURLWithPath: file)) { [weak self] fraction in
                let shouldEmit: Bool = progressLock.withLock {
                    guard fraction >= 1.0 || fraction - lastReported >= 0.01 else { return false }
                    lastReported = fraction
                    return true
                }
                if shouldEmit, let id {
                    self?.writer.send(["id": id, "event": "progress", "fraction": fraction])
                }
            }
            reply(id: id, data: ["text": output.text, "segments": output.segments.map(\.json)])
        } catch {
            replyError(id: id, error: errorMessage(error))
        }
    }

    private func handleLiveStart(id: Int?, params: [String: Any]) async {
        guard #available(macOS 26.0, *) else {
            replyError(id: id, error: "SpeechAnalyzer requires macOS 26 or later")
            return
        }
        guard let streamId = params["streamId"] as? String else {
            replyError(id: id, error: "missing param: streamId")
            return
        }
        let existing: Bool = sessionsLock.withLock { liveSessions[streamId] != nil }
        if existing {
            replyError(id: id, error: "stream already started: \(streamId)")
            return
        }
        let locale = LocaleResolver.resolve(params["locale"] as? String)
        let sampleRate = (params["sampleRate"] as? NSNumber)?.doubleValue ?? 16000
        let channels = (params["channels"] as? NSNumber)?.intValue ?? 1

        do {
            let session = try LiveSession(
                locale: locale,
                sampleRate: sampleRate,
                channels: channels,
                onPartial: { [weak self] text in
                    var event: [String: Any] = ["event": "partial", "streamId": streamId, "text": text]
                    if let id { event["id"] = id }
                    self?.writer.send(event)
                },
                onFinal: { [weak self] segment in
                    var event: [String: Any] = ["event": "final", "streamId": streamId, "segment": segment.json]
                    if let id { event["id"] = id }
                    self?.writer.send(event)
                }
            )
            try await session.start()
            sessionsLock.withLock { liveSessions[streamId] = session }
            reply(id: id, data: [:])
        } catch {
            replyError(id: id, error: errorMessage(error))
        }
    }

    private func handleLiveAudio(_ params: [String: Any]) {
        guard #available(macOS 26.0, *) else { return }
        guard let streamId = params["streamId"] as? String,
              let pcm = params["pcm"] as? String else {
            logErr("liveAudio: missing streamId or pcm")
            return
        }
        guard let session = sessionsLock.withLock({ liveSessions[streamId] }) as? LiveSession else {
            logErr("liveAudio: unknown stream \(streamId)")
            return
        }
        session.append(base64: pcm)
    }

    private func handleLiveStop(id: Int?, params: [String: Any]) async {
        guard #available(macOS 26.0, *) else {
            replyError(id: id, error: "SpeechAnalyzer requires macOS 26 or later")
            return
        }
        guard let streamId = params["streamId"] as? String else {
            replyError(id: id, error: "missing param: streamId")
            return
        }
        guard let session = sessionsLock.withLock({ liveSessions.removeValue(forKey: streamId) }) as? LiveSession else {
            replyError(id: id, error: "unknown stream: \(streamId)")
            return
        }
        let segments = await session.stop()
        reply(id: id, data: ["segments": segments.map(\.json)])
    }

    private func handleGenerateTitle(id: Int?, params: [String: Any]) async {
        guard #available(macOS 26.0, *) else {
            replyError(id: id, error: "FoundationModels requires macOS 26 or later")
            return
        }
        guard let text = params["text"] as? String else {
            replyError(id: id, error: "missing param: text")
            return
        }
        let lang = params["lang"] as? String ?? "ja"
        do {
            if let title = try await TitleGenerator.generate(text: text, lang: lang) {
                reply(id: id, data: ["title": title])
            } else {
                reply(id: id, data: [:])
            }
        } catch {
            replyError(id: id, error: errorMessage(error))
        }
    }

    private func handleTranslate(id: Int?, params: [String: Any]) async {
        guard #available(macOS 26.0, *) else {
            replyError(id: id, error: "Translation requires macOS 26 or later")
            return
        }
        guard let texts = params["texts"] as? [String] else {
            replyError(id: id, error: "missing param: texts")
            return
        }
        guard let target = params["target"] as? String else {
            replyError(id: id, error: "missing param: target")
            return
        }
        let results = await Translator.translate(texts: texts, target: target)
        let payload: [Any] = results.map { value in
            if let value { return value }
            return NSNull()
        }
        reply(id: id, data: ["translations": payload])
    }

    // MARK: - Responses

    private func reply(id: Int?, data: [String: Any]) {
        guard let id else { return }
        writer.send(["id": id, "ok": true, "data": data])
    }

    private func replyError(id: Int?, error: String) {
        guard let id else {
            logErr("request without id failed: \(error)")
            return
        }
        writer.send(["id": id, "ok": false, "error": error])
    }
}
