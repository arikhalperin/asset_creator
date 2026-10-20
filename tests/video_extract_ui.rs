//! Whole-video extraction through the app: Extract Assets…, the Review tab,
//! and animation entries in the Asset Library (tests/fixtures/video/walk.mp4).

use std::path::PathBuf;
use std::time::{Duration, Instant};

use egui::{Modifiers, Pos2};
use game_asset_creator::extract_ui::AssetAction;
use game_asset_creator::review_ui::ReviewId;
use game_asset_creator::ui::Panel;
use game_asset_creator::video_ui::VideoId;

mod common;
use common::Harness;

fn walk() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/video/walk.mp4")
}

fn wait_until(h: &mut Harness, what: &str, mut done: impl FnMut(&Harness) -> bool) {
    let start = Instant::now();
    while !done(h) {
        assert!(
            start.elapsed() < Duration::from_secs(120),
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
        egui::Shape::Text(t) if t.galley.text().contains(needle) => Some(t.pos),
        _ => None,
    })
}

fn open_video(h: &mut Harness) -> VideoId {
    assert_eq!(h.app.open_path(&walk()), None);
    let id = *h.app.video_tabs().last().unwrap();
    wait_until(h, "first frame", |h| {
        h.app.video(id).unwrap().displayed().is_some()
    });
    h.frames(2);
    id
}

/// Runs Extract Assets… with default settings; returns the Review tab.
fn run_defaults(h: &mut Harness) -> ReviewId {
    let out = h.frame(vec![]);
    h.click_at(
        text_pos(&out, "Extract Assets").expect("button on the video tab") + egui::vec2(4.0, 4.0),
    );
    h.frames(2);
    let d = h.app.video_assets_dialog().expect("dialog opened");
    assert_eq!((d.from, d.to, d.every), (0, 199, 1));
    assert_eq!((d.detect.min_size, d.detect.merge_distance), (16, 2));
    assert!(h.app.start_video_assets());
    let out = h.frame(vec![]);
    assert!(h.app.video_assets_dialog().is_none() || has_text(&out, "Extract Assets"));
    wait_until(h, "review tab", |h| !h.app.review_tabs().is_empty());
    assert!(h.app.video_assets_dialog().is_none());
    *h.app.review_tabs().last().unwrap()
}

#[test]
fn extract_assets_review_and_add_yield_one_potion_and_one_walk() {
    let mut h = Harness::new();
    open_video(&mut h);
    let review = run_defaults(&mut h);
    h.frames(2);
    let out = h.frame(vec![]);
    assert!(has_text(&out, "walk.mp4 review"), "tab title");
    assert!(has_text(&out, "Animation: 8 poses"));
    assert!(has_text(&out, "Single: seen in 200 frames"));
    let r = h.app.review(review).unwrap();
    assert_eq!(r.clusters.len(), 2);
    assert!(r.keep.iter().all(|k| *k), "all kept by default");

    let (singles, animations) = h.app.review_add_kept(review);
    assert_eq!((singles, animations), (1, 1));
    assert!(h.app.review_tabs().is_empty(), "the tab closes");
    let assets = h.app.state.assets.assets();
    assert_eq!(assets.len(), 2);
    let potion = assets.iter().find(|a| !a.is_animation()).unwrap();
    assert_eq!(potion.name, "walk 1");
    let walk = assets.iter().find(|a| a.is_animation()).unwrap().clone();
    assert_eq!(walk.name, "walk animation 1");
    assert_eq!(walk.frames.len(), 8);
    let size = walk.frames[0].dimensions();
    assert!(
        walk.frames.iter().all(|f| f.dimensions() == size),
        "shared canvas"
    );
    for f in &walk.frames {
        let (w, h2) = f.dimensions();
        assert!(
            (0..w).any(|x| f.get_pixel(x, h2 - 1)[3] > 0)
                || (0..w).any(|x| f.get_pixel(x, h2 - 2)[3] > 0),
            "bottom-aligned"
        );
    }
    assert!(h.app.is_panel_open(Panel::AssetLibrary));
    let out = h.frame(vec![]);
    assert!(has_text(&out, "walk animation 1"));
    assert!(has_text(
        &out,
        &format!("8 frames, {} × {}", size.0, size.1)
    ));

    // Save writes the numbered sequence; Open as Document gives one layer per frame.
    let dir = tempfile::tempdir().unwrap();
    assert!(h
        .app
        .save_asset_to(walk.id, &dir.path().join("skeleton.png")));
    for i in 1..=8 {
        let p = dir.path().join(format!("skeleton_{i:02}.png"));
        assert_eq!(image::open(&p).unwrap().into_rgba8(), walk.frames[i - 1]);
    }
    let walk_id = walk.id;
    let doc = h.app.asset_action(AssetAction::Open(walk_id)).unwrap();
    let document = &h.app.state.document(doc).unwrap().document;
    assert_eq!(document.layers().len(), 8);
    assert_eq!(
        document.layer(0).unwrap().name,
        "Frame 1",
        "frame 1 at the bottom"
    );
    assert_eq!(document.layer(7).unwrap().name, "Frame 8");
    let all = h.app.save_all_assets_to(&dir.path().join("all")).unwrap();
    assert_eq!(all.len(), 9, "one PNG plus an 8-frame sequence");
}

#[test]
fn discard_and_merge_in_the_review_tab() {
    let mut h = Harness::new();
    open_video(&mut h);
    let review = run_defaults(&mut h);
    h.frames(2);
    // Discard the potion with its Keep checkbox.
    let out = h.frame(vec![]);
    let single = text_pos(&out, "Single: seen in").unwrap();
    let row = h
        .app
        .review(review)
        .unwrap()
        .clusters
        .iter()
        .position(|c| !c.is_animation())
        .unwrap();
    let keeps: Vec<Pos2> = out
        .shapes
        .iter()
        .filter_map(|s| match &s.shape {
            egui::Shape::Text(t) if t.galley.text() == "Keep" => Some(t.pos),
            _ => None,
        })
        .collect();
    let keep = keeps
        .into_iter()
        .min_by(|a, b| (a.y - single.y).abs().total_cmp(&(b.y - single.y).abs()))
        .unwrap();
    h.click_at(keep + egui::vec2(-8.0, 6.0));
    assert!(!h.app.review(review).unwrap().keep[row], "unchecked");
    assert_eq!(
        h.app.review_add_kept(review),
        (0, 1),
        "discarded clusters are not added"
    );

    // A second run: select both rows (click, Cmd-click) and merge.
    let review = run_defaults(&mut h);
    h.frames(2);
    let out = h.frame(vec![]);
    let a = text_pos(&out, "Animation: 8 poses").unwrap();
    let b = text_pos(&out, "Single: seen in").unwrap();
    h.click_at(a + egui::vec2(4.0, 4.0));
    h.modifiers = Modifiers::COMMAND;
    h.click_at(b + egui::vec2(4.0, 4.0));
    h.modifiers = Modifiers::NONE;
    assert_eq!(h.app.review(review).unwrap().selected.len(), 2);
    let out = h.frame(vec![]);
    h.click_at(text_pos(&out, "Merge").unwrap() + egui::vec2(4.0, 4.0));
    let r = h.app.review(review).unwrap();
    assert_eq!(r.clusters.len(), 1);
    assert_eq!(r.clusters[0].poses.len(), 9, "potion + 8 walk poses");
    assert_eq!(r.clusters[0].frames.len(), 200);
    let frames: Vec<u64> = r.clusters[0].poses.iter().map(|p| p.frame).collect();
    assert!(
        frames.windows(2).all(|w| w[0] <= w[1]),
        "ordered by first appearance"
    );
    let out = h.frame(vec![]);
    h.click_at(text_pos(&out, "Add Kept to Library").unwrap() + egui::vec2(4.0, 4.0));
    assert!(h.app.review_tabs().is_empty());
    let last = h.app.state.assets.assets().last().unwrap();
    assert_eq!(last.name, "walk animation 2");
    assert_eq!(last.frames.len(), 9);
}

#[test]
fn cancel_or_closing_the_review_discards_the_run() {
    let mut h = Harness::new();
    let video = open_video(&mut h);
    assert!(h.app.open_video_assets(video));
    assert!(h.app.start_video_assets());
    h.app.cancel_video_assets();
    h.frames(3);
    assert!(h.app.video_assets_dialog().is_none());
    assert!(h.app.review_tabs().is_empty());
    let review = run_defaults(&mut h);
    h.app.close_review(review);
    assert!(h.app.review(review).is_none());
    assert!(h.app.state.assets.is_empty());
}
