// Tray icon: cerrar la ventana con servers corriendo minimiza al tray
// en vez de matar los procesos. Menú mínimo: Mostrar / Salir
// (Salir frena graceful todo y recién ahí cierra).

use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager};

/// Muestra y enfoca la ventana principal (aunque esté escondida o minimizada).
pub fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
}

/// Frena graceful todos los servers corriendo y sale de la app.
/// Se corre en background porque frenar tarda (guarda el mundo).
fn quit_graceful(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        crate::processes::stop_all(&app).await;
        app.exit(0);
    });
}

pub fn build(app: &AppHandle) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "Mostrar Digspawn", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Salir", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &quit])?;
    let icon = tauri::image::Image::from_bytes(include_bytes!("../icons/32x32.png"))?;
    TrayIconBuilder::new()
        .icon(icon)
        .tooltip("Digspawn")
        .menu(&menu)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => show_main(app),
            "quit" => quit_graceful(app.clone()),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, .. } = event {
                show_main(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}
