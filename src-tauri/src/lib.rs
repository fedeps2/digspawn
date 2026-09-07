// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
// Módulos backend (hito 1: solo placeholders, sin lógica).
pub mod java;
pub mod mojang_api;
pub mod paper_api;
pub mod properties;
pub mod server_manager;

#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![greet])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
