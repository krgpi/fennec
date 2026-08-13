# Fennec (Tauri)

会議の録音・文字起こし・議事録生成アプリのクロスプラットフォーム実装（macOS / Windows / Linux）。
既存のmacOS版（`../code`）と同じセッションフォルダ形式・CLIコマンド体系・IPCプロトコルを持つ。

## Build

要件: Rust (stable) / Node.js 22+ / pnpm。macOSはXcode Command Line Tools（Swiftヘルパー用）、Linuxは `libpipewire-0.3-dev` `libasound2-dev` ほかTauri標準の依存。

```bash
pnpm install
pnpm tauri dev                        # 開発起動（TCCの動作確認は下記の.appで）
pnpm tauri build --debug --bundles app  # デバッグ.app（macOSの権限検証はこちらを使う）
pnpm tauri build                      # リリースビルド
```

> **macOSの注意**: `pnpm tauri dev` は非バンドルバイナリのため、TCC（マイク・システム音声収録）がダイアログを出さずにサイレント拒否し録音が無音になる。録音を試すときは必ず `--bundles app` で作った `target/debug/bundle/macos/Fennec.app` を起動すること。

## 構成

```
crates/
  fennec-core    純ロジック（セグメントマージ・幻覚フィルタ・session.json・IPC型・設定）。ユニットテストはここに集中
  fennec-audio   キャプチャ（mac: Core Audio process tap / win: WASAPI loopback / linux: PipeWire、マイクはcpal）、WAV/Opus/m4aデコード、再生エンジン
  fennec-stt     whisper.cpp（バッチ+ライブVAD）、sherpa-onnx話者分離、モデルDL、macOSヘルパークライアント
  fennec-cli     `fennec` CLI（アプリとIPCで通信、既存Swift版CLI互換）
src-tauri/       アプリ本体（録音コントローラ、文字起こしジョブ、議事録生成、フック、IPCサーバー、トレイ）
src/             React + TypeScript + Tailwind + zustand
sidecar/FennecHelper/  Swiftヘルパー（SpeechAnalyzer・FoundationModels・Apple Translation。macOSのみ、sidecarとして同梱）
```

- 文字起こしエンジン: whisper.cpp（全OS）+ Apple音声認識（macOSのみ、ライブ/バッチとも選択可）
- 録音は WAV で書き、文字起こし後に Opus (.ogg) へ変換。既存 .m4a セッションも読める
- 議事録はセッションフォルダの `minutes.md` が正本。プリセットの出力先などへは明示的な「書き出し」で別ファイルとして書き出す。`session.json` には書き出し日時・パス・内容ハッシュだけを残し、書き出し先ファイルのリネームや移動は追跡しない（ハッシュ不一致＝未書き出しの編集ありとしてUIに表示する）
- i18n: `scripts/convert-xcstrings.mjs` が既存 `Localizable.xcstrings` から `src/i18n/{ja,en}.json` を生成

## CLI

アプリバンドルに `fennec` が同梱される（`Fennec.app/Contents/MacOS/fennec`）。

```bash
fennec status --launch
fennec record start / stop
fennec sessions list
fennec transcribe latest --engine apple
fennec minutes latest --backend claude
```

## リリース・配布

GitHub Releases が配布物の正本で、そこから各パッケージマネージャに流す。

| OS | チャネル |
|---|---|
| macOS | Homebrew cask（`.dmg` / `.zip`） |
| Windows | winget（`krgpi.Fennec`）+ NSIS `.exe` 直配布 |
| Linux | AUR（`fennec-bin`）+ `.deb` / `.rpm` / `.AppImage` 直配布 |

手順:

1. `src-tauri/tauri.conf.json` / `package.json` / ルート `Cargo.toml` の `version` を上げて `vX.Y.Z` タグを push し、draft release を作る
2. macOS / Windows / Linux それぞれのローカルマシンで `scripts/release.sh vX.Y.Z`
   → 各OSの成果物を同じ draft release にアップロードする（CIではビルドしない。macOSはSwiftヘルパーが macOS 26 SDK 必須でランナーでは作れず、Windows / Linux もローカルビルドに揃えている）
3. draft を publish
4. `vX.Y.Z` タグを選んで `.github/workflows/publish-packages.yml` を手動実行し、winget と AUR を更新する

必要なシークレット: `WINGET_TOKEN`（classic PAT / `public_repo` スコープ）、`AUR_USERNAME` / `AUR_EMAIL` / `AUR_SSH_PRIVATE_KEY`。

初回だけ手動が必要:

- winget — `wingetcreate new` で `krgpi.Fennec` の初版マニフェストをPR（winget-releaser は既存パッケージの更新専用）
- AUR — `fennec-bin` を一度手で `git push` して作成（PKGBUILDのテンプレートは `packaging/aur/PKGBUILD.in`）
- Homebrew — cask を tap に登録

未対応: Windowsのコード署名（無署名のためSmartScreen警告が出る）、macOSの署名/notarization、Linux aarch64。

## Install (Linux)

### AppImage（推奨・全ディストロ対応）

```bash
curl -LO https://github.com/krgpi/fennec/releases/latest/download/Fennec_0.0.1_amd64.AppImage
chmod +x Fennec_0.0.1_amd64.AppImage
./Fennec_0.0.1_amd64.AppImage
```

依存ライブラリ（sherpa-onnx、GTK等）を同梱しているため追加インストール不要。FUSE 2が必要（`sudo apt install libfuse2` / 多くのディストロにはプリインストール済み）。

### Debian / Ubuntu (.deb)

```bash
curl -LO https://github.com/krgpi/fennec/releases/latest/download/Fennec_0.0.1_amd64.deb
sudo dpkg -i Fennec_0.0.1_amd64.deb
sudo apt-get install -f  # 不足する依存パッケージがあれば解決
```

### AUR (Arch Linux)

```bash
# yay / paru 等の AUR ヘルパーを使用
yay -S fennec-bin
```

> **注意**: Linux版は x86_64 のみ。PipeWire が必要（システム音声のキャプチャに使用）。

## 既知の落とし穴（開発メモ）

- whisper-rs 0.14 の `set_abort_callback_safe` は `Box<dyn FnMut() -> bool>` 以外のクロージャ型を渡すとUB（ランダムabort / error -6）。必ず明示的にboxして渡す
- trash 5.x はmacOSデフォルトがFinder経由(osascript)でオートメーション権限プロンプトによりハングする。`DeleteMethod::NsFileManager` を使う
- cidre 0.19 は `core_audio` featureだけではコンパイルできず `at`/`av`/`cm` が必要
- Swiftヘルパーの `batchTranscribe` はAVAudioFile読み（wav/m4a）。ogg/opusは呼び出し側で一時wavに変換して渡す
- sherpa-rs はmacOSでは `static` でビルド（onnxruntimeを埋め込み、dll同梱を不要にする）。Linux/Windowsでは動的リンク（`libsherpa-onnx-c-api.so` / `.dll` をインストーラに同梱する必要がある）。AppImageはlinuxdeployが自動でバンドルするが、`.deb` はビルド後にsoファイルを手動で追加してリパッケージする
