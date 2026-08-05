# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/),
and this project adheres to [Semantic Versioning](https://semver.org/).

## [Unreleased]

## [1.0.0] - 2026-08-05

### Added

- Dual-stream audio capture (system audio + microphone) via ScreenCaptureKit
- Real-time transcription using Apple Speech Recognition (macOS 26+)
- Post-recording transcription with WhisperKit (Small / Large V3 models)
- Speaker diarization via WhisperKit SpeakerKit
- Live on-device translation
- Calendar integration with recording reminders
- Menu bar support with optional Dock icon
- Homebrew Cask distribution (`brew install krgpi/tap/fennec`)

[Unreleased]: https://github.com/krgpi/fennec/compare/v1.0.0...HEAD
[1.0.0]: https://github.com/krgpi/fennec/releases/tag/v1.0.0
