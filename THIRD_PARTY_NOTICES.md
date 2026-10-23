# Third-party notices

Game Asset Creator is MIT-licensed. The packaged app includes, or downloads on first use, the components below under their own licences.

## Bundled in the app

### FFmpeg 7.1.5 — LGPL 2.1 or later

The app links FFmpeg's libraries dynamically (`libavcodec`, `libavformat`, `libavutil`, `libavfilter`, `libavdevice`, `libswscale`, `libswresample` in `Contents/Frameworks`). They were built from the unmodified official source, <https://ffmpeg.org/releases/ffmpeg-7.1.5.tar.xz> (SHA-256 `de668509caf9e35e3cd162473441fdb29538c6d96ed080292b3cf9e6fc5d558f`), as a decode-only, LGPL build:

```
--disable-gpl --disable-nonfree --disable-version3 --enable-shared --disable-static --enable-pic
--disable-programs --disable-doc --disable-encoders --disable-muxers --disable-outdevs
--disable-indevs --disable-network --disable-protocols --enable-protocol=file,pipe
--disable-autodetect --enable-zlib --enable-bzlib --enable-videotoolbox
```

`scripts/package-macos.sh` reproduces the build. You may replace these libraries with your own build of the same FFmpeg version. The licence text is in `Contents/Resources/FFMPEG-COPYING.LGPLv2.1.txt` and at <https://www.gnu.org/licenses/old-licenses/lgpl-2.1.html>.

### ONNX Runtime 1.31.0 — MIT

Microsoft's official macOS arm64 release (`onnxruntime-osx-arm64-1.31.0.tgz`, SHA-256 `1031b36dd3bdaa8d976a6130d9cf2a05e9be320b93ae804f8427b91dc3432afc`), Copyright (c) Microsoft Corporation. The licence text is in `Contents/Resources/ONNXRUNTIME-LICENSE.txt` and at <https://github.com/microsoft/onnxruntime/blob/main/LICENSE>. This build collects telemetry by default; the app disables it (`ORT_DISABLE_TELEMETRY=1`) before loading it.

## Downloaded on first use

| Component | Licence | Source |
|---|---|---|
| PDFium (chromium/7881) | BSD-3-Clause and Apache-2.0 | <https://github.com/bblanchon/pdfium-binaries> |
| MobileSAM (ONNX export) | Apache-2.0 | <https://github.com/ChaoningZhang/MobileSAM>, ONNX files from <https://huggingface.co/Acly/MobileSAM> |
| OpenCLIP ViT-B/32 trained on LAION-2B (ONNX export) | MIT | <https://github.com/mlfoundations/open_clip>, ONNX files from <https://huggingface.co/onnx-community/CLIP-ViT-B-32-laion2B-s34B-b79K-ONNX> |

## Rust crates

The app is built from Rust crates published on crates.io under permissive licences (MIT, Apache-2.0, BSD, Zlib, and similar), including egui/eframe (MIT OR Apache-2.0), egui_dock (MIT), image (MIT OR Apache-2.0), ffmpeg-next (WTFPL), ort (MIT OR Apache-2.0), pdfium-render (MIT OR Apache-2.0), and ureq (MIT OR Apache-2.0). `cargo tree` lists them all; each crate's licence is in its published package.
