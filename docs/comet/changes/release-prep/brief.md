# Outcome

Game Asset Creator can be handed to someone else: a self-contained, ad-hoc-signed macOS app (Apple Silicon) that carries its own decode-only ffmpeg and ONNX Runtime, a README and user guide that walk from a video to finished game assets, a headless smoke test that runs locally and in GitHub Actions, and a performance pass so the UI never freezes during extraction.

# Scope

## Source coverage

Source: the user's release request (queued 2026-10-21 via /comet-native), read completely.

| Unit | Content | Status | Classification | Maps to |
|---|---|---|---|---|
| S1 | "performance pass (profile and fix the slowest canvas/extraction paths)" | complete | executable | R1, R2, R7 |
| S2 | "move segmentation to a worker thread so the UI never freezes, with progress + cancel for batch extraction" | complete | executable | R1, R2, R3 |
| S3 | "PyInstaller packaging for macOS" | complete | superseded → S3' (the app is Rust; PyInstaller does not apply) | — |
| S3' | macOS .app bundle (cargo build --release + bundle script) with ffmpeg and ONNX Runtime handled | complete | executable | R4 |
| S4 | "document model weights download-on-first-use rather than bundled" | complete | executable | R5 |
| S5 | "a user guide (README + docs/ with the video→asset walkthrough)" | complete | executable | R5 |
| S6 | "a smoke-test script exercising open/edit/extract/export headlessly" | complete | executable | R6 |
| S7 | Acceptance "packaged app launches on a clean machine" | complete | executable (automated proxy + manual check) | R4 |
| S8 | Acceptance "batch extraction of a 1-minute clip keeps the UI responsive" | complete | executable | R2 |
| S9 | Acceptance "smoke test passes in CI" | complete | executable | R6 |

## Features

- Performance: Remove Background and Auto-Detect compute on worker threads (results identical to today's; stale results dropped when parameters change; Cancel never waits for them); model and PDFium downloads get a Cancel button; Extract Frames' Cancel no longer blocks the UI; a release test runs Extract Assets on a 1-minute clip and holds every UI frame under 50 ms; all existing release benchmarks still pass.
- Packaging (scripts/package-macos.sh): builds a pinned, decode-only LGPL ffmpeg (no GPL or non-free parts), fetches Microsoft's official ONNX Runtime for macOS arm64 (pinned version and SHA-256), builds the app in release against that ffmpeg, and assembles "Game Asset Creator.app" (Info.plist, icon, dylibs in Contents/Frameworks with @rpath references, ONNX Runtime found there first), ad-hoc signs it, and produces a .zip and a .dmg. A `--self-test` mode of the app (headless, no windows, never touches the user's data) decodes a bundled sample video frame and loads the bundled ONNX Runtime.
- Icon: a simple designed icon (SVG source + generated .icns), replaceable by swapping one file.
- Documentation: README.md (what it is, install, first launch past Gatekeeper, build and package, licences) and docs/user-guide.md (text-only walkthrough: video → Extract Assets → review → library → tagging → sprite sheets; plus editor, patterns, PDF parts, recolour, enhance lines), stating which models download on first use (sizes, sources, where they are stored, how to remove them); THIRD_PARTY_NOTICES.md (ffmpeg LGPL, ONNX Runtime MIT, PDFium, MobileSAM and OpenCLIP model licences).
- Smoke test: tests/smoke.rs drives the app headlessly and offline (open an image → edit with a filter → extract with Remove Background → add to the library → export PNG and save the project; open a video → Extract Assets → review → export a sprite sheet); scripts/smoke.sh runs it; .github/workflows/ci.yml (macOS arm64 runner) runs fmt, clippy, tests, and the smoke test.

# Non-goals

- Intel or universal builds; Windows/Linux packages.
- Developer ID signing, notarization, auto-update, an installer, or the App Store.
- Bundling model weights (they keep downloading on first use).
- Screenshots in the guide.

# Acceptance examples

- R1: Remove Background and Auto-Detect previews run on workers: while one computes on a 2048 × 2048 layer, every UI frame stays under 50 ms (release); the final pixels and detected rectangles equal the synchronous results; changing a parameter mid-run discards the stale result; Cancel restores the layer at once.
- R2: Extract Assets on a 1-minute clip: progress rises steadily to completion, every UI frame during the run stays under 50 ms (release), and Cancel ends the run within 1 s with the cancelling frame also under 50 ms.
- R3: Model/PDFium downloads show a Cancel button that stops the download and leaves no partial files; cancelling Extract Frames returns within one UI frame (< 50 ms).
- R4: `scripts/package-macos.sh` produces "Game Asset Creator.app", a .zip, and a .dmg. Every Mach-O file in the app references only system libraries or @rpath/@executable_path/@loader_path (no Homebrew or other absolute paths); the bundled ffmpeg reports an LGPL, decode-only configuration; `codesign --verify --deep --strict` passes; the bundled binary's `--self-test` passes (decodes the sample frame, loads the bundled ONNX Runtime) without touching the user's data. Launching the .app on another Mac is a manual check described in the README.
- R5: README.md, docs/user-guide.md, and THIRD_PARTY_NOTICES.md exist; the guide's walkthrough uses only menu items and controls that exist in the app (checked by a test that finds each named menu item/button through the headless UI); model download sizes, sources, and storage paths match the code.
- R6: tests/smoke.rs passes headlessly and offline via scripts/smoke.sh; .github/workflows/ci.yml is valid YAML on a macOS arm64 runner and runs exactly the commands that pass locally.
- R7: All existing release benchmarks and UI tests pass unchanged.

# Constraints and invariants

- The app stays MIT; the bundled ffmpeg is LGPL (dynamically linked, with its notice and source reference).
- Tests and the self-test never touch ~/Library/Application Support/Game Asset Creator.
- Network access happens only in the packaging script (pinned sources with SHA-256) and in the app's existing consented downloads.

# Decisions

- ffmpeg: minimal decode-only LGPL build, bundled.
- ONNX Runtime: bundled (official Microsoft macOS arm64 build).
- Distribution: ad-hoc signed .zip and .dmg (Gatekeeper right-click ▸ Open on first launch).
- CI: GitHub Actions workflow plus a local smoke script.
- Performance: workers for Remove Background and Auto-Detect, download Cancel, non-blocking Extract Frames cancel, 50 ms UI-frame bar during a 1-minute Extract Assets run.
- Apple Silicon only.
- Icon: a simple designed icon, replaceable.
- Text-only user guide.
- Conventions adopted without asking: version 1.0.0; bundle identifier `io.github.game-asset-creator`; minimum macOS 13; the 1-minute fixture is a generated low-resolution clip committed under tests/fixtures/video.
- Workspace: branch `comet/release-prep` off master.

# Open questions

- None. User confirmed outcome, scope, decisions, acceptance R1–R7, and non-goals on 2026-10-23.

# Verification expectations

- `cargo test`, clippy `-D warnings`, `fmt --check`, all release benchmarks and release UI runs.
- New: release responsiveness tests (R1, R2, R3), tests/smoke.rs, the guide-menu test, the packaging script run with bundle checks (otool, codesign, ffmpeg configuration, `--self-test`).
