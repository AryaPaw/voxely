//! An opt-in test tool, never linked into the daily driver. It uses the production
//! window configuration/chrome and React screens, without production bootstrap.
use std::borrow::Cow;
use std::path::{Component, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::{Listener, Manager, WebviewUrl, WebviewWindowBuilder};

#[path = "window_capture.rs"]
mod window_capture;

struct Assets(PathBuf);
impl tauri::Assets<tauri::Wry> for Assets {
    fn get(&self, key: &tauri::utils::assets::AssetKey) -> Option<Cow<'_, [u8]>> {
        let relative = PathBuf::from(key.as_ref().trim_start_matches('/'));
        if !relative
            .components()
            .all(|part| matches!(part, Component::Normal(_)))
        {
            return None;
        }
        std::fs::read(self.0.join(relative)).ok().map(Cow::Owned)
    }
    fn iter(&self) -> Box<tauri::utils::assets::AssetsIter<'_>> {
        Box::new(std::iter::empty())
    }
    fn csp_hashes(
        &self,
        _: &tauri::utils::assets::AssetKey,
    ) -> Box<dyn Iterator<Item = tauri::utils::assets::CspHash<'_>> + '_> {
        Box::new(std::iter::empty())
    }
}

fn readme_ready(
    window: &tauri::WebviewWindow,
    work: &std::path::Path,
    payload: &str,
) -> Result<(), String> {
    if payload.len() > 4096 {
        return Err("Readiness report too large".into());
    }
    let report: serde_json::Value = serde_json::from_str(payload).map_err(|e| e.to_string())?;
    let section = report["section"].as_str().ok_or("Missing section")?;
    let expected_window = if section == "hud" { "overlay" } else { "main" };
    if window.label() != expected_window || !scenes().iter().any(|scene| scene.section == section) {
        return Err("Unexpected screenshot window/section".into());
    }
    let data = serde_json::json!({
        "section": section, "report": report["report"],
        "pid": std::process::id(), "hwnd": window.hwnd().map_err(|e| e.to_string())?.0 as usize,
        "scaleFactor": window.scale_factor().map_err(|e| e.to_string())?,
    });
    let temporary = work.join(format!("{section}.ready.tmp"));
    std::fs::write(
        &temporary,
        serde_json::to_vec(&data).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    std::fs::rename(temporary, work.join(format!("{section}.ready.json")))
        .map_err(|e| e.to_string())
}

#[derive(serde::Deserialize)]
struct Scene {
    section: String,
    route: Option<String>,
}
fn scenes() -> Vec<Scene> {
    serde_json::from_str(include_str!("scenes.json")).expect("Valid native screenshot scenes")
}

#[test]
#[ignore = "native README capture tool; run bun run docs:shots on a Windows desktop"]
fn capture_native_readme() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../.local/readme-shots")
        .canonicalize()
        .expect("Run docs:shots first");
    let work = PathBuf::from(std::env::var_os("VOXELY_SHOTS_WORK").expect("Run docs:shots first"))
        .canonicalize()
        .unwrap();
    assert_eq!(
        work.parent(),
        Some(root.as_path()),
        "Fixture must stay inside its owned work directory"
    );
    assert!(work
        .file_name()
        .unwrap()
        .to_string_lossy()
        .starts_with("run-"));
    assert!(work.join("assets/index.html").is_file());
    let mut scenes = scenes();
    if std::env::var("VOXELY_SHOTS_PILOT").as_deref() == Ok("1") {
        scenes.truncate(1);
    }
    let mut context = tauri::generate_context!();
    context.set_assets(Box::new(Assets(work.join("assets"))));
    context.config_mut().build.dev_url = None;
    context.config_mut().app.security.asset_protocol.enable = false;
    let main_config = context
        .config()
        .app
        .windows
        .iter()
        .find(|w| w.label == "main")
        .unwrap()
        .clone();
    context.config_mut().app.windows.clear();
    context.config_mut().identifier = format!("com.voxely.readme-fixture.p{}", std::process::id());
    context.config_mut().app.app_directories_override = Some(
        tauri::utils::config::AppDirectoriesOverride::Root(work.join("profile")),
    );
    // No production context, plugins, credentials, storage migration, tray,
    // microphone, global hotkeys, updater, autostart or network API handlers.
    let result = Arc::new(Mutex::new(None));
    let worker_result = result.clone();
    let app = tauri::Builder::default()
        .any_thread()
        .setup(move |app| {
            let reports = work.clone();
            let event_handle = app.handle().clone();
            app.listen("readme://ready", move |event| {
                let result = (|| -> Result<(), String> {
                    let payload: serde_json::Value =
                        serde_json::from_str(event.payload()).map_err(|e| e.to_string())?;
                    let label = if payload["section"] == "hud" {
                        "overlay"
                    } else {
                        "main"
                    };
                    let window = event_handle
                        .get_webview_window(label)
                        .ok_or("Missing screenshot window")?;
                    readme_ready(&window, &reports, event.payload())
                })();
                if let Err(error) = result {
                    eprintln!("Readiness report rejected: {error}");
                }
            });
            let window = WebviewWindowBuilder::from_config(app.handle(), &main_config)?
                .data_directory(work.join("webview"))
                .build()?;
            crate::windows_int::main_window::round_corners(&window);
            window.show()?;
            window.set_focus()?;
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                let outcome = (|| -> Result<(), String> {
                    for (index, scene) in scenes.iter().enumerate() {
                        let section = &scene.section;
                        let target = if section == "hud" {
                            let (tx, rx) = std::sync::mpsc::channel();
                            let ui_handle = handle.clone();
                            handle
                                .run_on_main_thread(move || {
                                    let result = super::build_overlay_window(
                                        &ui_handle,
                                        WebviewUrl::App(
                                            "overlay.html?section=hud&overlay=1".into(),
                                        ),
                                    )
                                    .inspect(super::reveal_overlay);
                                    let _ = tx.send(result);
                                })
                                .map_err(|e| e.to_string())?;
                            window.hide().map_err(|e| e.to_string())?;
                            rx.recv_timeout(Duration::from_secs(5))
                                .map_err(|e| e.to_string())?
                                .map_err(|e| e.to_string())?
                        } else {
                            window.clone()
                        };
                        if index > 0 && section != "hud" {
                            let mut url = window.url().map_err(|e| e.to_string())?;
                            let route = scene.route.as_deref().unwrap_or(section);
                            url.set_query(Some(&format!("section={route}&shot={section}")));
                            window.navigate(url).map_err(|e| e.to_string())?;
                        }
                        let ready = work.join(format!("{section}.ready.json"));
                        let deadline = Instant::now() + Duration::from_secs(45);
                        while !ready.is_file() {
                            if Instant::now() > deadline {
                                return Err(format!("Capture timed out: {section}"));
                            }
                            std::thread::sleep(Duration::from_millis(50));
                        }
                        let (width, height) = window_capture::save(
                            &target,
                            &work.join(format!("shot-{section}.png")),
                        )?;
                        let captured = work.join(format!("{section}.captured.tmp"));
                        std::fs::write(
                            &captured,
                            serde_json::json!({"width": width, "height": height}).to_string(),
                        )
                        .map_err(|e| e.to_string())?;
                        std::fs::rename(captured, work.join(format!("{section}.captured.json")))
                            .map_err(|e| e.to_string())?;
                        let done = work.join(format!("{section}.done"));
                        while !done.is_file() {
                            if Instant::now() > deadline {
                                return Err(format!(
                                    "Capture acknowledgement timed out: {section}"
                                ));
                            }
                            std::thread::sleep(Duration::from_millis(50));
                        }
                    }
                    Ok(())
                })();
                *worker_result.lock().unwrap() = Some(outcome);
                handle.exit(0);
            });
            Ok(())
        })
        .build(context)
        .expect("Build isolated native README window");
    assert_eq!(app.run_return(|_, _| {}), 0);
    result
        .lock()
        .unwrap()
        .take()
        .expect("Capture worker finished")
        .unwrap();
}
