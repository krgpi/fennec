import Foundation
import WhisperKit

enum TranscriptionEngineType: String {
    case apple
    case whisper
}

struct WhisperModelInfo: Identifiable, Hashable {
    let id: String
    let name: String
    let size: String
    let detail: String

    static let all: [WhisperModelInfo] = [
        WhisperModelInfo(
            id: "openai_whisper-small",
            name: "Small",
            size: String(localized: "約250MB"),
            detail: String(localized: "高速・省メモリ")
        ),
        WhisperModelInfo(
            id: "openai_whisper-large-v3-v20240930_626MB",
            name: String(localized: "Large V3（推奨）"),
            size: String(localized: "約600MB"),
            detail: String(localized: "高精度・量子化済み")
        ),
        WhisperModelInfo(
            id: "openai_whisper-large-v3-v20240930",
            name: String(localized: "Large V3（フル）"),
            size: String(localized: "約3GB"),
            detail: String(localized: "最高精度")
        ),
    ]
}

final class WhisperModelManager: ObservableObject {
    @Published var selectedModelId: String {
        didSet {
            UserDefaults.standard.set(selectedModelId, forKey: "whisperModelId")
            refreshModelReady()
        }
    }

    @Published var isEnabled: Bool {
        didSet {
            UserDefaults.standard.set(isEnabled, forKey: "whisperEnabled")
        }
    }

    @Published var isDownloading = false
    @Published var downloadProgress: Double = 0
    @Published var downloadError: String?
    @Published private(set) var isModelReady = false

    private var downloadedModels: [String: String] {
        didSet { UserDefaults.standard.set(downloadedModels, forKey: "whisperDownloadedModels") }
    }

    var selectedModel: WhisperModelInfo {
        WhisperModelInfo.all.first { $0.id == selectedModelId } ?? WhisperModelInfo.all[1]
    }

    var currentModelPath: String? {
        modelPath(for: selectedModelId)
    }

    var downloadedModelIds: [String] {
        downloadedModels.compactMap { (id, path) in
            FileManager.default.fileExists(atPath: path) ? id : nil
        }
    }

    func modelPath(for modelId: String) -> String? {
        guard let path = downloadedModels[modelId],
              FileManager.default.fileExists(atPath: path) else { return nil }
        return path
    }

    static let downloadBase: URL = {
        FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
            .appendingPathComponent("Fennec/WhisperModels")
    }()

    init() {
        self.selectedModelId = UserDefaults.standard.string(forKey: "whisperModelId")
            ?? WhisperModelInfo.all[1].id
        self.downloadedModels = (UserDefaults.standard.dictionary(forKey: "whisperDownloadedModels") as? [String: String]) ?? [:]
        self.isEnabled = UserDefaults.standard.bool(forKey: "whisperEnabled")
        refreshModelReady()
    }

    private func refreshModelReady() {
        isModelReady = currentModelPath != nil
    }

    @MainActor
    func downloadModel() async {
        isDownloading = true
        downloadProgress = 0
        downloadError = nil

        do {
            try FileManager.default.createDirectory(at: Self.downloadBase, withIntermediateDirectories: true)

            let modelFolder = try await WhisperKit.download(
                variant: selectedModelId,
                downloadBase: Self.downloadBase
            ) { [weak self] progress in
                Task { @MainActor in
                    self?.downloadProgress = progress.fractionCompleted
                }
            }

            downloadedModels[selectedModelId] = modelFolder.path
            refreshModelReady()
            isDownloading = false
        } catch {
            downloadError = error.localizedDescription
            isDownloading = false
        }
    }

    @MainActor
    func enableAndDownload() async {
        isEnabled = true
        downloadError = nil
        if !isModelReady {
            await downloadModel()
            if !isModelReady {
                isEnabled = false
            }
        }
    }

    func disableAndDeleteAllModels() {
        for (id, path) in downloadedModels {
            try? FileManager.default.removeItem(atPath: path)
            downloadedModels.removeValue(forKey: id)
        }
        isEnabled = false
        refreshModelReady()
    }

    func deleteModel() {
        guard let path = downloadedModels[selectedModelId] else { return }
        try? FileManager.default.removeItem(atPath: path)
        downloadedModels.removeValue(forKey: selectedModelId)
        refreshModelReady()
    }
}
