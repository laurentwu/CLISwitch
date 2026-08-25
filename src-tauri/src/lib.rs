pub mod adapters;
pub mod app_state;
pub mod catalog;
#[cfg(feature = "desktop")]
pub mod commands;
pub mod domain;
pub mod error;
pub mod filesystem;
pub mod persistence;
pub mod process;
pub mod services;

#[cfg(feature = "desktop")]
pub fn run() {
    desktop::run();
}

#[cfg(feature = "desktop")]
mod desktop {
    use std::sync::atomic::Ordering;

    use tauri::{Emitter, Manager};

    pub fn run() {
        let builder = tauri::Builder::default()
            .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.unminimize();
                    let _ = window.set_focus();
                }
            }))
            .plugin(tauri_plugin_dialog::init())
            .plugin(tauri_plugin_opener::init())
            .plugin(tauri_plugin_window_state::Builder::default().build());
        #[cfg(feature = "e2e")]
        let builder = builder
            .plugin(tauri_plugin_wdio::init())
            .plugin(tauri_plugin_wdio_webdriver::init());
        builder
            .invoke_handler(crate::cliswitch_invoke_handler!())
            .setup(|app| {
                let data_root = app.path().app_local_data_dir()?;
                match tauri::async_runtime::block_on(crate::app_state::AppState::initialize(
                    data_root.clone(),
                )) {
                    Ok(state) => {
                        let mut oauth_events = state.oauth.subscribe();
                        let mut apply_events = state.apply.subscribe();
                        let oauth_app = app.handle().clone();
                        tauri::async_runtime::spawn(async move {
                            while let Ok(event) = oauth_events.recv().await {
                                let _ = oauth_app.emit("cliswitch://oauth-progress", event);
                            }
                        });
                        let apply_app = app.handle().clone();
                        tauri::async_runtime::spawn(async move {
                            while let Ok(event) = apply_events.recv().await {
                                let _ = apply_app.emit("cliswitch://apply-progress", event);
                            }
                        });
                        app.manage(state);
                        app.manage(crate::app_state::StartupState::ready(data_root));
                    }
                    Err(error) => {
                        app.manage(crate::app_state::StartupState::diagnostic(
                            data_root,
                            error.code(),
                            error.to_string(),
                        ));
                    }
                }
                Ok(())
            })
            .on_window_event(|window, event| {
                if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                    let Some(state) = window.try_state::<crate::app_state::AppState>() else {
                        return;
                    };
                    if !state.safe_to_exit.load(Ordering::SeqCst) {
                        api.prevent_close();
                        let _ = window.emit(
                            "cliswitch://close-state",
                            serde_json::json!({ "phase": "requested" }),
                        );
                    }
                }
            })
            .run(tauri::generate_context!())
            .expect("failed to run CLISwitch");
    }
}
