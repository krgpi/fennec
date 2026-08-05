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
- **メニューバー対応** — メニューバーで動作（Dockアイコンの表示/非表示切替可能）
- **プライバシー最優先** — 音声、文字起こし、翻訳データはすべてMacから外に出ない

## インストール

### Homebrew（推奨）

```bash
brew tap krgpi/tap
brew install fennec
```

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
