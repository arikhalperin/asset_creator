//! MobileSAM through ONNX Runtime, loaded dynamically on first use.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use image::{GrayImage, Luma, RgbaImage};
use ndarray::{Array, Array1, Array2, Array3, Array4};
use ort::session::Session;
use ort::value::Tensor;

use super::models;
use super::segment::Prompt;

/// Longest side of the image the encoder sees.
pub const ENCODER_SIDE: u32 = 1024;

/// Where ONNX Runtime is looked for, in order.
pub fn default_runtime_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(p) = std::env::var_os("ORT_DYLIB_PATH") {
        paths.push(PathBuf::from(p));
    }
    #[cfg(target_os = "macos")]
    {
        paths.push("/opt/homebrew/lib/libonnxruntime.dylib".into());
        paths.push("/usr/local/lib/libonnxruntime.dylib".into());
        paths.push("libonnxruntime.dylib".into());
    }
    #[cfg(target_os = "linux")]
    {
        paths.push("/usr/local/lib/libonnxruntime.so".into());
        paths.push("libonnxruntime.so".into());
    }
    #[cfg(target_os = "windows")]
    paths.push("onnxruntime.dll".into());
    paths
}

static RUNTIME: OnceLock<Result<ort::environment::Environment, String>> = OnceLock::new();

/// Loads ONNX Runtime once per process from the first path that works.
pub fn runtime(paths: &[PathBuf]) -> Result<&'static ort::environment::Environment, String> {
    RUNTIME
        .get_or_init(|| {
            let mut last = "ONNX Runtime not found".to_owned();
            for path in paths {
                // Bare names are resolved by the system loader.
                if path.is_absolute() && !path.exists() {
                    continue;
                }
                match ort::init_from(path) {
                    Ok(builder) => match builder.with_name("game-asset-creator").build() {
                        Ok(env) => return Ok(env),
                        Err(e) => last = format!("ONNX Runtime failed to start: {e}"),
                    },
                    Err(e) => last = format!("ONNX Runtime could not be loaded: {e}"),
                }
            }
            Err(last)
        })
        .as_ref()
        .map_err(Clone::clone)
}

/// Whether ONNX Runtime has been loaded in this process.
pub fn runtime_loaded() -> bool {
    RUNTIME.get().is_some_and(|r| r.is_ok())
}

pub struct SamModel {
    encoder: Session,
    decoder: Session,
}

/// An encoded image.
pub struct Embedding {
    data: Array4<f32>,
    scale: f32,
    width: u32,
    height: u32,
}

impl SamModel {
    /// Loads the (verified) models from `dir` with the loaded runtime.
    pub fn load(env: &ort::environment::Environment, dir: &Path) -> Result<Self, String> {
        models::verify(dir)?;
        let session = |name: &str| -> Result<Session, String> {
            Session::builder(env)
                .and_then(|mut b| b.commit_from_file(dir.join(name)))
                .map_err(|e| format!("cannot load {name}: {e}"))
        };
        Ok(Self {
            encoder: session(models::FILES[0].name)?,
            decoder: session(models::FILES[1].name)?,
        })
    }

    pub fn embed(&mut self, image: &RgbaImage) -> Result<Embedding, String> {
        let (w, h) = image.dimensions();
        let scale = ENCODER_SIDE as f32 / w.max(h) as f32;
        let (rw, rh) = (
            ((w as f32 * scale).round() as u32).max(1),
            ((h as f32 * scale).round() as u32).max(1),
        );
        let resized = image::imageops::resize(image, rw, rh, image::imageops::FilterType::Triangle);
        let input: Array3<f32> =
            Array::from_shape_fn((rh as usize, rw as usize, 3), |(y, x, c)| {
                resized.get_pixel(x as u32, y as u32)[c] as f32
            });
        let tensor = Tensor::from_array(input).map_err(|e| e.to_string())?;
        let outputs = self
            .encoder
            .run(ort::inputs!["input_image" => tensor])
            .map_err(|e| format!("encoder failed: {e}"))?;
        let data = outputs["image_embeddings"]
            .try_extract_array::<f32>()
            .map_err(|e| e.to_string())?
            .to_owned()
            .into_dimensionality::<ndarray::Ix4>()
            .map_err(|e| e.to_string())?;
        Ok(Embedding {
            data,
            scale,
            width: w,
            height: h,
        })
    }

    /// The object mask (255 = object) for `prompt` at full resolution.
    pub fn decode(&mut self, embedding: &Embedding, prompt: &Prompt) -> Result<GrayImage, String> {
        let s = embedding.scale;
        let mut coords = Vec::new();
        let mut labels = Vec::new();
        for &(x, y, include) in &prompt.points {
            coords.extend([x * s, y * s]);
            labels.push(if include { 1.0 } else { 0.0 });
        }
        match prompt.rect {
            Some((a, b)) => {
                coords.extend([
                    a.0.min(b.0) * s,
                    a.1.min(b.1) * s,
                    a.0.max(b.0) * s,
                    a.1.max(b.1) * s,
                ]);
                labels.extend([2.0, 3.0]);
            }
            None => {
                coords.extend([0.0, 0.0]);
                labels.push(-1.0);
            }
        }
        let n = labels.len();
        let coords = Array::from_shape_vec((1, n, 2), coords).map_err(|e| e.to_string())?;
        let labels: Array2<f32> =
            Array::from_shape_vec((1, n), labels).map_err(|e| e.to_string())?;
        let t = |e: ort::Error| e.to_string();
        let outputs = self
            .decoder
            .run(ort::inputs![
                "image_embeddings" => Tensor::from_array(embedding.data.clone()).map_err(t)?,
                "point_coords" => Tensor::from_array(coords).map_err(t)?,
                "point_labels" => Tensor::from_array(labels).map_err(t)?,
                "mask_input" => Tensor::from_array(Array4::<f32>::zeros((1, 1, 256, 256))).map_err(t)?,
                "has_mask_input" => Tensor::from_array(Array1::<f32>::zeros(1)).map_err(t)?,
                "orig_im_size" => Tensor::from_array(Array1::from(vec![embedding.height as f32, embedding.width as f32])).map_err(t)?,
            ])
            .map_err(|e| format!("decoder failed: {e}"))?;
        let masks = outputs["masks"]
            .try_extract_array::<f32>()
            .map_err(|e| e.to_string())?;
        let (w, h) = (embedding.width, embedding.height);
        let mask = GrayImage::from_fn(w, h, |x, y| {
            Luma([if masks[[0, 0, y as usize, x as usize]] > 0.0 {
                255
            } else {
                0
            }])
        });
        Ok(super::clean_mask(&mask, &prompt.seeds(w, h), 0.005))
    }
}
