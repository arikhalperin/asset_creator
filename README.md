# Game Asset Creator

A desktop image editor for making and extracting 2D game assets on macOS. Pull sprites out of videos, sprite sheets, and scanned paper-model sheets; clean them up in a layered editor; keep them in a tagged library; and export single PNGs, sprite sheets with a JSON atlas, or animation strips.

- **Extract** — Remove Background, Auto-Detect Objects, and the Extract Object tool (click or drag a box; MobileSAM when installed, GrabCut otherwise).
- **Video → assets** — open a clip, run Extract Assets…, review the clusters (single sprites and animations), and add them to the library.
- **Editor** — layers with masks and blend modes, selections, brushes, filters (including Enhance Lines), Free Transform, Recolor Paint.
- **Library** — thumbnails, search, categories, automatic tagging (OpenCLIP), versions, and export to PNGs, packed sprite sheets (grid or max-rects) with an atlas, or animation strips.
- **Patterns** — find a repeating tile and make it seamless.
- **Paper models** — lay out the parts of PDF (or image) sheets onto A4 pages, recolour the paint, and enhance faded lines.

The [user guide](docs/user-guide.md) walks through all of it, starting with turning a video into game assets.

## Install

Requires an Apple Silicon Mac with macOS 13 or later.

1. Download `Game-Asset-Creator-1.0.0-arm64.dmg` (or the `.zip`) and drag **Game Asset Creator** into Applications.
2. The app is signed ad hoc (not notarized), so the first launch needs one extra step: in Finder, right-click (or Control-click) the app ▸ **Open**, then confirm **Open**. Later launches work normally.

### Models download on first use

Nothing is bundled that the app can fetch when needed. Each download asks first, shows progress, can be cancelled, and is checked against a fixed SHA-256:

| Used for | Download | Size | Stored in |
|---|---|---|---|
| Extract Object, Protect (MobileSAM) | [Acly/MobileSAM](https://huggingface.co/Acly/MobileSAM) image encoder + mask decoder (ONNX) | 44.7 MB | `~/Library/Application Support/Game Asset Creator/models` |
| Auto-Tag (OpenCLIP ViT-B/32, LAION-2B) | [onnx-community/CLIP-ViT-B-32-laion2B-s34B-b79K-ONNX](https://huggingface.co/onnx-community/CLIP-ViT-B-32-laion2B-s34B-b79K-ONNX) int8 model + tokenizer | 156.4 MB | `…/Game Asset Creator/models/clip` |
| Opening PDFs (PDFium) | [bblanchon/pdfium-binaries](https://github.com/bblanchon/pdfium-binaries) chromium/7881 | 3.5 MB | `…/Game Asset Creator/pdfium` |

Without MobileSAM the extract tools use GrabCut; without the tagging model new assets are tagged "other". To free the space, quit the app and delete those folders. The asset library itself lives in `~/Library/Application Support/Game Asset Creator/library`.

The app bundles ONNX Runtime (to run the models) and a decode-only ffmpeg (to read video). Microsoft's ONNX Runtime build sends usage telemetry by default; the app always switches it off.

## Build from source

You need Rust 1.95 or later and, from Homebrew, `ffmpeg`, `onnxruntime`, and `pkg-config`:

```sh
brew install ffmpeg onnxruntime pkg-config
cargo run --release
```

`ORT_DYLIB_PATH` points the app at a specific `libonnxruntime.dylib`. `game-asset-creator --self-test [video]` checks video decoding and ONNX Runtime without opening a window.

### Package the app

```sh
scripts/package-macos.sh            # add --skip-ffmpeg-build to reuse the cached ffmpeg
```

The script builds a decode-only LGPL ffmpeg (7.1.5, pinned and checksummed), downloads Microsoft's ONNX Runtime 1.31.0 for macOS arm64 (pinned), builds the app in release, assembles `target/package/Game Asset Creator.app` with its libraries in `Contents/Frameworks`, signs it ad hoc, and writes a `.zip` and a `.dmg` next to it. It then verifies the bundle and fails on any problem: every library reference stays inside the app or the system, ffmpeg reports an LGPL decode-only configuration, `codesign --verify --deep --strict` passes, and the bundled app's `--self-test` passes.

To be sure the app runs on a clean Mac, copy the `.dmg` to a Mac without Homebrew and open it as described in Install.

The icon is `assets/icon/icon.svg`; `scripts/make-icon.sh` regenerates `assets/icon/AppIcon.icns` from it.

## Tests

```sh
cargo test                       # unit, fixture, and headless UI tests
scripts/smoke.sh                 # headless, offline: open → edit → extract → export, video → sprite sheet
cargo test --release --test responsive -- --include-ignored   # UI stays responsive during extraction
```

Some tests use the downloaded models or PDFium when they are present (`GAC_MODEL_DIR`, `GAC_CLIP_DIR`, `GAC_PDFIUM_DIR`) and skip those parts otherwise. Benchmarks are `#[ignore]` tests run in release (`cargo test --release --test <name> -- --ignored`).

GitHub Actions (`.github/workflows/ci.yml`) runs format, clippy, the tests, and the smoke test on an Apple Silicon runner for every push and pull request.

## License

MIT. Bundled and downloaded components keep their own licences; see [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).
