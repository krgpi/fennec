import Foundation

enum FennecIPC {
    static var socketPath: String {
        let dir = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
            .appendingPathComponent("Fennec")
        #if DEBUG
        return dir.appendingPathComponent("fennec.debug.sock").path
        #else
        return dir.appendingPathComponent("fennec.sock").path
        #endif
    }

    static var appName: String {
        #if DEBUG
        "Fennec Dev.app"
        #else
        "Fennec.app"
        #endif
    }

    static var appBundleID: String {
        #if DEBUG
        "io.github.krgpi.Fennec.debug"
        #else
        "io.github.krgpi.Fennec"
        #endif
    }

    static func encodeLine(_ object: [String: Any]) -> Data? {
        guard var data = try? JSONSerialization.data(withJSONObject: object) else { return nil }
        data.append(0x0A)
        return data
    }
}
