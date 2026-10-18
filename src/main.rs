use std::path::PathBuf;

use game_asset_creator::{EditorApp, APP_TITLE};

fn main() -> eframe::Result {
    // Any paths on the command line are opened (as documents or videos) at startup.
    let paths: Vec<PathBuf> = std::env::args_os().skip(1).map(PathBuf::from).collect();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(APP_TITLE)
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([640.0, 400.0]),
        event_loop_builder: Some(Box::new(|builder| {
            #[cfg(target_os = "macos")]
            {
                // winit's default app menu binds Cmd+Q to an immediate
                // terminate; without it Cmd+Q reaches the app, which asks
                // about unsaved changes first.
                use winit::platform::macos::EventLoopBuilderExtMacOS;
                builder.with_default_menu(false);
            }
            let _ = builder;
        })),
        ..Default::default()
    };
    eframe::run_native(
        APP_TITLE,
        options,
        Box::new(move |cc| {
            let mut app = EditorApp::new();
            app.set_repaint_context(&cc.egui_ctx);
            for path in &paths {
                app.open_path(path);
            }
            Ok(Box::new(app))
        }),
    )
}
