//! Shared headless egui harness for integration tests.
#![allow(dead_code)]

use egui::{Event, Key, Modifiers, PointerButton, Pos2, Vec2};
use game_asset_creator::{DocId, EditorApp};

pub const SCREEN: Vec2 = Vec2::new(1280.0, 800.0);
/// A point inside the document area of the default layout.
pub const CANVAS_POINT: Pos2 = Pos2::new(640.0, 420.0);

pub struct Harness {
    pub app: EditorApp,
    pub ctx: egui::Context,
    pub modifiers: Modifiers,
    sent_modifiers: Modifiers,
    pub pointer: Pos2,
    /// Input time for the next frames (`None` lets egui advance it).
    pub time: Option<f64>,
}

impl Harness {
    pub fn new() -> Self {
        let mut h = Self {
            app: EditorApp::new(),
            ctx: egui::Context::default(),
            modifiers: Modifiers::NONE,
            sent_modifiers: Modifiers::NONE,
            pointer: CANVAS_POINT,
            time: None,
        };
        h.frame(vec![]);
        h
    }

    pub fn frame(&mut self, mut events: Vec<Event>) -> egui::FullOutput {
        events.insert(0, Event::PointerMoved(self.pointer));
        if self.modifiers != self.sent_modifiers {
            events.insert(0, Event::ModifiersChanged(self.modifiers));
            self.sent_modifiers = self.modifiers;
        }
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(Pos2::ZERO, SCREEN)),
            events,
            time: self.time,
            ..Default::default()
        };
        let app = &mut self.app;
        let mut output = self.ctx.run_ui(input, |ui| app.show(ui));
        output.textures_delta.clear();
        output
    }

    pub fn frames(&mut self, n: usize) {
        for _ in 0..n {
            self.frame(vec![]);
        }
    }

    pub fn key(&mut self, key: Key, pressed: bool) -> Event {
        Event::Key {
            key,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers: self.modifiers,
        }
    }

    /// Presses and releases `key` with `modifiers` in one frame.
    pub fn shortcut(&mut self, modifiers: Modifiers, key: Key) {
        self.modifiers = modifiers;
        let down = self.key(key, true);
        let up = self.key(key, false);
        self.frame(vec![down, up]);
        self.modifiers = Modifiers::NONE;
        self.frame(vec![]);
    }

    pub fn new_doc(&mut self, w: u32, h: u32) -> DocId {
        let id = self.app.create_document(w, h, 72.0).unwrap();
        self.frames(2);
        self.settle_canvas(id);
        id
    }

    /// Runs frames until the canvas of `id` has drawn every change (big
    /// changes are refreshed over several frames).
    pub fn settle_canvas(&mut self, id: DocId) {
        for _ in 0..1000 {
            if self.app.canvas(id).is_none_or(|c| c.tiles().up_to_date()) {
                return;
            }
            self.frame(vec![]);
        }
        panic!("the canvas never caught up");
    }

    pub fn view(&self, id: DocId) -> game_asset_creator::canvas::ViewState {
        self.app.canvas(id).unwrap().view().expect("canvas shown")
    }

    pub fn uploads(&self, id: DocId) -> u64 {
        self.app.canvas(id).unwrap().tiles().uploads()
    }
}

impl Harness {
    pub fn press(&mut self, button: PointerButton, pressed: bool) -> Event {
        Event::PointerButton {
            pos: self.pointer,
            button,
            pressed,
            modifiers: self.modifiers,
        }
    }

    /// Clicks the primary button at the current pointer position.
    pub fn click(&mut self) {
        let down = self.press(PointerButton::Primary, true);
        self.frame(vec![down]);
        let up = self.press(PointerButton::Primary, false);
        self.frame(vec![up]);
    }

    /// Moves the pointer to `to` and clicks there.
    pub fn click_at(&mut self, to: Pos2) {
        self.pointer = to;
        self.frame(vec![]);
        self.click();
    }

    /// Drags with the primary button from `from` to `to` in `steps` moves.
    pub fn drag(&mut self, from: Pos2, to: Pos2, steps: usize) {
        self.pointer = from;
        self.frame(vec![]);
        let down = self.press(PointerButton::Primary, true);
        self.frame(vec![down]);
        for i in 1..=steps {
            self.pointer = from + (to - from) * (i as f32 / steps as f32);
            self.frame(vec![]);
        }
        let up = self.press(PointerButton::Primary, false);
        self.frame(vec![up]);
        self.frame(vec![]);
    }

    /// Screen position of an image point on the current document's canvas.
    pub fn screen_of(&self, id: DocId, image: Pos2) -> Pos2 {
        let canvas = self.app.canvas(id).unwrap();
        let rect = canvas.rect().unwrap();
        let view = canvas.view().unwrap();
        view.image_to_view(image, 1.0) + rect.min.to_vec2()
    }

    pub fn tap_key(&mut self, key: Key) {
        let down = self.key(key, true);
        let up = self.key(key, false);
        self.frame(vec![down, up]);
    }
}
