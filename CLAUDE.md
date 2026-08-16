# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

Fennec の Tauri 実装（macOS / Windows / Linux）。元は macOS 専用の SwiftUI アプリで、コミット `11872c4` でこのリポジトリごと Tauri に置き換えた（Swift 実装はそれ以前の履歴にのみ存在する）。機能・データ形式・CLI コマンド体系は Swift 版と互換。ビルド手順は `README.md` を参照。

## Build

```bash
pnpm install
pnpm tauri dev                          # 開発（ただし録音の検証には使えない。下記参照）
pnpm tauri build --debug --bundles app  # 権限まわりの動作検証はこの .app で行う
cargo test -p fennec-core               # 純ロジックのテストはここに集約
```

- **Node 22 必須**。親ディレクトリの `.tool-versions` が Node 20 を指しており mise が非対話シェルで活性化しないことがあるため、スクリプトから叩くときは `export PATH="$HOME/.local/share/mise/installs/node/22.23.1/bin:$PATH"` を明示する
- **macOS で録音を検証するときは必ずバンドル版 `.app` を使う**。`pnpm tauri dev` は非バンドルバイナリのため TCC がダイアログを出さずサイレント拒否し、録音が「エラーなしの無音」になる（原因の切り分けに時間を溶かすポイント）
- リリース: `scripts/release.sh`。CI は `.github/workflows/build.yml`（Windows/Linux はフルビルド、macOS は core テストのみ ― Swift ヘルパーが macOS 26 SDK を要求し GitHub ランナーでビルドできないため）

## Architecture

```
crates/fennec-core   純ロジック（OS 依存なし・唯一の被依存クレート）。セグメントマージ、Whisper 品質フィルタ、
                     session.json、IPC 型、設定、議事録プロンプト。テストはここに集約する
crates/fennec-audio  キャプチャ（mac: Core Audio process tap / win: WASAPI loopback / linux: PipeWire、
                     マイクは全 OS cpal）、WAV/Opus/m4a デコード、再生エンジン
crates/fennec-stt    whisper.cpp（バッチ + ライブ VAD）、sherpa-onnx 話者分離、モデル DL、Swift ヘルパークライアント
crates/fennec-cli    `fennec` CLI。アプリと IPC で通信（Swift 版 CLI とコマンド体系互換）
src-tauri/           アプリ本体。commands（フロント向け）/ recording / transcription / services / ipc / calendar / tray
src/                 React + TypeScript + Tailwind + zustand
sidecar/FennecHelper Swift ヘルパー（macOS のみ、sidecar として同梱）
```

### 設計の要点

- **OS 差はクレート内で吸収する**。`fennec-audio::new_system_capture()` のような関数が cfg で実装を切り替え、上位（`src-tauri`）に cfg を漏らさない
- **文字起こしエンジンは 2 系統**。whisper.cpp（全 OS）と Apple 音声認識（macOS のみ、ライブ / バッチとも選択可）。設定 `liveTranscriptionEngine` / `autoTranscribeEngine` で切り替え、Apple が使えなければ whisper にフォールバックする
- **Swift 専用 API は Swift ヘルパーに隔離する**。SpeechAnalyzer / FoundationModels / Translation は Rust から呼べないため、stdin/stdout の JSON-line プロトコルで別プロセス化している（`crates/fennec-stt/src/apple.rs` がクライアント）。Windows/Linux ビルドでは `tauri.macos.conf.json` ごと読まれないので自然に外れる
- **録音は WAV で書き、文字起こし後に Opus (.ogg) へ変換する**。Rust にクロスプラットフォームな AAC エンコーダがないため。既存 `.m4a` は symphonia で読めるので再生・再文字起こしは可能
- **議事録はセッションフォルダが正**。アプリ内で編集でき、任意の場所へ「書き出す」と `minutesExportedAt/Path/Hash` を記録して差分を検出する（Swift 版にはない拡張。未知キーは無視されるので互換は保たれる）

## 既存アプリとの互換（壊さないこと）

- セッションフォルダ構成（`<base>/yyyyMMdd_HHmmss/`）、`session.json` のキー名、`transcript_<id>.txt` の `[MM:SS] ラベル: 本文` 形式
- IPC プロトコル（JSON-line、`args` は全て String、config は 8 キーのホワイトリスト）と CLI のコマンド体系・終了コード（エラー 1 / 未起動 2）
- 議事録プロンプトの文言（`fennec-core::minutes::build_prompt`）。既存の出力と揃える意図があるので安易に変えない

Swift 版から意図的に直した点: `engineType` が更新されないバグ、`[MM:SS]` が 1 時間超で 60 分以上になるバグ、翻訳結果が保存されないバグ（`transcript_<id>.json` に構造化保存するようにした）。

## 依存ライブラリの落とし穴

いずれも実際に踏んで原因特定に時間がかかったもの。触るときは戻さないこと。

- **whisper-rs 0.14 の `set_abort_callback_safe`**: `Box<dyn FnMut() -> bool>` 以外のクロージャ型を渡すと trampoline と user_data の型が食い違い UB になる（ランダムに abort して `error -6`）。必ず明示的に box して渡す
- **trash 5.x**: macOS の既定が Finder 経由（osascript）で、GUI アプリからだとオートメーション権限プロンプトでハングする。`DeleteMethod::NsFileManager` を使う
- **sherpa-rs**: default features に `tts` が含まれ不要に重い。また動的リンクだとバンドル版 `.app` が `libonnxruntime.dylib` を見つけられず起動即死するため、macOS のみ `static` を有効にしている（Windows/Linux は動的のままなのでインストーラ同梱が将来必要）
- **cidre 0.19**: `core_audio` だけでは feature gate が足りずコンパイルできない。`at` / `av` / `cm` を併せて有効にする
- **Swift ヘルパーの `batchTranscribe`**: AVAudioFile で読むため wav/m4a のみ。ogg/opus は呼び出し側で一時 wav に変換して渡している
- **`tauri build` は externalBin（release ビルドの CLI）を `target/debug/fennec` にも上書きコピーする**。そのため CLI は debug / release 両方のソケットを順に試す実装にしてある

## Conventions

- UI テキストは日本語がキー。`src/i18n/{ja,en}.json` が唯一のソース（Swift 版の `Localizable.xcstrings` から変換していた名残でかつて生成物だったが、Swift 実装が無くなったので手で編集する）。**新しい UI 文言を足したら両ファイルに同じキーを追記する**（`ja.json` は日本語をそのまま、`en.json` に英訳）
- テストは `fennec-core` に寄せる。OS やモデルに依存する部分は example（`cargo run -p fennec-stt --example diarize_test` など）で手動確認する
- リアルタイム処理のコールバック（キャプチャの `on_chunk`）はオーディオスレッドから呼ばれる。重い処理を書かず、チャネルで別スレッドに渡す
