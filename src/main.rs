use std::path::PathBuf;

use game_asset_creator::{EditorApp, APP_TITLE};

fn main() -> eframe::Result {
    // Microsoft's ONNX Runtime builds (the one bundled in the app) record and
    // send usage telemetry unless this is set before the library loads. Set
    // here, while the process is still single-threaded.
    if std::env::var_os("ORT_DISABLE_TELEMETRY").is_none() {
        std::env::set_var("ORT_DISABLE_TELEMETRY", "1");
    }
    let args: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
    // `--self-test [video]`: headless check of the bundled libraries, before
    // any window (or the asset library) is opened.
    if let Some(code) = game_asset_creator::selftest::from_args(&args) {
        std::process::exit(code);
    }
    // Any paths on the command line are opened (as documents or videos) at startup.
    let paths: Vec<PathBuf> = args.into_iter().map(PathBuf::from).collect();
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
            // The persistent asset library (app data folder).
            app.open_library(&game_asset_creator::asset_library::AssetLibrary::default_dir());
            app.set_tag_config(game_asset_creator::library::tagger::TagConfig::default());
            for path in &paths {
                app.open_path(path);
            }
            Ok(Box::new(app))
        }),
    )
}
