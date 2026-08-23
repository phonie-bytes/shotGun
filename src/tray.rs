use tray_icon::{
    menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem},
    Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent,
};

pub enum TrayAction {
    ToggleShowWindow,
    TriggerCapture,
    TriggerNewSession,
    ExitApp,
}

pub struct TrayHandler {
    _tray_icon: TrayIcon,
    menu_show_id: tray_icon::menu::MenuId,
    menu_capture_id: tray_icon::menu::MenuId,
    menu_new_session_id: tray_icon::menu::MenuId,
    menu_exit_id: tray_icon::menu::MenuId,
}

impl TrayHandler {
    pub fn new() -> Result<Self, String> {
        let icon = create_app_icon()?;

        let tray_menu = Menu::new();
        let item_show = MenuItem::new("Show / Hide shotGun", true, None);
        let item_capture = MenuItem::new("📸 Capture Region", true, None);
        let item_new_session = MenuItem::new("🔄 Start New Session", true, None);
        let item_separator = PredefinedMenuItem::separator();
        let item_exit = MenuItem::new("Exit shotGun", true, None);

        let menu_show_id = item_show.id().clone();
        let menu_capture_id = item_capture.id().clone();
        let menu_new_session_id = item_new_session.id().clone();
        let menu_exit_id = item_exit.id().clone();

        tray_menu
            .append_items(&[
                &item_show,
                &item_capture,
                &item_new_session,
                &item_separator,
                &item_exit,
            ])
            .map_err(|e| format!("Failed to build tray menu: {e}"))?;

        let tray_icon = TrayIconBuilder::new()
            .with_menu(Box::new(tray_menu))
            .with_tooltip("shotGun - Screen & Region Capture (Noerotech)")
            .with_icon(icon)
            .build()
            .map_err(|e| format!("Failed to create tray icon: {e}"))?;

        Ok(Self {
            _tray_icon: tray_icon,
            menu_show_id,
            menu_capture_id,
            menu_new_session_id,
            menu_exit_id,
        })
    }

    pub fn check_events(&self) -> Option<TrayAction> {
        // Handle menu item click events
        if let Ok(event) = MenuEvent::receiver().try_recv() {
            if event.id == self.menu_show_id {
                return Some(TrayAction::ToggleShowWindow);
            } else if event.id == self.menu_capture_id {
                return Some(TrayAction::TriggerCapture);
            } else if event.id == self.menu_new_session_id {
                return Some(TrayAction::TriggerNewSession);
            } else if event.id == self.menu_exit_id {
                return Some(TrayAction::ExitApp);
            }
        }

        // Handle tray icon click events (e.g. left click or double click)
        if let Ok(event) = TrayIconEvent::receiver().try_recv() {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                return Some(TrayAction::ToggleShowWindow);
            }
        }

        None
    }
}

fn create_app_icon() -> Result<Icon, String> {
    let width = 32u32;
    let height = 32u32;
    let mut rgba = Vec::with_capacity((width * height * 4) as usize);

    for y in 0..height {
        for x in 0..width {
            let dx = x as f32 - 15.5;
            let dy = y as f32 - 15.5;
            let dist = (dx * dx + dy * dy).sqrt();

            if (dist >= 12.0 && dist <= 14.5) || (dist >= 5.0 && dist <= 7.5) || (dist <= 2.5) {
                // Vibrant Cyan / Aqua target rings
                rgba.extend_from_slice(&[0, 220, 255, 255]);
            } else if dist < 15.0 && ((dx.abs() <= 1.5 && dy.abs() >= 3.0) || (dy.abs() <= 1.5 && dx.abs() >= 3.0)) {
                // Crosshairs
                rgba.extend_from_slice(&[0, 220, 255, 255]);
            } else {
                rgba.extend_from_slice(&[0, 0, 0, 0]);
            }
        }
    }

    Icon::from_rgba(rgba, width, height).map_err(|e| format!("Failed to generate icon RGBA: {e}"))
}
