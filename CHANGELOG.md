# 変更履歴

このプロジェクトの主な変更点を記録します。

書式は [Keep a Changelog](https://keepachangelog.com/) に、バージョニングは
[Semantic Versioning](https://semver.org/) に従います。

## [Unreleased]

### 追加

- 録音一覧の右クリックメニューに「音声を書き出す」。PC音声とマイクを1本のモノラル音声へ
  結合し、Opus (.ogg) または WAV で任意の場所に保存する

### 修正

- Swift ヘルパー（fennec-helper）が長時間の文字起こしで 1GB 級のメモリを消費する問題。
  SpeechAnalyzer への音声供給を上限付きバッファ + 背圧に変更し、SFSpeechRecognizer
  では部分結果ごとに全文を組み立てないようにした

## [0.0.2] - 2026-08-19

### 追加

- 議事録生成のカスタムプロンプト。議事録ダイアログで指示を自由に入力でき、バックエンドや
  モデルと同様にプリセットへ保存される。CLI からも指定可能
  （`fennec minutes --prompt` / `fennec preset create --prompt`）

### 変更

- `src/i18n/{ja,en}.json` を直接編集する方式に変更し、`Localizable.xcstrings` の
  変換スクリプトを削除
- リリースを CI ではなく `scripts/release.sh` で実行するように変更。タグは
  `tauri.conf.json` から導出し、OS ごとのビルドを並行させるためドラフトリリースを先に作成

### 修正

- CMake キャッシュが古いまま残ることによる whisper-rs-sys のビルド失敗

### 削除

- Linux の AppImage バンドルターゲット

## [0.0.1] - 2026-08-13

最初のリリース。

### 追加

- Tauri（Rust + React + TypeScript）によるクロスプラットフォーム対応
  （macOS / Windows / Linux）
- OS ごとのシステム音声キャプチャ（Core Audio process tap / WASAPI loopback /
  PipeWire）と、cpal によるマイクキャプチャ
- 全 OS で whisper.cpp による文字起こし（バッチ + ライブ VAD）。macOS では Swift
  サイドカー経由で Apple 音声認識も選択可能
- sherpa-onnx による話者分離
- Claude Code / Codex / Gemini CLI を使った議事録生成。プリセット、アプリ内編集、
  書き出しの追跡（`minutesExportedAt` / `Path` / `Hash`）に対応
- IPC ソケット経由の `fennec` CLI と自動化フック
- カレンダー連携と録音リマインダー
- Windows / Linux 向けパッケージング（MSI・NSIS、deb・rpm）

[Unreleased]: https://github.com/krgpi/fennec/compare/v0.0.2...HEAD
[0.0.2]: https://github.com/krgpi/fennec/compare/v0.0.1...v0.0.2
[0.0.1]: https://github.com/krgpi/fennec/releases/tag/v0.0.1
