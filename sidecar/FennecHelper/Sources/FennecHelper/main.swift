import Foundation

let server = HelperServer()

Thread.detachNewThread {
    while let line = readLine(strippingNewline: true) {
        guard !line.isEmpty else { continue }
        server.handle(line: line)
    }
    server.drainAndExit()
}

dispatchMain()
