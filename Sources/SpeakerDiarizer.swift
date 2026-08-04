#if DEBUG
import Foundation
import SpeakerKit
import WhisperKit

final class SpeakerDiarizer: ObservableObject {
    @Published var isModelReady = false
    @Published var isDownloading = false
    @Published var isLoading = false
    @Published var downloadError: String?

    private var speakerKit: SpeakerKit?

    init() {
        checkModelExists()
    }

    private var modelDirectory: URL? {
        FileManager.default.urls(for: .documentDirectory, in: .userDomainMask).first?
            .appendingPathComponent("huggingface/models/argmaxinc/speakerkit-coreml")
    }

    private func checkModelExists() {
        if let modelDir = modelDirectory, FileManager.default.fileExists(atPath: modelDir.path) {
            isModelReady = true
        }
    }

    func deleteModel() {
        if let modelDir = modelDirectory {
            try? FileManager.default.removeItem(at: modelDir)
        }
        speakerKit = nil
        isModelReady = false
    }

    func downloadModel() async {
        await MainActor.run {
            isDownloading = true
            downloadError = nil
        }

        do {
            let config = PyannoteConfig(
                download: true,
                load: false,
                verbose: false,
                logLevel: .error
            )
            speakerKit = try await SpeakerKit(config)

            await MainActor.run {
                isModelReady = true
                isDownloading = false
            }
        } catch {
            await MainActor.run {
                downloadError = error.localizedDescription
                isDownloading = false
            }
        }
    }

    func diarize(url: URL, numberOfSpeakers: Int? = nil) async throws -> [SpeakerTimedSegment] {
        if speakerKit == nil {
            let config = PyannoteConfig(
                download: true,
                load: true,
                verbose: false,
                logLevel: .error
            )
            speakerKit = try await SpeakerKit(config)
            await MainActor.run { isModelReady = true }
        }

        let audioArray = try AudioProcessor.loadAudioAsFloatArray(fromPath: url.path)

        let options = PyannoteDiarizationOptions(numberOfSpeakers: numberOfSpeakers)
        let result = try await speakerKit!.diarize(audioArray: audioArray, options: options)

        return result.segments.map { seg in
            SpeakerTimedSegment(
                speakerId: seg.speaker.speakerId ?? -1,
                start: TimeInterval(seg.startTime),
                end: TimeInterval(seg.endTime)
            )
        }
    }
}

struct SpeakerTimedSegment {
    let speakerId: Int
    let start: TimeInterval
    let end: TimeInterval
}
#endif
