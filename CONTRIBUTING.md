# Contributing to Fennec

日本語版は [CONTRIBUTING.ja.md](CONTRIBUTING.ja.md) にあります。

Thanks for your interest in Fennec. Bug reports, feature requests, and pull requests are all welcome.

## Contributor License Agreement

**Before your first pull request can be merged, you need to sign the [Contributor License Agreement](CLA.md).**

You don't need to do anything in advance. When you open your first PR, a bot will comment with instructions — signing takes one comment and is remembered for all of your future contributions.

### Why

Fennec's core is MIT-licensed and will stay that way. The CLA assigns copyright in contributions to the maintainer so that the project keeps unified ownership of the codebase, which is what makes it possible to offer commercial editions later, adjust the license of future versions, and enforce the project's rights. If copyright were split across dozens of contributors, none of that would be possible without tracking every one of them down for consent.

Section 5 of the CLA grants you back unrestricted rights to your own contribution — you can reuse it anywhere, including commercially. Authorship credit stays in the git history.

If you'd rather not sign, that's fine — please still open an issue describing the problem or the idea. Detailed bug reports are genuinely useful on their own.

## Reporting bugs

Open an issue and include:

- Your OS and version, and the Fennec version (shown in Settings)
- Which transcription engine you were using (Apple / Whisper) and which model
- What you expected and what happened instead
- Relevant log output

Fennec processes audio locally and logs can contain fragments of what you recorded — please review before pasting.

## Pull requests

Set up the development environment first — see [Development](README.md#development) in the README.

Before opening a PR:

```bash
cargo fmt --all
cargo clippy --workspace --all-targets
cargo test -p fennec-core
pnpm build
```

Some things worth knowing:

- Keep OS-specific code inside the crates. `src-tauri` should stay free of `cfg` — see [Layout](README.md#layout).
- UI strings are keyed in Japanese and live in `src/i18n/{ja,en}.json`. Add new keys to **both** files.
- On macOS, always verify recording changes with a bundled `.app` (`pnpm tauri build --debug --bundles app`). `pnpm tauri dev` runs unbundled, so TCC silently denies audio capture and recordings come out silent.
- Keep the diff focused. Unrelated reformatting makes review harder.

For anything large, please open an issue first so we can agree on the approach before you spend time on it.

## Privacy

Fennec's central promise is that audio never leaves the user's machine unless they explicitly ask it to. Any change that adds a network call, telemetry, or crash reporting needs to be discussed in an issue first, no matter how small.
