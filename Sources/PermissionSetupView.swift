import AVFoundation
import Speech
import SwiftUI

struct PermissionSetupView: View {
    var onComplete: () -> Void

    @State private var micStatus = AVCaptureDevice.authorizationStatus(for: .audio)
    @State private var speechStatus = SFSpeechRecognizer.authorizationStatus()
    @State private var screenCaptureGranted = CGPreflightScreenCaptureAccess()

    private var allGranted: Bool {
        micStatus == .authorized && speechStatus == .authorized && screenCaptureGranted
    }

    var body: some View {
        VStack(spacing: 28) {
            VStack(spacing: 8) {
                Image(systemName: "lock.shield")
                    .font(.system(size: 48))
                    .foregroundStyle(.blue)
                Text("文字起こし")
                    .font(.largeTitle.bold())
                Text("アプリを使用するには以下の権限が必要です")
                    .foregroundStyle(.secondary)
            }

            VStack(spacing: 12) {
                permissionRow(
                    icon: "mic.fill",
                    title: "permission.microphone",
                    description: "音声を録音するために使用します",
                    granted: micStatus == .authorized,
                    needsSettings: micStatus == .denied || micStatus == .restricted
                ) {
                    if micStatus == .notDetermined {
                        Task {
                            await AVCaptureDevice.requestAccess(for: .audio)
                            micStatus = AVCaptureDevice.authorizationStatus(for: .audio)
                        }
                    } else {
                        openPrivacySettings()
                    }
                }

                permissionRow(
                    icon: "waveform",
                    title: "音声認識",
                    description: "リアルタイムで文字起こしするために使用します",
                    granted: speechStatus == .authorized,
                    needsSettings: speechStatus == .denied || speechStatus == .restricted
                ) {
                    if speechStatus == .notDetermined {
                        SFSpeechRecognizer.requestAuthorization { status in
                            DispatchQueue.main.async { speechStatus = status }
                        }
                    } else {
                        openPrivacySettings()
                    }
                }

                permissionRow(
                    icon: "rectangle.on.rectangle",
                    title: "画面収録",
                    description: "システム音声をキャプチャするために使用します",
                    granted: screenCaptureGranted,
                    needsSettings: true
                ) {
                    _ = CGRequestScreenCaptureAccess()
                }
            }

            Button {
                onComplete()
            } label: {
                Text("はじめる")
                    .frame(width: 200, height: 40)
            }
            .buttonStyle(.borderedProminent)
            .controlSize(.large)
            .disabled(!allGranted)
        }
        .padding(40)
        .frame(minWidth: 450, minHeight: 450)
        .onReceive(NotificationCenter.default.publisher(for: NSApplication.didBecomeActiveNotification)) { _ in
            refreshStatuses()
        }
    }

    private func refreshStatuses() {
        micStatus = AVCaptureDevice.authorizationStatus(for: .audio)
        speechStatus = SFSpeechRecognizer.authorizationStatus()
        screenCaptureGranted = CGPreflightScreenCaptureAccess()
    }

    private func openPrivacySettings() {
        if let url = URL(string: "x-apple.systempreferences:com.apple.preference.security?Privacy") {
            NSWorkspace.shared.open(url)
        }
    }

    private func permissionRow(
        icon: String,
        title: LocalizedStringKey,
        description: LocalizedStringKey,
        granted: Bool,
        needsSettings: Bool,
        action: @escaping () -> Void
    ) -> some View {
        HStack(spacing: 16) {
            Image(systemName: icon)
                .font(.title2)
                .foregroundStyle(granted ? .green : .secondary)
                .frame(width: 32)

            VStack(alignment: .leading, spacing: 2) {
                Text(title)
                    .font(.headline)
                Text(description)
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }

            Spacer()

            if granted {
                Image(systemName: "checkmark.circle.fill")
                    .foregroundStyle(.green)
                    .font(.title2)
            } else {
                Button(needsSettings ? "設定を開く" : "次へ") {
                    action()
                }
                .buttonStyle(.borderedProminent)
                .controlSize(.small)
            }
        }
        .padding(12)
        .background(.quaternary.opacity(0.5), in: RoundedRectangle(cornerRadius: 10))
    }
}
