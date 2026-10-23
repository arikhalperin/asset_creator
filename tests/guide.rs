//! The user guide and README only name menu commands that exist: every
//! `Menu ▸ Command` they mention is found in that menu of the running app.

use egui::{Key, Pos2, Vec2};

mod common;
use common::Harness;

const MENUS: [&str; 8] = [
    "File", "Edit", "Select", "Image", "Layer", "Filter", "Extract", "View",
];

/// Every `` `Menu ▸ Command` `` in `text` whose menu is a top-level menu.
fn menu_paths(text: &str) -> Vec<(String, String)> {
    text.split('`')
        .skip(1)
        .step_by(2)
        .filter_map(|span| {
            let (menu, command) = span.split_once(" ▸ ")?;
            MENUS
                .contains(&menu)
                .then(|| (menu.to_owned(), command.trim().to_owned()))
        })
        .collect()
}

fn text_pos(out: &egui::FullOutput, needle: &str) -> Option<Pos2> {
    out.shapes.iter().find_map(|s| match &s.shape {
        egui::Shape::Text(t) if t.galley.text() == needle => Some(t.pos),
        _ => None,
    })
}

/// The texts shown in `menu` once it is opened.
fn menu_items(h: &mut Harness, menu: &str) -> Vec<String> {
    let out = h.frame(vec![]);
    let pos = text_pos(&out, menu).unwrap_or_else(|| panic!("no {menu} menu"));
    h.click_at(pos + Vec2::new(4.0, 4.0));
    h.frame(vec![]);
    let out = h.frame(vec![]);
    let items = out
        .shapes
        .iter()
        .filter_map(|s| match &s.shape {
            egui::Shape::Text(t) => Some(t.galley.text().to_owned()),
            _ => None,
        })
        .collect();
    let esc = h.key(Key::Escape, true);
    h.frame(vec![esc]);
    h.frames(2);
    items
}

#[test]
fn every_menu_command_in_the_docs_exists() {
    let root = env!("CARGO_MANIFEST_DIR");
    let mut paths = Vec::new();
    for doc in ["docs/user-guide.md", "README.md"] {
        let text = std::fs::read_to_string(format!("{root}/{doc}")).unwrap();
        paths.extend(menu_paths(&text));
    }
    assert!(paths.len() >= 30, "the guide names its menu commands");
    let mut h = Harness::new();
    h.new_doc(64, 64);
    let mut missing = Vec::new();
    for menu in MENUS {
        let wanted: Vec<&String> = paths
            .iter()
            .filter(|(m, _)| m == menu)
            .map(|(_, c)| c)
            .collect();
        if wanted.is_empty() {
            continue;
        }
        let items = menu_items(&mut h, menu);
        for command in wanted {
            if !items.iter().any(|t| t == command) {
                missing.push(format!("{menu} ▸ {command}"));
            }
        }
    }
    missing.sort();
    missing.dedup();
    assert!(missing.is_empty(), "not in the app's menus: {missing:#?}");
}
