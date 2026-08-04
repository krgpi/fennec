import AVFoundation
import Combine

final class AudioPlayer: NSObject, ObservableObject, AVAudioPlayerDelegate {
    @Published var isPlaying = false
    @Published var currentTime: TimeInterval = 0
    @Published var duration: TimeInterval = 0
    @Published var playingSessionId: String?
    @Published var systemEnabled = true
    @Published var micEnabled = true

    private var systemPlayer: AVAudioPlayer?
    private var micPlayer: AVAudioPlayer?
    private var timer: AnyCancellable?

    func play(systemURL: URL?, micURL: URL?, sessionId: String) {
        stop()
        systemEnabled = true
        micEnabled = true

        if let url = systemURL {
            systemPlayer = try? AVAudioPlayer(contentsOf: url)
            systemPlayer?.delegate = self
        }
        if let url = micURL {
            micPlayer = try? AVAudioPlayer(contentsOf: url)
            micPlayer?.delegate = self
        }

        guard systemPlayer != nil || micPlayer != nil else { return }
        playingSessionId = sessionId
        duration = max(systemPlayer?.duration ?? 0, micPlayer?.duration ?? 0)

        systemPlayer?.play()
        micPlayer?.play()
        isPlaying = true
        startTimer()
    }

    func togglePlayPause() {
        if isPlaying {
            systemPlayer?.pause()
            micPlayer?.pause()
            isPlaying = false
            timer?.cancel()
        } else {
            if systemEnabled { systemPlayer?.play() }
            if micEnabled { micPlayer?.play() }
            isPlaying = true
            startTimer()
        }
    }

    func stop() {
        systemPlayer?.stop()
        micPlayer?.stop()
        systemPlayer = nil
        micPlayer = nil
        timer?.cancel()
        isPlaying = false
        currentTime = 0
        duration = 0
        playingSessionId = nil
    }

    func seek(to time: TimeInterval) {
        systemPlayer?.currentTime = time
        micPlayer?.currentTime = time
        currentTime = time
    }

    func setSystemEnabled(_ enabled: Bool) {
        systemEnabled = enabled
        if enabled {
            if isPlaying {
                systemPlayer?.currentTime = currentTime
                systemPlayer?.play()
            }
        } else {
            systemPlayer?.pause()
        }
    }

    func setMicEnabled(_ enabled: Bool) {
        micEnabled = enabled
        if enabled {
            if isPlaying {
                micPlayer?.currentTime = currentTime
                micPlayer?.play()
            }
        } else {
            micPlayer?.pause()
        }
    }

    func audioPlayerDidFinishPlaying(_ player: AVAudioPlayer, successfully flag: Bool) {
        let systemDone = systemPlayer == nil || !systemPlayer!.isPlaying
        let micDone = micPlayer == nil || !micPlayer!.isPlaying
        if systemDone && micDone {
            stop()
        }
    }

    private func startTimer() {
        timer = Timer.publish(every: 0.1, on: .main, in: .common)
            .autoconnect()
            .sink { [weak self] _ in
                guard let self else { return }
                let t = self.systemPlayer?.currentTime ?? self.micPlayer?.currentTime ?? 0
                self.currentTime = t
            }
    }
}
