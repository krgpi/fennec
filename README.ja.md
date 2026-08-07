<p align="right">
  <a href="README.md">English</a> | <strong>日本語</strong>
</p>

<img src="icon_1024.png" width="128" height="128" alt="Fennec" align="left">

# Fennec

macOS向けのローカル文字起こし & AI議事録アプリ。

システム音声とマイク入力を同時にキャプチャし、リアルタイムおよび録音後の音声テキスト変換を行います。文字起こし結果をコーディングエージェント（Claude Code、Codex、Gemini CLIなど）に渡して、文脈を踏まえた議事録を作成できます。すべての処理はデバイス上で実行され、データは外部サーバーに送信されません。

<br clear="left">

## 特徴

- **あらゆる通話アプリに対応** — ScreenCaptureKit経由でシステム音声を録音するため、Discord、LINE、Zoom、Google Meet、Teams、FaceTimeなど、あらゆるアプリで動作
- **デュアルストリームキャプチャ** — システム音声（相手の声）とマイク（自分の声）を同時に録音
- **リアルタイム文字起こし** — Apple音声認識によるライブ文字起こし（macOS 26+）
- **高精度文字起こし** — WhisperKit（Small / Large V3モデル）による録音後の文字起こし
- **AI議事録** — コーディングエージェントを使い、過去の会議コンテキストを踏まえた議事録を生成
- **ライブ翻訳** — 外国語の発話をリアルタイムで翻訳（すべてオンデバイス）
- **話者分離** — WhisperKit SpeakerKitにより誰が何を言ったかを識別
- **カレンダー連携** — ビデオ会議を自動検出し、録音開始をリマインド
- **CLI & オートメーション** — `fennec` コマンドで全機能を操作でき、録音停止などのイベントでシェルコマンドを実行可能
- **メニューバー対応** — メニューバーで動作（Dockアイコンの表示/非表示切替可能）
- **プライバシー最優先** — 音声、文字起こし、翻訳データはすべてMacから外に出ない

## インストール

### Homebrew（推奨）

```bash
brew tap krgpi/tap
brew install fennec
```

## CLI

> [!WARNING]
> CLIはアルファ版です。コマンドや出力形式は予告なく変更される可能性があります。

アプリには `fennec` コマンドが同梱されています（Homebrew経由ならPATHに追加されます。それ以外は `Fennec.app/Contents/MacOS/fennec`）。起動中のアプリとローカルソケットで通信します。

```bash
fennec status --launch        # アプリの状態（未起動なら起動）
fennec record start           # 録音開始
fennec record stop            # 録音停止
fennec sessions list          # 録音セッション一覧
fennec transcribe latest      # 文字起こし（--engine apple|whisper）
fennec minutes latest                 # プリセットなしで議事録生成（録音フォルダに保存）
fennec minutes latest --preset work   # プリセットを使って議事録生成
fennec preset list            # 議事録プリセット管理（list/show/create/delete）
fennec config list            # 設定の読み書き（list/get/set）
fennec model list             # Whisperモデル管理（list/download）
fennec hook list              # オートメーションフック管理（list/add/enable/disable/delete）
```

セッションIDには `latest` が使えます。一覧・詳細系コマンドは `--json` でJSON出力できます。

## オートメーション

イベント発生時にシェルコマンドを実行できます: `recordingStarted`、`recordingStopped`、`transcriptionCompleted`、`minutesGenerated`。設定 > オートメーション、またはCLIから登録します。

```bash
fennec hook add recordingStopped 'cp "$FENNEC_SESSION_DIR"/*.m4a ~/Backup/'
```

フックには環境変数でコンテキストが渡されます — `FENNEC_EVENT`、`FENNEC_SESSION_ID`、`FENNEC_SESSION_DIR`、`FENNEC_TRANSCRIPT_FILE`、`FENNEC_MINUTES_FILE`。実行結果は `~/Library/Logs/Fennec/hooks.log` に記録されます。

## ソースからビルド

### 要件

- macOS 26.0+
- Xcode 16.0+
- [XcodeGen](https://github.com/yonaskolb/XcodeGen)

### ビルド

```bash
xcodegen generate
xcodebuild -project Fennec.xcodeproj -scheme Fennec build \
  DEVELOPMENT_TEAM=YOUR_TEAM_ID -allowProvisioningUpdates
```

## ライセンス

MIT
