import Foundation

struct MinutesPreset: Identifiable, Codable, Hashable {
    let id: UUID
    var name: String
    var contextFolderBookmark: Data
    var outputFolderBookmark: Data
    var backend: MinutesBackend
    var model: String
    var createdAt: Date
    var lastUsedAt: Date

    init(id: UUID = UUID(), name: String, contextFolderBookmark: Data, outputFolderBookmark: Data, backend: MinutesBackend = .claude, model: String? = nil) {
        self.id = id
        self.name = name
        self.contextFolderBookmark = contextFolderBookmark
        self.outputFolderBookmark = outputFolderBookmark
        self.backend = backend
        self.model = model ?? backend.defaultModel
        let now = Date()
        self.createdAt = now
        self.lastUsedAt = now
    }

    func resolveContextFolder() -> URL? {
        Self.resolveBookmark(contextFolderBookmark)
    }

    func resolveOutputFolder() -> URL? {
        Self.resolveBookmark(outputFolderBookmark)
    }

    private static func resolveBookmark(_ data: Data) -> URL? {
        var isStale = false
        guard let url = try? URL(
            resolvingBookmarkData: data,
            options: .withSecurityScope,
            relativeTo: nil,
            bookmarkDataIsStale: &isStale
        ) else { return nil }
        guard url.startAccessingSecurityScopedResource() else { return nil }
        return url
    }

    static func createBookmark(for url: URL) -> Data? {
        try? url.bookmarkData(
            options: .withSecurityScope,
            includingResourceValuesForKeys: nil,
            relativeTo: nil
        )
    }
}

final class MinutesPresetStore: ObservableObject {
    static let shared = MinutesPresetStore()

    @Published var presets: [MinutesPreset] = []

    private static let key = "minutesPresets"

    init() {
        load()
    }

    func load() {
        guard let data = UserDefaults.standard.data(forKey: Self.key) else { return }
        presets = (try? JSONDecoder().decode([MinutesPreset].self, from: data)) ?? []
        presets.sort { $0.lastUsedAt > $1.lastUsedAt }
    }

    func save() {
        guard let data = try? JSONEncoder().encode(presets) else { return }
        UserDefaults.standard.set(data, forKey: Self.key)
    }

    func addOrUpdate(_ preset: MinutesPreset) {
        if let idx = presets.firstIndex(where: { $0.id == preset.id }) {
            presets[idx] = preset
        } else {
            presets.append(preset)
        }
        presets.sort { $0.lastUsedAt > $1.lastUsedAt }
        save()
    }

    func rename(_ preset: MinutesPreset, to name: String) {
        let trimmed = name.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty, let idx = presets.firstIndex(where: { $0.id == preset.id }) else { return }
        presets[idx].name = trimmed
        save()
    }

    func delete(_ preset: MinutesPreset) {
        presets.removeAll { $0.id == preset.id }
        save()
    }
}
