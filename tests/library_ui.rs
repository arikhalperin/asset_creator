//! The Asset Library through the app: panel, editing, categories,
//! persistence across restarts, auto-tagging with and without the model.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use egui::Pos2;
use game_asset_creator::asset_library::CategorySource;
use game_asset_creator::extract::models::{self, Source};
use game_asset_creator::extract::sam;
use game_asset_creator::extract::segment::Availability;
use game_asset_creator::library::pack::{ExportFormat, SheetOptions};
use game_asset_creator::library::tagger::TagConfig;
use game_asset_creator::library_ui::LibraryAction;
use game_asset_creator::ui::{Consent, Panel};

mod common;
use common::Harness;

fn fixture(path: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(path)
}

fn wait_until(h: &mut Harness, what: &str, mut done: impl FnMut(&Harness) -> bool) {
    let start = Instant::now();
    while !done(h) {
        assert!(
            start.elapsed() < Duration::from_secs(60),
            "timed out waiting for {what}"
        );
        std::thread::sleep(Duration::from_millis(5));
        h.frame(vec![]);
    }
}

fn has_text(out: &egui::FullOutput, needle: &str) -> bool {
    out.shapes.iter().any(|s| match &s.shape {
        egui::Shape::Text(t) => t.galley.text().contains(needle),
        _ => false,
    })
}

fn text_pos(out: &egui::FullOutput, needle: &str) -> Option<Pos2> {
    out.shapes.iter().find_map(|s| match &s.shape {
        egui::Shape::Text(t) if t.galley.text() == needle => Some(t.pos),
        _ => None,
    })
}

/// Opens the flat sprite sheet and adds its 19 sprites via Auto-Detect.
fn add_sheet(h: &mut Harness) {
    h.app.open_path(&fixture("extract/sheet_flat.png")).unwrap();
    h.frames(2);
    assert!(h.app.open_auto_detect());
    assert_eq!(h.app.auto_detect_add(), 19);
    h.frames(2);
}

#[test]
fn panel_lists_filters_searches_and_edits() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = Harness::new();
    h.app.open_library(dir.path());
    add_sheet(&mut h);
    assert!(h.app.is_panel_open(Panel::AssetLibrary));
    let out = h.frame(vec![]);
    assert!(has_text(&out, "19 assets"));
    assert!(has_text(&out, "All 19") && has_text(&out, "other 19") && has_text(&out, "potion 0"));
    assert!(has_text(&out, "sheet_flat 1"));
    // Every asset records how it was extracted.
    let first = &h.app.state.assets.assets()[0];
    assert_eq!(first.origin.method, "auto_detect");
    assert_eq!(first.origin.params["min_size"], 16);
    assert!(first
        .origin
        .file
        .as_deref()
        .unwrap()
        .ends_with("sheet_flat.png"));

    // Click selects; set the category, tags, and name.
    let pos = text_pos(&out, "sheet_flat 2").expect("asset shown");
    h.click_at(pos + egui::vec2(4.0, 4.0));
    let id = *h.app.asset_view().selected.iter().next().expect("selected");
    assert_eq!(h.app.state.assets.get(id).unwrap().name, "sheet_flat 2");
    h.app
        .library_action(LibraryAction::SetCategory(vec![id], "potion".into()));
    h.app.library_action(LibraryAction::SetTags(
        id,
        vec!["blue".into(), "glass".into()],
    ));
    h.app
        .library_action(LibraryAction::Rename(id, "blue bottle".into()));
    let a = h.app.state.assets.get(id).unwrap();
    assert_eq!(
        (a.category.as_str(), a.category_source),
        ("potion", CategorySource::Manual)
    );
    assert_eq!(a.tags, vec!["blue", "glass"]);
    // Search and chips filter the grid.
    h.app.asset_view_mut().search = "GLASS".into();
    assert_eq!(h.app.asset_view().visible(&h.app.state.assets).len(), 1);
    h.app.asset_view_mut().search.clear();
    h.app.asset_view_mut().chips.insert("potion".into());
    let out = h.frame(vec![]);
    assert!(has_text(&out, "blue bottle") && !has_text(&out, "sheet_flat 1"));
    assert!(has_text(&out, "potion 1"));
    h.app.asset_view_mut().chips.clear();

    // Categories: add, rename, remove (assets fall back to "other").
    h.app
        .library_action(LibraryAction::AddCategory("Bottle".into()));
    h.app
        .library_action(LibraryAction::SetCategory(vec![id], "bottle".into()));
    h.app.library_action(LibraryAction::RenameCategory(
        "bottle".into(),
        "flask".into(),
    ));
    assert_eq!(h.app.state.assets.get(id).unwrap().category, "flask");
    h.app
        .library_action(LibraryAction::RemoveCategory("flask".into()));
    assert_eq!(h.app.state.assets.get(id).unwrap().category, "other");
    h.app
        .library_action(LibraryAction::RemoveCategory("other".into()));
    assert!(h
        .app
        .state
        .assets
        .categories()
        .contains(&"other".to_owned()));

    // Remove asks first.
    let out = h.frame(vec![]);
    h.click_at(text_pos(&out, "Remove…").unwrap() + egui::vec2(4.0, 4.0));
    h.frames(2);
    let out = h.frame(vec![]);
    assert!(has_text(&out, "Remove 1 asset from the library?"));
    let button = out
        .shapes
        .iter()
        .rev()
        .filter_map(|s| match &s.shape {
            egui::Shape::Text(t) if t.galley.text() == "Remove" => Some(t.pos),
            _ => None,
        })
        .next()
        .unwrap();
    h.click_at(button + egui::vec2(4.0, 4.0));
    assert!(h.app.state.assets.get(id).is_none());
    assert_eq!(h.app.state.assets.len(), 18);
}

#[test]
fn the_library_persists_across_app_restarts() {
    let dir = tempfile::tempdir().unwrap();
    let before = {
        let mut h = Harness::new();
        h.app.open_library(dir.path());
        add_sheet(&mut h);
        let id = h.app.state.assets.assets()[2].id;
        h.app
            .library_action(LibraryAction::SetCategory(vec![id], "chalice".into()));
        h.app
            .library_action(LibraryAction::SetTags(id, vec!["gold".into()]));
        h.app
            .library_action(LibraryAction::AddCategory("crown".into()));
        h.app.state.assets.assets().to_vec()
    };
    let mut h = Harness::new();
    h.app.open_library(dir.path());
    assert_eq!(h.app.state.assets.assets(), &before[..]);
    assert!(h
        .app
        .state
        .assets
        .categories()
        .contains(&"crown".to_owned()));
    let out = h.frame(vec![]);
    assert!(has_text(&out, "19 assets") && has_text(&out, "chalice 1"));
    // Export from the restored library.
    let ids: Vec<u64> = before.iter().map(|a| a.id).collect();
    let out_dir = tempfile::tempdir().unwrap();
    let files = h
        .app
        .export_assets(
            &ids,
            ExportFormat::Sheet(SheetOptions::default()),
            &out_dir.path().join("sprites.png"),
        )
        .unwrap();
    assert_eq!(files.len(), 2);
}

#[test]
fn without_the_model_new_assets_are_other_and_correctable() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = Harness::new();
    h.app.open_library(dir.path());
    h.app.set_tag_config(TagConfig {
        runtime_paths: Vec::new(),
        model_dir: dir.path().join("no-model"),
        source: Source::Directory(dir.path().join("no-model")),
    });
    add_sheet(&mut h);
    wait_until(&mut h, "tagging availability", |h| {
        h.app.tag_availability().is_some()
    });
    h.frames(3);
    let out = h.frame(vec![]);
    assert!(has_text(&out, "Auto-tag off"), "panel says why");
    assert!(h
        .app
        .state
        .assets
        .assets()
        .iter()
        .all(|a| a.category == "other"));
    let id = h.app.state.assets.assets()[0].id;
    h.app
        .library_action(LibraryAction::SetCategory(vec![id], "chalice".into()));
    assert_eq!(h.app.state.assets.get(id).unwrap().category, "chalice");
}

#[test]
fn missing_model_asks_first_and_a_damaged_download_is_refused() {
    let runtime = sam::default_runtime_paths();
    if sam::runtime(&runtime).is_err() {
        eprintln!("skipped: ONNX Runtime not installed");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let bad = tempfile::tempdir().unwrap();
    for f in models::CLIP.files {
        std::fs::write(bad.path().join(f.name), b"not a model").unwrap();
    }
    let mut h = Harness::new();
    h.app.open_library(&dir.path().join("lib"));
    h.app.set_tag_config(TagConfig {
        runtime_paths: runtime,
        model_dir: dir.path().join("clip"),
        source: Source::Directory(bad.path().to_path_buf()),
    });
    assert!(
        !h.app.tagger_started(),
        "nothing loads before the first asset"
    );
    add_sheet(&mut h);
    assert!(h.app.tagger_started());
    wait_until(&mut h, "consent question", |h| {
        h.app.tag_consent() == Consent::Asking
    });
    h.frames(2);
    let out = h.frame(vec![]);
    assert!(has_text(&out, "Download the tagging model?"));
    h.app.start_tag_download();
    wait_until(&mut h, "download end", |h| {
        !h.app.tag_download_in_progress()
    });
    let out = h.frame(vec![]);
    assert!(has_text(&out, "failed verification"));
    assert!(!models::present_set(
        &models::CLIP,
        &dir.path().join("clip")
    ));
}

/// With the real model (`GAC_CLIP_DIR`): new assets are tagged in the
/// background, a hand-set category survives automatic tagging, Auto-Tag
/// re-tags on request.
#[test]
fn assets_are_tagged_automatically_and_manual_categories_are_kept() {
    let Some(clip) = std::env::var_os("GAC_CLIP_DIR").map(PathBuf::from) else {
        eprintln!("skipped: set GAC_CLIP_DIR to the OpenCLIP model folder");
        return;
    };
    let runtime = sam::default_runtime_paths();
    if sam::runtime(&runtime).is_err() {
        eprintln!("skipped: ONNX Runtime not installed");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let mut h = Harness::new();
    h.app.open_library(dir.path());
    h.app.set_tag_config(TagConfig {
        runtime_paths: runtime,
        model_dir: clip,
        source: Source::Directory(PathBuf::from("/nonexistent")),
    });
    // Two potions from the labeled fixture, via Add Selection to Library.
    for file in ["tagging/potion_01.png", "tagging/potion_03.png"] {
        let doc = h.app.open_path(&fixture(file)).unwrap();
        h.frames(2);
        assert!(h.app.select_all());
        assert!(h.app.add_selection_to_library());
        let _ = doc;
    }
    wait_until(&mut h, "auto tags", |h| {
        h.app
            .state
            .assets
            .assets()
            .iter()
            .all(|a| a.category_source == CategorySource::Auto)
    });
    assert_eq!(h.app.tag_availability(), Some(&Availability::Ready));
    for a in h.app.state.assets.assets() {
        assert_eq!(a.category, "potion", "{}", a.name);
        assert!(a.confidence.unwrap() > 0.0);
    }
    let out = h.frame(vec![]);
    assert!(has_text(&out, "Auto-tag: OpenCLIP"));
    // A manual category stays through automatic tagging; Auto-Tag replaces it.
    let id = h.app.state.assets.assets()[0].id;
    h.app
        .library_action(LibraryAction::SetCategory(vec![id], "wall".into()));
    h.app.queue_tagging(&[id], false);
    h.frames(20);
    assert_eq!(h.app.state.assets.get(id).unwrap().category, "wall");
    h.app.library_action(LibraryAction::AutoTag(vec![id]));
    wait_until(&mut h, "re-tag", |h| {
        h.app.state.assets.get(id).unwrap().category == "potion"
    });
}

/// The panel wraps its chips and grid to its own width (nothing drawn past
/// the window's right edge).
#[test]
fn panel_content_wraps_inside_the_panel() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = Harness::new();
    h.app.open_library(dir.path());
    add_sheet(&mut h);
    h.frames(2);
    let out = h.frame(vec![]);
    let screen_right = common::SCREEN.x;
    let mut checked = 0;
    for s in &out.shapes {
        if let egui::Shape::Text(t) = &s.shape {
            let text = t.galley.text();
            if text.starts_with("sheet_flat ")
                || text.starts_with("other ")
                || text.starts_with("potion ")
                || text.starts_with("deathtrap ")
            {
                let right = t.pos.x + t.galley.size().x;
                assert!(right <= screen_right + 0.5, "{text:?} ends at {right}");
                checked += 1;
            }
        }
    }
    assert!(checked >= 4, "found the panel's texts ({checked})");
}

/// After a restart, assets left untagged are tagged when the model is
/// installed; without it, startup asks nothing and loads nothing.
#[test]
fn untagged_assets_are_tagged_after_a_restart() {
    let dir = tempfile::tempdir().unwrap();
    {
        let mut h = Harness::new();
        h.app.open_library(dir.path());
        add_sheet(&mut h);
    }
    // Startup without the model: no question, no worker.
    let mut h = Harness::new();
    h.app.set_tag_config(TagConfig {
        runtime_paths: sam::default_runtime_paths(),
        model_dir: dir.path().join("no-model"),
        source: Source::Directory(dir.path().join("no-model")),
    });
    h.app.open_library(dir.path());
    h.frames(3);
    assert!(!h.app.tagger_started());
    assert_eq!(h.app.tag_consent(), Consent::NotAsked);

    let Some(clip) = std::env::var_os("GAC_CLIP_DIR").map(PathBuf::from) else {
        eprintln!("skipped model half: set GAC_CLIP_DIR to the OpenCLIP model folder");
        return;
    };
    let runtime = sam::default_runtime_paths();
    if sam::runtime(&runtime).is_err() {
        eprintln!("skipped model half: ONNX Runtime not installed");
        return;
    }
    let mut h = Harness::new();
    h.app.set_tag_config(TagConfig {
        runtime_paths: runtime,
        model_dir: clip,
        source: Source::Directory(PathBuf::from("/nonexistent")),
    });
    h.app.open_library(dir.path());
    assert!(h.app.tagger_started());
    wait_until(&mut h, "startup tags", |h| {
        h.app
            .state
            .assets
            .assets()
            .iter()
            .all(|a| a.category_source == CategorySource::Auto)
    });
    assert_eq!(h.app.state.assets.len(), 19);
}
