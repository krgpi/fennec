<p align="right">
  <strong>English</strong> | <a href="README.ja.md">日本語</a>
</p>

<img src="icon_1024.png" width="128" height="128" alt="Fennec" align="left">

# Fennec

Local transcription & AI meeting notes for macOS.

Captures system audio and microphone input simultaneously, with real-time and post-recording speech-to-text transcription. Feed the transcript to your coding agent (Claude Code, Codex, Gemini CLI, etc.) for context-aware meeting minutes. All processing runs on-device — no data is sent to any server.

<br clear="left">

## Features

- **Any call app** — Records system audio via ScreenCaptureKit, so it works with Discord, LINE, Zoom, Google Meet, Teams, FaceTime, and any other app
- **Dual-stream capture** — System audio (their voice) + microphone (your voice) recorded together
- **Real-time transcription** — Live transcription using Apple Speech Recognition (macOS 26+)
- **High-accuracy transcription** — Post-recording transcription with WhisperKit (Small / Large V3 models)
- **AI meeting minutes** — Generate minutes from transcripts using your preferred coding agent, with context from previous meetings
- **Live translation** — Foreign-language speech translated in real-time, all on-device
- **Speaker diarization** — Identify who said what via WhisperKit SpeakerKit
- **Calendar integration** — Auto-detect video meetings and remind you to start recording
- **Menu bar support** — Runs in the menu bar with optional Dock icon
- **CLI & automation** — Control everything from the `fennec` command, and run shell commands on events like recording stop
- **Privacy first** — Audio, transcripts, and translations never leave your Mac

## Install

### Homebrew (recommended)

```bash
brew tap krgpi/tap
brew install fennec
```

## CLI

> [!WARNING]
> The CLI is in alpha. Commands and output formats may change without notice.

The app bundles a `fennec` command (installed to your PATH via Homebrew; otherwise available at `Fennec.app/Contents/MacOS/fennec`). It talks to the running app over a local socket.

```bash
fennec status --launch        # App status (launches the app if needed)
fennec record start           # Start recording
fennec record stop            # Stop recording
fennec sessions list          # List recording sessions
fennec transcribe latest      # Transcribe a session (--engine apple|whisper)
fennec minutes latest --preset work   # Generate minutes with a preset
fennec preset list            # Manage minutes presets (list/show/create/delete)
fennec config list            # Read & write app settings (list/get/set)
fennec model list             # Manage Whisper models (list/download)
fennec hook list              # Manage automation hooks (list/add/enable/disable/delete)
```

Session IDs accept `latest`. Add `--json` to list/show commands for machine-readable output.

## Automation

Run shell commands when events fire: `recordingStarted`, `recordingStopped`, `transcriptionCompleted`, `minutesGenerated`. Configure them in Settings > Automation, or via the CLI:

```bash
fennec hook add recordingStopped 'cp "$FENNEC_SESSION_DIR"/*.m4a ~/Backup/'
```

Hooks receive context via environment variables — `FENNEC_EVENT`, `FENNEC_SESSION_ID`, `FENNEC_SESSION_DIR`, `FENNEC_TRANSCRIPT_FILE`, `FENNEC_MINUTES_FILE` — and are logged to `~/Library/Logs/Fennec/hooks.log`.

## Build from source

### Requirements

- macOS 26.0+
- Xcode 16.0+
- [XcodeGen](https://github.com/yonaskolb/XcodeGen)

### Build

```bash
xcodegen generate
xcodebuild -project Fennec.xcodeproj -scheme Fennec build \
  DEVELOPMENT_TEAM=YOUR_TEAM_ID -allowProvisioningUpdates
```

## License

MIT
