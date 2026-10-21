//! PDFium: the downloadable build (pdfium-binaries, SHA-256 pinned), its
//! installation, and the process-wide binding.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use pdfium_render::prelude::Pdfium;

use crate::extract::models::{sha256_file, ModelFile, ModelSet};

const BASE_URL: &str =
    "https://github.com/bblanchon/pdfium-binaries/releases/download/chromium/7881/";

#[cfg(target_arch = "aarch64")]
const ARCHIVE: ModelFile = ModelFile {
    name: "pdfium-mac-arm64.tgz",
    remote: "pdfium-mac-arm64.tgz",
    sha256: "52e94ca5aa8847934330daf3f8150c190682c5ca93831468794f8b90d4392e40",
    size: 3_533_019,
};

#[cfg(not(target_arch = "aarch64"))]
const ARCHIVE: ModelFile = ModelFile {
    name: "pdfium-mac-x64.tgz",
    remote: "pdfium-mac-x64.tgz",
    sha256: "6dedf83990e0e3d6b7c93c9e7589c5a126b0ae14b7464d76120cff7a26afb18b",
    size: 3_588_813,
};

/// PDFium chromium/7881 for this Mac (BSD-3-Clause / Apache-2.0).
pub const PDFIUM: ModelSet = ModelSet {
    name: "PDFium",
    base_url: BASE_URL,
    files: &[ARCHIVE],
};

/// `<app data>/Game Asset Creator/pdfium` (`GAC_PDFIUM_DIR` overrides).
pub fn default_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("GAC_PDFIUM_DIR") {
        return PathBuf::from(dir);
    }
    dirs::data_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("Game Asset Creator")
        .join("pdfium")
}

/// The library file inside `dir`.
pub fn library_path(dir: &Path) -> PathBuf {
    Pdfium::pdfium_platform_library_name_at_path(dir)
}

/// Whether PDFium is installed in `dir`.
pub fn installed(dir: &Path) -> bool {
    library_path(dir).is_file()
}

/// Installs the library from the downloaded archive in `dir` (verified
/// again), then removes the archive.
pub fn install_from_archive(dir: &Path) -> Result<(), String> {
    let archive = dir.join(ARCHIVE.name);
    let hash = sha256_file(&archive).map_err(|e| format!("{}: {e}", ARCHIVE.name))?;
    if hash != ARCHIVE.sha256 {
        let _ = std::fs::remove_file(&archive);
        return Err(format!(
            "{} failed verification (checksum mismatch)",
            ARCHIVE.name
        ));
    }
    let file = std::fs::File::open(&archive).map_err(|e| e.to_string())?;
    let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(file));
    let target = library_path(dir);
    let name = target
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let part = dir.join(format!("{name}.part"));
    let mut found = false;
    for entry in tar.entries().map_err(|e| e.to_string())? {
        let mut entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path().map_err(|e| e.to_string())?.into_owned();
        if path == Path::new("lib").join(&name) {
            entry.unpack(&part).map_err(|e| e.to_string())?;
            found = true;
            break;
        }
    }
    if !found {
        return Err(format!("{} does not contain lib/{name}", ARCHIVE.name));
    }
    std::fs::rename(&part, &target).map_err(|e| e.to_string())?;
    let _ = std::fs::remove_file(&archive);
    Ok(())
}

static PDFIUM_INSTANCE: OnceLock<Result<Pdfium, String>> = OnceLock::new();

/// The process-wide PDFium (bound to the library in `dir` on first use; a
/// process can bind only once).
pub fn pdfium(dir: &Path) -> Result<&'static Pdfium, String> {
    PDFIUM_INSTANCE
        .get_or_init(|| {
            let path = library_path(dir);
            if !path.is_file() {
                return Err("PDFium is not installed".to_owned());
            }
            Pdfium::bind_to_library(&path)
                .map(Pdfium::new)
                .map_err(|e| format!("PDFium could not be loaded: {e}"))
        })
        .as_ref()
        .map_err(Clone::clone)
}
