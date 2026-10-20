//! Zero-shot auto-tagging with OpenCLIP (ViT-B/32, LAION-2B, int8 ONNX)
//! through the dynamically loaded ONNX Runtime, on a background worker.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;

use image::RgbaImage;
use ndarray::{Array2, Array4};
use ort::session::Session;
use ort::value::Tensor;

use crate::extract::models::{self, Source};
use crate::extract::sam;
use crate::extract::segment::Availability;

/// CLIP's learned similarity scale.
const LOGIT_SCALE: f32 = 100.0;
const SIDE: u32 = 224;

/// Prompt phrases per built-in category ("pixel art of …"); other
/// categories use "a <name>".
pub fn phrases(category: &str) -> Vec<String> {
    let list: &[&str] = match category {
        "chalice" => &[
            "a golden chalice",
            "a goblet",
            "a holy grail cup",
            "an ornate drinking cup",
        ],
        "skeleton" => &[
            "a skeleton",
            "an undead skeleton warrior",
            "a skull and bones",
        ],
        "potion" => &[
            "a potion bottle",
            "a glass flask with liquid",
            "a magic elixir vial",
        ],
        "deathtrap" => &[
            "a spike trap",
            "a deadly trap",
            "sharp floor spikes",
            "a saw blade trap",
        ],
        "wall" => &["a stone brick wall", "a wall tile", "a dungeon wall"],
        "prop" => &[
            "a wooden crate",
            "a barrel",
            "a treasure chest",
            "a wooden table",
            "furniture",
        ],
        "other" => &[
            "a person",
            "a knight",
            "a wizard",
            "a sword",
            "an axe",
            "a monster",
            "a ghost",
            "a spider",
        ],
        name => return vec![format!("pixel art of a {name}")],
    };
    list.iter().map(|p| format!("pixel art of {p}")).collect()
}

/// Composites over neutral gray, pads to a square, resizes to 224 (bicubic)
/// and normalizes with CLIP's mean and deviation.
pub fn preprocess(image: &RgbaImage) -> Array4<f32> {
    let (w, h) = image.dimensions();
    let side = w.max(h) + 8;
    let mut square = RgbaImage::from_pixel(side, side, image::Rgba([128, 128, 128, 255]));
    for (x, y, p) in image.enumerate_pixels() {
        let a = p[3] as f32 / 255.0;
        let q = square.get_pixel_mut(x + (side - w) / 2, y + (side - h) / 2);
        for c in 0..3 {
            q[c] = (p[c] as f32 * a + 128.0 * (1.0 - a)).round() as u8;
        }
    }
    let r = image::imageops::resize(&square, SIDE, SIDE, image::imageops::FilterType::CatmullRom);
    let mean = [0.481_454_66, 0.457_827_5, 0.408_210_73];
    let std = [0.268_629_54, 0.261_302_6, 0.275_777_1];
    Array4::from_shape_fn((1, 3, SIDE as usize, SIDE as usize), |(_, c, y, x)| {
        (r.get_pixel(x as u32, y as u32)[c] as f32 / 255.0 - mean[c]) / std[c]
    })
}

fn normalize(v: &mut [f32]) {
    let n = v.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-12);
    v.iter_mut().for_each(|x| *x /= n);
}

/// Per prompt: its category's index and its normalized text embedding.
type PromptEmbeddings = Vec<(usize, Vec<f32>)>;

pub struct ClipModel {
    session: Session,
    tokenizer: tokenizers::Tokenizer,
    /// Cached text embeddings: categories they were made for, and per
    /// prompt (category index, embedding).
    text: Option<(Vec<String>, PromptEmbeddings)>,
}

impl ClipModel {
    pub fn load(
        env: &ort::environment::Environment,
        dir: &std::path::Path,
    ) -> Result<Self, String> {
        models::verify_set(&models::CLIP, dir)?;
        let session = Session::builder(env)
            .and_then(|mut b| b.commit_from_file(dir.join(models::CLIP.files[0].name)))
            .map_err(|e| format!("cannot load the tagging model: {e}"))?;
        let tokenizer = tokenizers::Tokenizer::from_file(dir.join(models::CLIP.files[1].name))
            .map_err(|e| format!("cannot load the tokenizer: {e}"))?;
        Ok(Self {
            session,
            tokenizer,
            text: None,
        })
    }

    fn tokens(&self, prompts: &[String]) -> Result<(Array2<i64>, Array2<i64>), String> {
        let enc: Vec<_> = prompts
            .iter()
            .map(|p| {
                self.tokenizer
                    .encode(p.as_str(), true)
                    .map_err(|e| e.to_string())
            })
            .collect::<Result<_, _>>()?;
        let len = enc.iter().map(|e| e.get_ids().len()).max().unwrap_or(1);
        let ids = Array2::from_shape_fn((enc.len(), len), |(i, j)| {
            *enc[i].get_ids().get(j).unwrap_or(&49407) as i64
        });
        let mask = Array2::from_shape_fn((enc.len(), len), |(i, j)| {
            (j < enc[i].get_ids().len()) as i64
        });
        Ok((ids, mask))
    }

    fn run(
        &mut self,
        ids: Array2<i64>,
        mask: Array2<i64>,
        pixels: Array4<f32>,
        output: &str,
    ) -> Result<Vec<Vec<f32>>, String> {
        let t = |e: ort::Error| e.to_string();
        let out = self
            .session
            .run(ort::inputs![
                "input_ids" => Tensor::from_array(ids).map_err(t)?,
                "attention_mask" => Tensor::from_array(mask).map_err(t)?,
                "pixel_values" => Tensor::from_array(pixels).map_err(t)?,
            ])
            .map_err(|e| format!("tagging failed: {e}"))?;
        let a = out[output]
            .try_extract_array::<f32>()
            .map_err(|e| e.to_string())?;
        Ok(a.outer_iter()
            .map(|row| {
                let mut v: Vec<f32> = row.iter().copied().collect();
                normalize(&mut v);
                v
            })
            .collect())
    }

    fn text_embeddings(&mut self, categories: &[String]) -> Result<Vec<(usize, Vec<f32>)>, String> {
        if let Some((cats, emb)) = &self.text {
            if cats == categories {
                return Ok(emb.clone());
            }
        }
        let mut owner = Vec::new();
        let mut prompts = Vec::new();
        for (i, c) in categories.iter().enumerate() {
            for p in phrases(c) {
                owner.push(i);
                prompts.push(p);
            }
        }
        let (ids, mask) = self.tokens(&prompts)?;
        let blank = Array4::<f32>::zeros((1, 3, SIDE as usize, SIDE as usize));
        let emb = self.run(ids, mask, blank, "text_embeds")?;
        let pairs: Vec<(usize, Vec<f32>)> = owner.into_iter().zip(emb).collect();
        self.text = Some((categories.to_vec(), pairs.clone()));
        Ok(pairs)
    }

    /// The best category for `image` and its confidence (softmax over the
    /// categories' best-phrase scores).
    pub fn classify(
        &mut self,
        image: &RgbaImage,
        categories: &[String],
    ) -> Result<(String, f32), String> {
        if categories.is_empty() {
            return Err("no categories".to_owned());
        }
        let text = self.text_embeddings(categories)?;
        let (ids, mask) = self.tokens(&["pixel art".to_owned()])?;
        let img = self
            .run(ids, mask, preprocess(image), "image_embeds")?
            .remove(0);
        let mut score = vec![f32::NEG_INFINITY; categories.len()];
        for (i, t) in &text {
            let s = LOGIT_SCALE * t.iter().zip(&img).map(|(a, b)| a * b).sum::<f32>();
            score[*i] = score[*i].max(s);
        }
        let max = score.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        let sum: f32 = score.iter().map(|s| (s - max).exp()).sum();
        let best = (0..score.len())
            .max_by(|&a, &b| score[a].total_cmp(&score[b]))
            .expect("non-empty");
        Ok((categories[best].clone(), 1.0 / sum))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TagConfig {
    pub runtime_paths: Vec<PathBuf>,
    pub model_dir: PathBuf,
    pub source: Source,
}

impl TagConfig {
    /// No tagging (no ONNX Runtime is looked for): new assets stay "other".
    pub fn disabled() -> Self {
        Self {
            runtime_paths: Vec::new(),
            ..Self::default()
        }
    }
}

impl Default for TagConfig {
    fn default() -> Self {
        Self {
            runtime_paths: sam::default_runtime_paths(),
            model_dir: models::default_dir().join("clip"),
            source: Source::Http(models::CLIP.base_url.to_owned()),
        }
    }
}

/// One tagging job.
pub struct TagJob {
    pub id: u64,
    pub image: RgbaImage,
    /// Explicit Auto-Tag (may replace a manual category).
    pub force: bool,
}

#[derive(Debug)]
pub enum TagEvent {
    Availability(Availability),
    Tagged {
        id: u64,
        category: String,
        confidence: f32,
        force: bool,
    },
    /// A job that could not be tagged (no model): its id and why.
    Skipped {
        id: u64,
        reason: String,
    },
}

#[derive(Default)]
struct Slot {
    jobs: VecDeque<TagJob>,
    categories: Vec<String>,
    reprobe: bool,
    quit: bool,
}

type Shared = Arc<(Mutex<Slot>, Condvar)>;

/// The tagging worker; created on the first tagging request, so nothing
/// ML-related loads before then.
pub struct Tagger {
    shared: Shared,
    events: Receiver<TagEvent>,
    handle: Option<JoinHandle<()>>,
}

impl Tagger {
    pub fn new(config: TagConfig, notify: Arc<dyn Fn() + Send + Sync>) -> Self {
        let shared: Shared = Arc::default();
        let (tx, events) = mpsc::channel();
        let s = shared.clone();
        let handle = std::thread::Builder::new()
            .name("tagger".to_owned())
            .spawn(move || run(config, s, tx, notify))
            .expect("spawn tagger");
        Self {
            shared,
            events,
            handle: Some(handle),
        }
    }

    /// Queues jobs, tagged against `categories`.
    pub fn tag(&self, jobs: Vec<TagJob>, categories: Vec<String>) {
        let (lock, cvar) = &*self.shared;
        let mut slot = lock.lock().expect("tagger lock");
        slot.jobs.extend(jobs);
        slot.categories = categories;
        cvar.notify_all();
    }

    /// Checks again for the model (after a download).
    pub fn reprobe(&self) {
        let (lock, cvar) = &*self.shared;
        lock.lock().expect("tagger lock").reprobe = true;
        cvar.notify_all();
    }

    pub fn poll(&self) -> Vec<TagEvent> {
        self.events.try_iter().collect()
    }

    /// Jobs still waiting.
    pub fn pending(&self) -> usize {
        self.shared.0.lock().map(|s| s.jobs.len()).unwrap_or(0)
    }
}

impl Drop for Tagger {
    fn drop(&mut self) {
        let (lock, cvar) = &*self.shared;
        if let Ok(mut slot) = lock.lock() {
            slot.quit = true;
        }
        cvar.notify_all();
        drop(self.handle.take());
    }
}

enum Model {
    Unknown,
    Ready(Box<ClipModel>),
    Missing(Availability),
}

fn probe(config: &TagConfig) -> Model {
    if config.runtime_paths.is_empty() {
        return Model::Missing(Availability::Unavailable(
            "ONNX Runtime not found".to_owned(),
        ));
    }
    let env = match sam::runtime(&config.runtime_paths) {
        Ok(env) => env,
        Err(reason) => return Model::Missing(Availability::Unavailable(reason)),
    };
    if !models::present_set(&models::CLIP, &config.model_dir) {
        return Model::Missing(Availability::NeedsModels(None));
    }
    match ClipModel::load(env, &config.model_dir) {
        Ok(m) => Model::Ready(Box::new(m)),
        Err(reason) => Model::Missing(Availability::NeedsModels(Some(reason))),
    }
}

fn run(
    config: TagConfig,
    shared: Shared,
    tx: Sender<TagEvent>,
    notify: Arc<dyn Fn() + Send + Sync>,
) {
    let send = |e: TagEvent| {
        let _ = tx.send(e);
        notify();
    };
    let mut model = Model::Unknown;
    loop {
        let (job, categories, reprobe) = {
            let (lock, cvar) = &*shared;
            let Ok(mut slot) = lock.lock() else { return };
            while !slot.quit && slot.jobs.is_empty() && !slot.reprobe {
                slot = match cvar.wait(slot) {
                    Ok(s) => s,
                    Err(_) => return,
                };
            }
            if slot.quit {
                return;
            }
            (
                slot.jobs.pop_front(),
                slot.categories.clone(),
                std::mem::take(&mut slot.reprobe),
            )
        };
        if reprobe || matches!(model, Model::Unknown) {
            model = probe(&config);
            let availability = match &model {
                Model::Ready(_) => Availability::Ready,
                Model::Missing(a) => a.clone(),
                Model::Unknown => Availability::Unavailable("not checked".to_owned()),
            };
            send(TagEvent::Availability(availability));
        }
        let Some(job) = job else { continue };
        match &mut model {
            Model::Ready(m) => match m.classify(&job.image, &categories) {
                Ok((category, confidence)) => send(TagEvent::Tagged {
                    id: job.id,
                    category,
                    confidence,
                    force: job.force,
                }),
                Err(reason) => send(TagEvent::Skipped { id: job.id, reason }),
            },
            Model::Missing(a) => {
                let reason = match a {
                    Availability::NeedsModels(None) => {
                        "the tagging model is not installed".to_owned()
                    }
                    Availability::NeedsModels(Some(r)) | Availability::Unavailable(r) => r.clone(),
                    Availability::Ready => String::new(),
                };
                send(TagEvent::Skipped { id: job.id, reason });
            }
            Model::Unknown => {}
        }
    }
}
