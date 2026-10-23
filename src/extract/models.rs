//! MobileSAM model files: where they live, verification, and download.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;

use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModelFile {
    /// Local file name.
    pub name: &'static str,
    /// Path under the set's base URL.
    pub remote: &'static str,
    pub sha256: &'static str,
    pub size: u64,
}

/// A downloadable model: its files and where they come from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModelSet {
    pub name: &'static str,
    pub base_url: &'static str,
    pub files: &'static [ModelFile],
}

impl ModelSet {
    /// Total download size in bytes.
    pub fn total_size(&self) -> u64 {
        self.files.iter().map(|f| f.size).sum()
    }
}

/// The MobileSAM ONNX export (MIT) from Hugging Face `Acly/MobileSAM`.
pub const FILES: [ModelFile; 2] = [
    ModelFile {
        name: "mobile_sam_image_encoder.onnx",
        remote: "mobile_sam_image_encoder.onnx",
        sha256: "580f5fb648ea1062c0aabc26217aed56921985f03f0cbbd852bba81d760cc749",
        size: 28_157_093,
    },
    ModelFile {
        name: "sam_mask_decoder_single.onnx",
        remote: "sam_mask_decoder_single.onnx",
        sha256: "93915fc7c993ab9d59ab8c9ccd3bce37f7509c81ab4150a74abd4d2abbd8570d",
        size: 16_501_323,
    },
];

pub const BASE_URL: &str = "https://huggingface.co/Acly/MobileSAM/resolve/main/";

pub const MOBILE_SAM: ModelSet = ModelSet {
    name: "MobileSAM",
    base_url: BASE_URL,
    files: &FILES,
};

/// OpenCLIP ViT-B/32 trained on LAION-2B (MIT), int8 ONNX export from
/// Hugging Face `onnx-community/CLIP-ViT-B-32-laion2B-s34B-b79K-ONNX`.
pub const CLIP: ModelSet = ModelSet {
    name: "OpenCLIP",
    base_url:
        "https://huggingface.co/onnx-community/CLIP-ViT-B-32-laion2B-s34B-b79K-ONNX/resolve/main/",
    files: &[
        ModelFile {
            name: "model_int8.onnx",
            remote: "onnx/model_int8.onnx",
            sha256: "2ac8410fe198fbd003cd97460bdd68653d99c33f53311881cd306190067aee9b",
            size: 152_708_043,
        },
        ModelFile {
            name: "tokenizer.json",
            remote: "tokenizer.json",
            sha256: "6d9109cc838977f3ca94a379eec36aecc7c807e1785cd729660ca2fc0171fb35",
            size: 3_642_073,
        },
    ],
};

/// Total MobileSAM download size in bytes.
pub fn total_size() -> u64 {
    MOBILE_SAM.total_size()
}

/// `<app data>/models` (`GAC_MODEL_DIR` overrides).
pub fn default_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("GAC_MODEL_DIR") {
        return PathBuf::from(dir);
    }
    dirs::data_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("Game Asset Creator")
        .join("models")
}

/// Whether every MobileSAM file is present with the right size.
pub fn present(dir: &Path) -> bool {
    present_set(&MOBILE_SAM, dir)
}

/// Whether every file of `set` is present in `dir` with the right size.
pub fn present_set(set: &ModelSet, dir: &Path) -> bool {
    set.files.iter().all(|f| {
        std::fs::metadata(dir.join(f.name))
            .map(|m| m.len() == f.size)
            .unwrap_or(false)
    })
}

pub fn sha256_file(path: &Path) -> std::io::Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

/// Checks every MobileSAM file's SHA-256.
pub fn verify(dir: &Path) -> Result<(), String> {
    verify_set(&MOBILE_SAM, dir)
}

/// Checks every file of `set` in `dir` against its SHA-256.
pub fn verify_set(set: &ModelSet, dir: &Path) -> Result<(), String> {
    for f in set.files {
        let path = dir.join(f.name);
        let hash = sha256_file(&path).map_err(|e| format!("{}: {e}", f.name))?;
        if hash != f.sha256 {
            return Err(format!("{} is damaged (checksum mismatch)", f.name));
        }
    }
    Ok(())
}

/// Where downloads come from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    /// `<base>/<file name>` over HTTPS.
    Http(String),
    /// Copy from a local folder (tests, offline installs).
    Directory(PathBuf),
}

impl Default for Source {
    fn default() -> Self {
        Source::Http(BASE_URL.to_owned())
    }
}

/// A model download running in the background.
pub struct Download {
    total: u64,
    done: Arc<AtomicU64>,
    cancel: Arc<AtomicBool>,
    result: Receiver<Result<(), String>>,
    finished: Option<Result<(), String>>,
}

impl Download {
    /// Downloads MobileSAM.
    pub fn start(source: Source, dir: PathBuf, notify: Arc<dyn Fn() + Send + Sync>) -> Self {
        Self::start_set(MOBILE_SAM, source, dir, notify)
    }

    /// Downloads the files of `set` into `dir`.
    pub fn start_set(
        set: ModelSet,
        source: Source,
        dir: PathBuf,
        notify: Arc<dyn Fn() + Send + Sync>,
    ) -> Self {
        let done = Arc::new(AtomicU64::new(0));
        let cancel = Arc::new(AtomicBool::new(false));
        let (tx, result) = mpsc::channel();
        let (d, c) = (done.clone(), cancel.clone());
        std::thread::Builder::new()
            .name("model-download".to_owned())
            .spawn(move || {
                let outcome = download_all(&set, &source, &dir, &d, &c, &*notify);
                let _ = tx.send(outcome);
                notify();
            })
            .expect("spawn download");
        Self {
            total: set.total_size(),
            done,
            cancel,
            result,
            finished: None,
        }
    }

    /// Fraction downloaded, 0…1.
    pub fn progress(&self) -> f32 {
        (self.done.load(Ordering::Relaxed) as f64 / self.total.max(1) as f64).min(1.0) as f32
    }

    /// The outcome once finished.
    pub fn result(&mut self) -> Option<&Result<(), String>> {
        if self.finished.is_none() {
            self.finished = self.result.try_recv().ok();
        }
        self.finished.as_ref()
    }
}

impl Drop for Download {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

fn download_all(
    set: &ModelSet,
    source: &Source,
    dir: &Path,
    done: &AtomicU64,
    cancel: &AtomicBool,
    notify: &dyn Fn(),
) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    for &f in set.files {
        let target = dir.join(f.name);
        if target.exists() && sha256_file(&target).ok().as_deref() == Some(f.sha256) {
            done.fetch_add(f.size, Ordering::Relaxed);
            continue;
        }
        // Unique per download, so a cancelled one cleaning up never removes
        // the partial file of a newer one.
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let part = dir.join(format!(
            "{}.{}-{}.part",
            f.name,
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let result = fetch(set, source, f, &part, done, cancel, notify).and_then(|()| {
            let hash = sha256_file(&part).map_err(|e| e.to_string())?;
            if hash == f.sha256 {
                Ok(())
            } else {
                Err(format!(
                    "{} failed verification (checksum mismatch)",
                    f.name
                ))
            }
        });
        if let Err(e) = result {
            let _ = std::fs::remove_file(&part);
            return Err(e);
        }
        std::fs::rename(&part, &target).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn fetch(
    set: &ModelSet,
    source: &Source,
    f: ModelFile,
    part: &Path,
    done: &AtomicU64,
    cancel: &AtomicBool,
    notify: &dyn Fn(),
) -> Result<(), String> {
    let mut reader: Box<dyn Read> = match source {
        Source::Http(base) => {
            // The MobileSAM source string may be overridden; other sets use
            // their own base URL.
            let base = if set.name == MOBILE_SAM.name {
                base.as_str()
            } else {
                set.base_url
            };
            let url = format!("{base}{}", f.remote);
            let response = ureq::get(&url)
                .call()
                .map_err(|e| format!("download failed: {e}"))?;
            Box::new(response.into_body().into_reader())
        }
        Source::Directory(dir) => {
            Box::new(std::fs::File::open(dir.join(f.name)).map_err(|e| format!("{}: {e}", f.name))?)
        }
    };
    let mut out = std::fs::File::create(part).map_err(|e| e.to_string())?;
    let mut buf = vec![0u8; 1 << 16];
    let mut last_notify = 0u64;
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err("download cancelled".to_owned());
        }
        let n = reader
            .read(&mut buf)
            .map_err(|e| format!("download failed: {e}"))?;
        if n == 0 {
            break;
        }
        out.write_all(&buf[..n]).map_err(|e| e.to_string())?;
        let total = done.fetch_add(n as u64, Ordering::Relaxed) + n as u64;
        if total - last_notify > 1 << 20 {
            last_notify = total;
            notify();
        }
    }
    out.flush().map_err(|e| e.to_string())
}
