import AVFoundation
import Accelerate
import Foundation
import WhisperKit

final class WhisperTranscriber {
    private var whisperKit: WhisperKit?
    private var loadedModelPath: String?

    struct Result {
        var text: String
        var segments: [TimedSegment]
        var error: String?
    }

    private static let hallucinationPatterns: Set<String> = [
        "ご視聴ありがとうございました",
        "ありがとうございました",
        "チャンネル登録よろしくお願いします",
        "チャンネル登録お願いします",
        "おやすみなさい",
        "お疲れ様でした",
    ]

    func transcribe(url: URL, modelPath: String, language: String = "ja", onUpdate: @escaping (String) -> Void, onProgress: ((Double) -> Void)? = nil, isCancelled: (@Sendable () -> Bool)? = nil) async -> Result {
        if isSilent(url: url) {
            return Result(text: "", segments: [], error: nil)
        }

        do {
            if whisperKit == nil || loadedModelPath != modelPath {
                whisperKit = try await WhisperKit(
                    downloadBase: WhisperModelManager.downloadBase,
                    modelFolder: modelPath,
                    verbose: false,
                    logLevel: .error,
                    download: false
                )
                loadedModelPath = modelPath
            }

            guard let wk = whisperKit else {
                return Result(text: "", segments: [], error: String(localized: "Whisperモデルの初期化に失敗しました"))
            }

            let audioDuration: Double
            if onProgress != nil {
                let asset = AVURLAsset(url: url)
                audioDuration = (try? await asset.load(.duration).seconds) ?? 0
            } else {
                audioDuration = 0
            }
            let estimatedWindows = max(1, Int(ceil(audioDuration / 30.0)))

            let options = DecodingOptions(
                language: language,
                skipSpecialTokens: true,
                suppressBlank: true,
                compressionRatioThreshold: 2.4,
                logProbThreshold: -0.7,
                noSpeechThreshold: 0.3,
                chunkingStrategy: .vad
            )

            let callback: TranscriptionCallback = { progress in
                if audioDuration > 0 {
                    let fraction = min(Double(progress.windowId + 1) / Double(estimatedWindows), 0.99)
                    DispatchQueue.main.async { onProgress?(fraction) }
                }
                if let isCancelled, isCancelled() { return false }
                return nil
            }
            let results = try await wk.transcribe(audioPath: url.path, decodeOptions: options, callback: callback)

            let allSegments = results.flatMap { $0.segments }.filter { seg in
                if seg.noSpeechProb > 0.6 { return false }
                if seg.avgLogprob < -0.9 { return false }
                let cleaned = seg.text.trimmingCharacters(in: .whitespacesAndNewlines)
                if cleaned.isEmpty { return false }
                if Self.hallucinationPatterns.contains(cleaned) { return false }
                return true
            }

            var filteredSegments: [TranscriptionSegment] = []
            for seg in allSegments {
                let isDuplicate = filteredSegments.contains { existing in
                    existing.text == seg.text && abs(existing.start - seg.start) < 5.0
                }
                if !isDuplicate {
                    filteredSegments.append(seg)
                }
            }

            let text = filteredSegments.map { $0.text.trimmingCharacters(in: .whitespaces) }
                .joined(separator: " ")
                .trimmingCharacters(in: .whitespaces)

            let segments = filteredSegments.map { seg in
                TimedSegment(
                    text: seg.text.trimmingCharacters(in: .whitespaces),
                    start: TimeInterval(seg.start),
                    end: TimeInterval(seg.end)
                )
            }

            DispatchQueue.main.async { onProgress?(1.0) }
            await MainActor.run { onUpdate(text) }
            return Result(text: text, segments: segments, error: nil)
        } catch {
            whisperKit = nil
            loadedModelPath = nil
            return Result(text: "", segments: [], error: "\(url.lastPathComponent): \(error.localizedDescription)")
        }
    }

    private func isSilent(url: URL, threshold: Float = 0.001) -> Bool {
        guard let file = try? AVAudioFile(forReading: url) else { return true }
        let length = AVAudioFrameCount(file.length)
        guard length > 0,
              let buffer = AVAudioPCMBuffer(pcmFormat: file.processingFormat, frameCapacity: length)
        else { return true }
        do { try file.read(into: buffer) } catch { return true }
        guard let data = buffer.floatChannelData else { return true }
        let channelCount = Int(buffer.format.channelCount)
        let frameCount = Int(buffer.frameLength)
        var maxRMS: Float = 0
        for ch in 0..<channelCount {
            var rms: Float = 0
            vDSP_rmsqv(data[ch], 1, &rms, vDSP_Length(frameCount))
            maxRMS = max(maxRMS, rms)
        }
        return maxRMS < threshold
    }
}
