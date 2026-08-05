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
- **Privacy first** — Audio, transcripts, and translations never leave your Mac

## Requirements

- macOS 26.0+
- Xcode 16.0+
- [XcodeGen](https://github.com/yonaskolb/XcodeGen)

## Build

```bash
xcodegen generate
xcodebuild -project Fennec.xcodeproj -scheme Fennec build \
  DEVELOPMENT_TEAM=YOUR_TEAM_ID -allowProvisioningUpdates
```

## License

MIT
