//! The persistent asset library, sprite-sheet export, and auto-tagging
//! accuracy (tests/fixtures/tagging; CC0 Kenney sprites).

use std::path::PathBuf;

use game_asset_creator::asset_library::{AssetLibrary, CategorySource, Origin};
use game_asset_creator::document::PixelRect;
use game_asset_creator::extract::{models, sam};
use game_asset_creator::library::pack::{self, ExportFormat, Layout, SheetOptions};
use game_asset_creator::library::tagger::ClipModel;
use image::{Rgba, RgbaImage};

fn sprite(w: u32, h: u32, seed: u8) -> RgbaImage {
    RgbaImage::from_fn(w, h, |x, y| {
        let a = if (x + y) % 5 == 0 { 0 } else { 255 };
        Rgba([seed, (x * 9) as u8, (y * 7) as u8, a])
    })
}

fn sample(dir: &std::path::Path) -> AssetLibrary {
    let mut lib = AssetLibrary::open(dir);
    lib.add_with(
        "sheet.png",
        sprite(12, 9, 10),
        PixelRect::new(3, 4, 12, 9),
        Origin {
            file: Some("/art/sheet.png".to_owned()),
            frame: None,
            method: "auto_detect".to_owned(),
            params: serde_json::json!({"min_size": 16, "merge_distance": 2}),
        },
    );
    let frames: Vec<RgbaImage> = (0..3).map(|i| sprite(8, 10, 50 + i)).collect();
    lib.add_animation_with(
        "walk.mp4",
        frames,
        PixelRect::new(100, 50, 8, 10),
        Origin {
            file: Some("/clips/walk.mp4".to_owned()),
            frame: Some(42),
            method: "video".to_owned(),
            params: serde_json::json!({"every": 1}),
        },
    );
    lib
}

#[test]
fn library_survives_a_restart_with_all_metadata() {
    let dir = tempfile::tempdir().unwrap();
    let before = {
        let mut lib = sample(dir.path());
        let id = lib.assets()[0].id;
        lib.rename(id, "golden cup");
        lib.add_category("crown");
        lib.set_category(&[id], "chalice");
        lib.set_tags(id, vec!["gold".into(), " shiny ".into(), "".into()]);
        let anim = lib.assets()[1].id;
        lib.apply_auto_tag(anim, "skeleton", 0.83, false);
        lib.assets().to_vec()
    };
    // "Restart": open the same folder again.
    let lib = AssetLibrary::open(dir.path());
    assert!(lib.notices().is_empty(), "{:?}", lib.notices());
    assert_eq!(lib.assets(), &before[..]);
    let cup = &lib.assets()[0];
    assert_eq!(cup.name, "golden cup");
    assert_eq!(
        (cup.category.as_str(), cup.category_source),
        ("chalice", CategorySource::Manual)
    );
    assert_eq!(cup.tags, vec!["gold", "shiny"]);
    assert_eq!(cup.origin.method, "auto_detect");
    assert_eq!(cup.origin.params["merge_distance"], 2);
    let walk = &lib.assets()[1];
    assert_eq!(walk.frames.len(), 3);
    assert_eq!(walk.origin.frame, Some(42));
    assert_eq!(
        (walk.category.as_str(), walk.confidence),
        ("skeleton", Some(0.83))
    );
    assert!(lib.categories().contains(&"crown".to_owned()));
    // Manual categories are not overwritten by automatic tagging.
    let cup_id = cup.id;
    let mut lib = lib;
    assert!(!lib.apply_auto_tag(cup_id, "potion", 0.9, false));
    assert!(
        lib.apply_auto_tag(cup_id, "potion", 0.9, true),
        "explicit Auto-Tag may"
    );
    // New ids continue after a restart.
    let id = lib.add("x.png", sprite(2, 2, 1), PixelRect::new(0, 0, 2, 2));
    assert!(id > before.iter().map(|a| a.id).max().unwrap());
}

#[test]
fn damaged_files_are_skipped_and_damaged_metadata_kept_aside() {
    let dir = tempfile::tempdir().unwrap();
    let id = {
        let lib = sample(dir.path());
        lib.assets()[0].id
    };
    std::fs::write(
        dir.path().join("assets").join(format!("{id}.png")),
        b"not a png",
    )
    .unwrap();
    let lib = AssetLibrary::open(dir.path());
    assert_eq!(lib.len(), 1, "the damaged asset is skipped, the rest loads");
    assert_eq!(lib.notices().len(), 1);
    std::fs::write(dir.path().join("library.json"), b"{ broken").unwrap();
    let lib = AssetLibrary::open(dir.path());
    assert!(lib.is_empty());
    assert!(dir.path().join("library.json.bad").exists());
    assert!(lib.notices()[0].contains("library.json.bad"));
}

#[test]
fn a_damaged_entry_keeps_its_metadata_and_files_through_later_saves() {
    let dir = tempfile::tempdir().unwrap();
    let before = sample(dir.path()).assets().to_vec();
    let png = dir
        .path()
        .join("assets")
        .join(format!("{}.png", before[0].id));
    let good = std::fs::read(&png).unwrap();
    std::fs::write(&png, b"not a png").unwrap();
    let new_id = {
        let mut lib = AssetLibrary::open(dir.path());
        assert_eq!(lib.len(), 1);
        // Later saves must not drop the damaged entry or reuse its id.
        let id = lib.add("more.png", sprite(5, 5, 9), PixelRect::new(0, 0, 5, 5));
        assert!(before.iter().all(|a| a.id != id));
        id
    };
    assert_eq!(std::fs::read(&png).unwrap(), b"not a png", "file untouched");
    // Repairing the file brings the asset back with all its metadata.
    std::fs::write(&png, good).unwrap();
    let lib = AssetLibrary::open(dir.path());
    assert!(lib.notices().is_empty());
    assert_eq!(lib.len(), 3);
    assert_eq!(lib.get(before[0].id).unwrap(), &before[0]);
    assert!(lib.get(new_id).is_some());
}

#[test]
fn removing_assets_and_categories_updates_disk() {
    let dir = tempfile::tempdir().unwrap();
    let mut lib = sample(dir.path());
    let (a, b) = (lib.assets()[0].id, lib.assets()[1].id);
    lib.add_category("crown");
    lib.set_category(&[a], "crown");
    assert!(lib.remove_category("crown"));
    assert!(!lib.remove_category("other"), "other stays");
    assert_eq!(lib.get(a).unwrap().category, "other");
    lib.remove(&[b]);
    assert!(!dir
        .path()
        .join("assets")
        .join(format!("{b}_01.png"))
        .exists());
    let lib = AssetLibrary::open(dir.path());
    assert_eq!(lib.len(), 1);
    assert!(!lib.categories().contains(&"crown".to_owned()));
    // Search matches name, tags, category, and source file.
    let mut lib = lib;
    lib.set_tags(a, vec!["Treasure".into()]);
    let asset = lib.get(a).unwrap();
    for q in ["treasure", "SHEET", "other", "sheet.png", ""] {
        assert!(asset.matches(q), "{q}");
    }
    assert!(!asset.matches("dragon"));
}

#[test]
fn sprite_sheet_atlas_coordinates_match_the_assets() {
    let dir = tempfile::tempdir().unwrap();
    let mut lib = sample(dir.path());
    for i in 0..9 {
        lib.add(
            "props.png",
            sprite(4 + i * 3, 30 - i * 2, 100 + i as u8),
            PixelRect::new(0, 0, 1, 1),
        );
    }
    let assets: Vec<_> = lib.assets().iter().collect();
    let out = tempfile::tempdir().unwrap();
    for layout in [Layout::Grid { columns: None }, Layout::MaxRects] {
        for (pad, pot) in [(0, false), (2, false), (3, true)] {
            let opts = SheetOptions {
                layout,
                padding: pad,
                power_of_two: pot,
                max_size: 4096,
            };
            let path = out.path().join(format!("sheet_{pad}_{pot}.png"));
            let files = pack::export(&assets, ExportFormat::Sheet(opts), &path).unwrap();
            let png = image::open(&files[0]).unwrap().into_rgba8();
            let atlas: serde_json::Value =
                serde_json::from_str(&std::fs::read_to_string(&files[1]).unwrap()).unwrap();
            assert_eq!(atlas["meta"]["size"]["w"], png.width());
            assert_eq!(
                atlas["meta"]["image"],
                files[0].file_name().unwrap().to_str().unwrap()
            );
            let frames = atlas["frames"].as_object().unwrap();
            assert_eq!(
                frames.len(),
                10 + 3,
                "9 props + 1 single + 3 animation frames"
            );
            let mut rects = Vec::new();
            for asset in &assets {
                for (i, img) in asset.all_frames().into_iter().enumerate() {
                    let key = if asset.is_animation() {
                        format!("{}_{:02}", asset.name, i + 1)
                    } else {
                        asset.name.clone()
                    };
                    let f = &frames[&key]["frame"];
                    let (x, y, w, h) = (
                        f["x"].as_u64().unwrap() as u32,
                        f["y"].as_u64().unwrap() as u32,
                        f["w"].as_u64().unwrap() as u32,
                        f["h"].as_u64().unwrap() as u32,
                    );
                    assert!(
                        x >= pad
                            && y >= pad
                            && x + w + pad <= png.width()
                            && y + h + pad <= png.height()
                    );
                    let cut = image::imageops::crop_imm(&png, x, y, w, h).to_image();
                    assert_eq!(&cut, img, "{key} pixels");
                    rects.push((x, y, w, h));
                }
            }
            for (i, a) in rects.iter().enumerate() {
                for b in &rects[i + 1..] {
                    let apart = a.0 + a.2 + pad <= b.0
                        || b.0 + b.2 + pad <= a.0
                        || a.1 + a.3 + pad <= b.1
                        || b.1 + b.3 + pad <= a.1;
                    assert!(apart, "{a:?} {b:?}");
                }
            }
            let walk = assets.iter().find(|a| a.is_animation()).unwrap();
            assert_eq!(
                atlas["animations"][&walk.name],
                serde_json::json!([
                    "walk animation 1_01",
                    "walk animation 1_02",
                    "walk animation 1_03"
                ])
            );
        }
    }
}

#[test]
fn animations_export_as_strips_or_numbered_pngs() {
    let dir = tempfile::tempdir().unwrap();
    let lib = sample(dir.path());
    let assets: Vec<_> = lib.assets().iter().collect();
    let out = tempfile::tempdir().unwrap();
    let files = pack::export(&assets, ExportFormat::Strips { padding: 1 }, out.path()).unwrap();
    let strip_png = files
        .iter()
        .find(|p| p.to_string_lossy().ends_with("walk animation 1.png"))
        .unwrap();
    let strip = image::open(strip_png).unwrap().into_rgba8();
    assert_eq!(
        strip.dimensions(),
        (1 + 3 * (8 + 1), 1 + 10 + 1),
        "frames left to right"
    );
    let atlas: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(strip_png.with_extension("json")).unwrap())
            .unwrap();
    assert_eq!(atlas["frames"]["walk animation 1_02"]["frame"]["x"], 10);
    assert_eq!(
        atlas["animations"]["walk animation 1"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    let pngs = tempfile::tempdir().unwrap();
    let files = pack::export(&assets, ExportFormat::Pngs, pngs.path()).unwrap();
    let names: Vec<String> = files
        .iter()
        .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        names,
        vec![
            "sheet 1.png",
            "walk animation 1_01.png",
            "walk animation 1_02.png",
            "walk animation 1_03.png"
        ]
    );
}

fn clip_dir() -> PathBuf {
    std::env::var_os("GAC_CLIP_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| models::default_dir().join("clip"))
}

/// Accuracy on the held-out half of the labeled fixture (prompt phrases were
/// chosen on the `tune` half only). Runs when ONNX Runtime and the model are
/// installed (`GAC_CLIP_DIR` may point at the model folder).
#[test]
fn auto_tagging_is_at_least_70_percent_correct_on_held_out_sprites() {
    let Ok(env) = sam::runtime(&sam::default_runtime_paths()) else {
        eprintln!("skipped: ONNX Runtime not installed");
        return;
    };
    if !models::present_set(&models::CLIP, &clip_dir()) {
        eprintln!("skipped: tagging model not in {}", clip_dir().display());
        return;
    }
    let mut model = ClipModel::load(env, &clip_dir()).unwrap();
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/tagging");
    let labels: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("labels.json")).unwrap()).unwrap();
    let categories: Vec<String> = labels["categories"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c.as_str().unwrap().to_owned())
        .collect();
    let (mut hit, mut total) = (0, 0);
    let mut per_category = std::collections::BTreeMap::<String, (u32, u32)>::new();
    for s in labels["sprites"].as_array().unwrap() {
        if s["split"] != "test" {
            continue;
        }
        let img = image::open(dir.join(s["file"].as_str().unwrap()))
            .unwrap()
            .into_rgba8();
        let (got, confidence) = model.classify(&img, &categories).unwrap();
        let want = s["category"].as_str().unwrap();
        assert!((0.0..=1.0).contains(&confidence));
        let e = per_category.entry(want.to_owned()).or_default();
        e.1 += 1;
        total += 1;
        if got == want {
            hit += 1;
            e.0 += 1;
        }
    }
    let accuracy = hit as f32 / total as f32;
    println!(
        "held-out accuracy {hit}/{total} = {:.0} %; per category {per_category:?}",
        accuracy * 100.0
    );
    assert!(accuracy >= 0.70, "accuracy {accuracy:.2}");
}

/// Release benchmark: `cargo test --release --test library -- --ignored --nocapture`.
#[test]
#[ignore]
fn bench_library_load_and_tagging() {
    let dir = tempfile::tempdir().unwrap();
    {
        let mut lib = AssetLibrary::open(dir.path());
        for i in 0..1000u32 {
            lib.add(
                "bulk.png",
                sprite(24 + i % 40, 24 + i % 30, (i % 250) as u8),
                PixelRect::new(0, 0, 1, 1),
            );
        }
    }
    let start = std::time::Instant::now();
    let lib = AssetLibrary::open(dir.path());
    let load = start.elapsed().as_secs_f64();
    assert_eq!(lib.len(), 1000);
    println!("load 1000 assets: {load:.2} s");
    let mut tag = None;
    if let (Ok(env), true) = (
        sam::runtime(&sam::default_runtime_paths()),
        models::present_set(&models::CLIP, &clip_dir()),
    ) {
        let mut model = ClipModel::load(env, &clip_dir()).unwrap();
        let cats: Vec<String> = lib.categories().to_vec();
        model.classify(&lib.assets()[0].image, &cats).unwrap(); // warm up (text embeddings)
        let start = std::time::Instant::now();
        for a in &lib.assets()[1..11] {
            model.classify(&a.image, &cats).unwrap();
        }
        tag = Some(start.elapsed().as_secs_f64() / 10.0);
        println!("tagging: {:.0} ms per asset", tag.unwrap() * 1000.0);
    }
    if !cfg!(debug_assertions) {
        assert!(load < 2.0, "load {load:.2} s");
        if let Some(t) = tag {
            assert!(t < 1.0, "tagging {t:.2} s");
        }
    }
}

#[test]
fn exports_never_overwrite_assets_with_the_same_name() {
    let dir = tempfile::tempdir().unwrap();
    let mut lib = sample(dir.path());
    let a = lib.add("x.png", sprite(6, 6, 1), PixelRect::new(0, 0, 6, 6));
    let b = lib.add("x.png", sprite(7, 5, 2), PixelRect::new(0, 0, 7, 5));
    let anim = lib.assets()[1].id;
    for id in [a, b, anim] {
        assert!(lib.rename(id, "hero"));
    }
    // A single asset named like an animation frame.
    let c = lib.add("x.png", sprite(4, 4, 3), PixelRect::new(0, 0, 4, 4));
    assert!(lib.rename(c, "hero_01"));
    let assets: Vec<_> = lib.assets().iter().collect();
    let frame_count = 1 + 1 + 3 + 1 + 1;
    let out = tempfile::tempdir().unwrap();
    let files = pack::export(
        &assets,
        ExportFormat::Sheet(SheetOptions::default()),
        &out.path().join("all.png"),
    )
    .unwrap();
    let atlas: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&files[1]).unwrap()).unwrap();
    let frames = atlas["frames"].as_object().unwrap();
    assert_eq!(frames.len(), frame_count, "every frame has its own entry");
    let anims = atlas["animations"].as_object().unwrap();
    let names = anims.values().next().unwrap().as_array().unwrap();
    assert_eq!(names.len(), 3);
    assert!(names
        .iter()
        .all(|n| frames.contains_key(n.as_str().unwrap())));
    // Strips: same names, and exporting twice into one folder, keep every file.
    let strips = tempfile::tempdir().unwrap();
    let first = pack::export(&assets, ExportFormat::Strips { padding: 0 }, strips.path()).unwrap();
    let second = pack::export(&assets, ExportFormat::Strips { padding: 0 }, strips.path()).unwrap();
    let mut all: Vec<_> = first.iter().chain(&second).cloned().collect();
    let n = all.len();
    all.sort();
    all.dedup();
    assert_eq!(all.len(), n, "no file written twice");
    let pngs = std::fs::read_dir(strips.path())
        .unwrap()
        .filter(|e| e.as_ref().unwrap().path().extension().unwrap() == "png")
        .count();
    assert_eq!(pngs, 2 * assets.len());
    assert!(strips.path().join("hero (2).png").exists());
}
