import AppKit
import Foundation

enum StorageLocation {
    private static let bookmarkKey = "saveDirectoryBookmark"
    private static var accessedURL: URL?

    static var baseDirectory: URL {
        if let url = accessedURL {
            return url
        }
        if let url = resolveBookmark() {
            accessedURL = url
            return url
        }
        return defaultDirectory
    }

    private static var defaultDirectory: URL {
        FileManager.default.urls(for: .documentDirectory, in: .userDomainMask)[0]
            .appendingPathComponent("Fennec")
    }

    static var hasCustomDirectory: Bool {
        UserDefaults.standard.data(forKey: bookmarkKey) != nil
    }

    static var displayPath: String {
        let url = baseDirectory
        return url.path.replacingOccurrences(of: NSHomeDirectory(), with: "~")
    }

    @discardableResult
    static func chooseDirectory() -> URL? {
        let panel = NSOpenPanel()
        panel.canChooseFiles = false
        panel.canChooseDirectories = true
        panel.canCreateDirectories = true
        panel.allowsMultipleSelection = false
        panel.message = String(localized: "録音ファイルの保存先フォルダを選択してください")
        panel.prompt = String(localized: "選択")

        guard panel.runModal() == .OK, let url = panel.url else { return nil }

        accessedURL?.stopAccessingSecurityScopedResource()
        accessedURL = nil

        saveBookmark(for: url)

        if let resolved = resolveBookmark() {
            accessedURL = resolved
            return resolved
        }
        return url
    }

    private static func saveBookmark(for url: URL) {
        guard let data = try? url.bookmarkData(
            options: .withSecurityScope,
            includingResourceValuesForKeys: nil,
            relativeTo: nil
        ) else { return }
        UserDefaults.standard.set(data, forKey: bookmarkKey)
    }

    private static func resolveBookmark() -> URL? {
        guard let data = UserDefaults.standard.data(forKey: bookmarkKey) else { return nil }
        var isStale = false
        guard let url = try? URL(
            resolvingBookmarkData: data,
            options: .withSecurityScope,
            relativeTo: nil,
            bookmarkDataIsStale: &isStale
        ) else { return nil }

        if isStale {
            saveBookmark(for: url)
        }

        guard url.startAccessingSecurityScopedResource() else { return nil }
        return url
    }
}
