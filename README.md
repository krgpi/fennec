<p align="right">
  <strong>English</strong> | <a href="README.ja.md">日本語</a>
</p>

<img src="icon_1024.png" width="128" height="128" alt="Fennec" align="left">

# Fennec

Local transcription & AI meeting notes for macOS, Windows, and Linux.

Captures system audio and microphone input simultaneously, with real-time and post-recording speech-to-text. Feed the transcript to your coding agent (Claude Code, Codex, Gemini CLI) to generate minutes with the context of past meetings. All audio processing runs on-device — recordings and transcripts never leave your machine.

<br clear="left">

## Philosophy

- **On-device first** — Recording, transcription, diarization, and translation all run locally. Only minutes generation calls out, and only to the agent CLI you already trust.
- **Files, not a database** — One folder per session (`<base>/yyyyMMdd_HHmmss/`) holding the audio, transcripts, and `minutes.md`. Everything is readable and greppable without the app.
- **Works with any call app** — Captures system audio at the OS level, so Zoom, Meet, Teams, Discord, LINE, and FaceTime all just work.
- **Scriptable** — The `fennec` CLI and hooks let you drive recording and wire it into your own automation.

## Features

- **Dual-stream capture** — System audio (their voice) + microphone (your voice), recorded together. No virtual audio device (BlackHole, VB-CABLE) to install or route through
- **Real-time transcription** — Live transcription with whisper.cpp (all platforms) or Apple Speech Recognition (macOS 26+)
- **High-accuracy transcription** — Post-recording transcription with Whisper Small / Large V3 (Turbo) models
- **AI meeting minutes** — Generate minutes via Claude Code / Codex / Gemini CLI, with context from previous meetings
- **Live translation** — On-device translation of foreign-language speech (macOS)
- **Speaker diarization** — Identify who said what (sherpa-onnx)
- **Calendar integration** — Detect video meetings and remind you to start recording
- **Tray / menu bar** — Runs in the background with a tray icon
- **CLI** — Control the app from your terminal or scripts

## Install

Every build is attached to a [GitHub Release](https://github.com/krgpi/fennec/releases). The current version is `0.0.1`; replace the version in the commands below when a newer one is out.

Nothing is code-signed yet, so every platform shows an "unidentified developer" style warning on first launch.

### macOS — Homebrew

Apple Silicon and macOS 26 (Tahoe) or later.

```bash
brew tap krgpi/tap
brew install --cask fennec
```

Or download `Fennec_0.0.1_arm64.dmg` from the release page.

### Windows

Download and run `Fennec_0.0.1_x64-setup.exe` (or `Fennec_0.0.1_x64_en-US.msi`) from the [release page](https://github.com/krgpi/fennec/releases/tag/v0.0.1). x64 only.

### Linux

```bash
# Debian / Ubuntu
curl -LO https://github.com/krgpi/fennec/releases/download/v0.0.1/Fennec_0.0.1_amd64.deb
sudo dpkg -i Fennec_0.0.1_amd64.deb && sudo apt-get install -f

# Fedora / RHEL
curl -LO https://github.com/krgpi/fennec/releases/download/v0.0.1/Fennec-0.0.1-1.x86_64.rpm
sudo dnf install ./Fennec-0.0.1-1.x86_64.rpm
```

x86_64 only. PipeWire is required for system audio capture.

## Usage

1. Launch Fennec and grant microphone and system-audio recording permission.
2. Download a Whisper model from Settings (Small is a good default; Large V3 Turbo for accuracy).
3. Press **Record**. Live transcription appears as you speak; recording continues in the background from the tray.
4. Stop recording — the full transcript is generated automatically and saved next to the audio.
5. Open **Minutes**, pick a preset and backend (Claude Code / Codex / Gemini CLI), and generate. Edit in-app, then **Export** to write the minutes anywhere you like.

Each session is a folder under your recordings directory:

```
20260813_140000/
  system_<id>.ogg / mic_<id>.ogg   audio
  transcript_<id>.txt       [MM:SS] Speaker: text
  minutes.md                the canonical minutes
  session.json              metadata
```

### CLI

`fennec` ships inside the app bundle (macOS: `Fennec.app/Contents/MacOS/fennec`).

```bash
fennec status --launch
fennec record start / stop
fennec sessions list
fennec transcribe latest --engine apple
fennec minutes latest --backend claude
```

## Development

Requirements: Rust (stable), Node.js 22+, pnpm. macOS also needs Xcode Command Line Tools (for the Swift helper); Linux needs `libpipewire-0.3-dev`, `libasound2-dev`, and the standard Tauri dependencies.

```bash
pnpm install
pnpm tauri dev                          # dev server
pnpm tauri build --debug --bundles app  # debug .app (use this to verify permissions on macOS)
pnpm tauri build                        # release build
cargo test -p fennec-core               # unit tests
```

> **macOS note**: `pnpm tauri dev` runs an unbundled binary, so TCC silently denies microphone / system audio capture without showing a prompt and recordings come out silent. Always test recording with the bundled `.app`.

### Layout

```
crates/fennec-core   Pure logic (segment merging, hallucination filter, session.json, IPC types, settings). Tests live here
crates/fennec-audio  Capture (mac: Core Audio process tap / win: WASAPI loopback / linux: PipeWire; mic via cpal), decoding, playback
crates/fennec-stt    whisper.cpp (batch + live VAD), sherpa-onnx diarization, model downloads, Swift helper client
crates/fennec-cli    `fennec` CLI, talks to the app over IPC
src-tauri/           App core: recording, transcription jobs, minutes, hooks, IPC server, tray
src/                 React + TypeScript + Tailwind + zustand
sidecar/FennecHelper Swift helper (SpeechAnalyzer / FoundationModels / Translation; macOS only)
```

Platform differences are absorbed inside the crates — `src-tauri` stays free of `cfg`. Recording is written as WAV and converted to Opus (`.ogg`) after transcription. UI strings are keyed in Japanese; run `scripts/convert-xcstrings.mjs` to regenerate `src/i18n/{ja,en}.json`.

### Release

GitHub Releases is the source of truth; package managers pull from it.

1. Bump `version` in `src-tauri/tauri.conf.json`, `package.json`, and the root `Cargo.toml`, push a `vX.Y.Z` tag, and create a draft release.
2. Run `scripts/release.sh` on a macOS, Windows, and Linux machine, uploading each build to the same draft. (The tag is derived from `tauri.conf.json`; pass `--no-upload` to build without publishing.) (Builds are local — the Swift helper needs the macOS 26 SDK, which CI runners don't have.)
3. Publish the draft.
4. Run `.github/workflows/publish-packages.yml` manually against the tag to update winget and AUR. (Both need their first version registered by hand — until then this workflow fails.)

Not yet supported: Windows code signing, macOS signing / notarization, macOS x86_64, Linux aarch64, AppImage, winget / AUR packages.

## License

MIT
