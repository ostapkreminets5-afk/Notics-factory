use tauri::Webview;

const MIN_ZOOM: f64 = 0.5;
const MAX_ZOOM: f64 = 2.0;

#[tauri::command]
pub fn set_ui_zoom(webview: Webview, scale: f64) -> Result<(), String> {
    if !scale.is_finite() {
        return Err("Масштаб має бути числом".to_owned());
    }
    webview
        .set_zoom(scale.clamp(MIN_ZOOM, MAX_ZOOM))
        .map_err(|error| error.to_string())
}
