<p align="right">
  <a href="README.md">English</a> | <strong>日本語</strong>
</p>

<img src="icon_1024.png" width="128" height="128" alt="Fennec" align="left">

# Fennec

macOS / Windows / Linux 向けのローカル文字起こし & AI議事録アプリ。

システム音声とマイク入力を同時にキャプチャし、リアルタイムおよび録音後の文字起こしを行います。文字起こし結果をコーディングエージェント（Claude Code / Codex / Gemini CLI）に渡せば、過去の会議のコンテキストを踏まえた議事録を生成できます。音声処理はすべてデバイス上で完結し、録音や文字起こしが外部に送信されることはありません。

<br clear="left">

## 思想

- **オンデバイス優先** — 録音・文字起こし・話者分離・翻訳はすべてローカルで実行する。外部に出るのは議事録生成だけで、それも普段使っているエージェントCLIに渡すだけ
- **DBではなくファイル** — 1セッション＝1フォルダ（`<base>/yyyyMMdd_HHmmss/`）。音声・文字起こし・`minutes.md` がそのまま置かれ、アプリなしでも読めて grep できる
- **通話アプリを選ばない** — OSレベルでシステム音声を録るため、Zoom / Meet / Teams / Discord / LINE / FaceTime のいずれでも動く
- **スクリプトから叩ける** — `fennec` CLI とフックで録音を制御でき、自前の自動化に組み込める

## 特徴

- **デュアルストリームキャプチャ** — システム音声（相手の声）とマイク（自分の声）を同時に録音
- **リアルタイム文字起こし** — whisper.cpp（全OS）または Apple音声認識（macOS 26+）によるライブ文字起こし
- **高精度文字起こし** — 録音後に Whisper Small / Large V3 (Turbo) で文字起こし
- **AI議事録** — Claude Code / Codex / Gemini CLI で、過去の会議コンテキストを踏まえた議事録を生成
- **ライブ翻訳** — 外国語の発話をオンデバイスで翻訳（macOS）
- **話者分離** — 誰が何を言ったかを識別（sherpa-onnx）
- **カレンダー連携** — ビデオ会議を検出して録音開始をリマインド
- **トレイ / メニューバー** — バックグラウンド常駐
- **CLI** — ターミナルやスクリプトからアプリを操作

## インストール

### macOS — Homebrew

```bash
brew tap krgpi/tap
brew install --cask fennec
```

### Windows — winget

```powershell
winget install krgpi.Fennec
```

### Linux

```bash
# AppImage（全ディストロ対応・依存同梱・FUSE 2 が必要）
curl -LO https://github.com/krgpi/fennec/releases/latest/download/Fennec_0.0.1_amd64.AppImage
chmod +x Fennec_0.0.1_amd64.AppImage && ./Fennec_0.0.1_amd64.AppImage

# Debian / Ubuntu
curl -LO https://github.com/krgpi/fennec/releases/latest/download/Fennec_0.0.1_amd64.deb
sudo dpkg -i Fennec_0.0.1_amd64.deb && sudo apt-get install -f

# Arch Linux
yay -S fennec-bin
```

x86_64 のみ。システム音声のキャプチャに PipeWire が必要です。

`.deb` / `.rpm` / `.exe` / `.dmg` は各 [GitHub Release](https://github.com/krgpi/fennec/releases) にも添付されています。

## 使い方

1. Fennec を起動し、マイクとシステム音声の録音権限を許可する
2. 設定画面から Whisper モデルをダウンロードする（まずは Small、精度重視なら Large V3 Turbo）
3. **録音**を開始する。話すそばからライブ文字起こしが流れ、トレイに入れてもバックグラウンドで録音が続く
4. 録音を停止すると全体の文字起こしが自動生成され、音声と同じフォルダに保存される
5. **議事録**タブでプリセットとバックエンド（Claude Code / Codex / Gemini CLI）を選んで生成する。アプリ内で編集し、**書き出し**で任意の場所へ出力できる

セッションは録音フォルダ配下の1フォルダにまとまります。

```
20260813_140000/
  system_<id>.ogg / mic_<id>.ogg   音声
  transcript_<id>.txt       [MM:SS] 話者: 本文
  minutes.md                議事録の正本
  session.json              メタデータ
```

### CLI

`fennec` はアプリバンドルに同梱されています（macOS: `Fennec.app/Contents/MacOS/fennec`）。

```bash
fennec status --launch
fennec record start / stop
fennec sessions list
fennec transcribe latest --engine apple
fennec minutes latest --backend claude
```

## 開発

要件: Rust (stable) / Node.js 22+ / pnpm。macOS は Xcode Command Line Tools（Swiftヘルパー用）、Linux は `libpipewire-0.3-dev` `libasound2-dev` ほか Tauri 標準の依存が必要です。

```bash
pnpm install
pnpm tauri dev                          # 開発起動
pnpm tauri build --debug --bundles app  # デバッグ.app（macOSの権限検証はこちら）
pnpm tauri build                        # リリースビルド
cargo test -p fennec-core               # ユニットテスト
```

> **macOSの注意**: `pnpm tauri dev` は非バンドルバイナリのため、TCC（マイク・システム音声収録）がダイアログを出さずサイレント拒否し、録音が無音になります。録音の検証は必ずバンドル版 `.app` で行ってください。

### 構成

```
crates/fennec-core   純ロジック（セグメントマージ・幻覚フィルタ・session.json・IPC型・設定）。テストはここに集中
crates/fennec-audio  キャプチャ（mac: Core Audio process tap / win: WASAPI loopback / linux: PipeWire、マイクは cpal）、デコード、再生
crates/fennec-stt    whisper.cpp（バッチ+ライブVAD）、sherpa-onnx 話者分離、モデルDL、Swiftヘルパークライアント
crates/fennec-cli    `fennec` CLI（アプリとIPCで通信）
src-tauri/           アプリ本体（録音・文字起こしジョブ・議事録・フック・IPCサーバー・トレイ）
src/                 React + TypeScript + Tailwind + zustand
sidecar/FennecHelper Swiftヘルパー（SpeechAnalyzer / FoundationModels / Translation、macOSのみ）
```

OS差はクレート内で吸収し、`src-tauri` に `cfg` を漏らさない方針です。録音は WAV で書き、文字起こし後に Opus (`.ogg`) へ変換します。UI文言は日本語がキーで、`scripts/convert-xcstrings.mjs` が `src/i18n/{ja,en}.json` を生成します。

### リリース

GitHub Releases が配布物の正本で、そこから各パッケージマネージャに流します。

1. `src-tauri/tauri.conf.json` / `package.json` / ルート `Cargo.toml` の `version` を上げ、`vX.Y.Z` タグを push して draft release を作る
2. macOS / Windows / Linux それぞれのマシンで `scripts/release.sh` を実行し、同じ draft に成果物をアップロードする（タグは `tauri.conf.json` から導出。`--no-upload` を渡すとビルドのみ）（Swiftヘルパーが macOS 26 SDK を要求しCIランナーで作れないため、ビルドはローカルに揃えている）
3. draft を publish する
4. `.github/workflows/publish-packages.yml` を該当タグで手動実行し、winget と AUR を更新する

未対応: Windowsのコード署名、macOSの署名/notarization、Linux aarch64。

## ライセンス

MIT
