use tray_icon::{
    menu::{CheckMenuItem, Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem, Submenu},
    Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent,
};

pub enum TrayAction {
    ToggleShowWindow,
    TriggerCapture,
    TriggerNewSession,
    /// Switch to the profile with this id.
    SwitchProfile(String),
    ExitApp,
}

/// One row of the tray's "Switch Profile" submenu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrayProfileEntry {
    pub id: String,
    pub label: String,
    pub active: bool,
}

/// The ids of every clickable item in the current menu. Menus are rebuilt
/// whenever the profile list changes, and each rebuild mints fresh ids.
struct MenuIds {
    show: MenuId,
    capture: MenuId,
    new_session: MenuId,
    exit: MenuId,
    /// (menu item id, profile id) for each row of the submenu.
    profiles: Vec<(MenuId, String)>,
}

pub struct TrayHandler {
    tray_icon: TrayIcon,
    ids: MenuIds,
    /// Kept so tests can inspect the menu that's actually installed.
    #[allow(dead_code)]
    menu: Menu,
}

fn build_menu(profiles: &[TrayProfileEntry]) -> Result<(Menu, MenuIds), String> {
    let menu = Menu::new();
    let item_show = MenuItem::new("Show / Hide shotGun", true, None);
    let item_capture = MenuItem::new("📸 Capture Region", true, None);
    let item_new_session = MenuItem::new("🔄 Start New Session", true, None);
    let item_separator = PredefinedMenuItem::separator();
    let item_exit = MenuItem::new("Exit shotGun", true, None);

    let profile_submenu = Submenu::new("📂 Switch Profile", !profiles.is_empty());
    let mut profile_ids = Vec::with_capacity(profiles.len());
    for entry in profiles {
        // A check mark shows which profile is live; clicking any row (even
        // the checked one) just asks the app to switch, and the rebuilt
        // menu then shows the true state.
        let item = CheckMenuItem::new(&entry.label, true, entry.active, None);
        profile_ids.push((item.id().clone(), entry.id.clone()));
        profile_submenu
            .append(&item)
            .map_err(|e| format!("Failed to add profile to tray menu: {e}"))?;
    }

    menu.append_items(&[
        &item_show,
        &item_capture,
        &item_new_session,
        &profile_submenu,
        &item_separator,
        &item_exit,
    ])
    .map_err(|e| format!("Failed to build tray menu: {e}"))?;

    let ids = MenuIds {
        show: item_show.id().clone(),
        capture: item_capture.id().clone(),
        new_session: item_new_session.id().clone(),
        exit: item_exit.id().clone(),
        profiles: profile_ids,
    };
    Ok((menu, ids))
}

impl TrayHandler {
    pub fn new() -> Result<Self, String> {
        let icon = create_app_icon()?;
        let (menu, ids) = build_menu(&[])?;

        let tray_icon = TrayIconBuilder::new()
            .with_menu(Box::new(menu.clone()))
            .with_tooltip("shotGun - Screen & Region Capture (Noerotech)")
            .with_icon(icon)
            .build()
            .map_err(|e| format!("Failed to create tray icon: {e}"))?;

        Ok(Self { tray_icon, ids, menu })
    }

    /// Rebuilds the menu with the current profiles in the "Switch Profile"
    /// submenu. Called when the profile list or the active profile changes.
    pub fn set_profiles(&mut self, profiles: &[TrayProfileEntry]) {
        match build_menu(profiles) {
            Ok((menu, ids)) => {
                self.tray_icon.set_menu(Some(Box::new(menu.clone())));
                self.menu = menu;
                self.ids = ids;
            }
            Err(e) => crate::video::debug_log(&format!("tray: couldn't rebuild profile menu: {e}")),
        }
    }

    /// Maps a clicked menu item to what the app should do about it.
    fn action_for_menu_id(&self, id: &MenuId) -> Option<TrayAction> {
        if *id == self.ids.show {
            Some(TrayAction::ToggleShowWindow)
        } else if *id == self.ids.capture {
            Some(TrayAction::TriggerCapture)
        } else if *id == self.ids.new_session {
            Some(TrayAction::TriggerNewSession)
        } else if *id == self.ids.exit {
            Some(TrayAction::ExitApp)
        } else {
            self.ids
                .profiles
                .iter()
                .find(|(item_id, _)| item_id == id)
                .map(|(_, profile_id)| TrayAction::SwitchProfile(profile_id.clone()))
        }
    }

    pub fn check_events(&self) -> Option<TrayAction> {
        // Handle menu item click events
        if let Ok(event) = MenuEvent::receiver().try_recv() {
            if let Some(action) = self.action_for_menu_id(&event.id) {
                return Some(action);
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

#[cfg(test)]
mod tests {
    use super::*;
    use tray_icon::menu::MenuItemKind;

    fn entries() -> Vec<TrayProfileEntry> {
        vec![
            TrayProfileEntry { id: "default".into(), label: "📸 Default".into(), active: false },
            TrayProfileEntry { id: "demo".into(), label: "🎬 Demo".into(), active: true },
        ]
    }

    /// The submenu (enabled, rows of (text, checked)) actually installed in `menu`.
    fn submenu_rows(menu: &Menu) -> Option<(bool, Vec<(String, bool)>)> {
        for item in menu.items() {
            if let MenuItemKind::Submenu(sub) = item {
                let rows = sub
                    .items()
                    .into_iter()
                    .filter_map(|i| match i {
                        MenuItemKind::Check(c) => Some((c.text(), c.is_checked())),
                        _ => None,
                    })
                    .collect();
                return Some((sub.is_enabled(), rows));
            }
        }
        None
    }

    #[test]
    fn menu_has_a_switch_profile_submenu_with_the_active_one_checked() {
        let (menu, ids) = build_menu(&entries()).unwrap();
        let (enabled, rows) = submenu_rows(&menu).expect("submenu present");
        assert!(enabled);
        assert_eq!(rows, vec![("📸 Default".to_string(), false), ("🎬 Demo".to_string(), true)]);
        assert_eq!(ids.profiles.len(), 2);
        assert_eq!(ids.profiles[1].1, "demo");
    }

    #[test]
    fn submenu_is_disabled_without_profiles() {
        let (menu, _) = build_menu(&[]).unwrap();
        let (enabled, rows) = submenu_rows(&menu).expect("submenu present");
        assert!(!enabled);
        assert!(rows.is_empty());
    }

    #[test]
    fn clicking_a_profile_row_maps_to_its_profile_id_and_rebuilds_get_fresh_ids() {
        let mut handler = TrayHandler::new().expect("tray icon");
        handler.set_profiles(&entries());

        let (demo_item, demo_profile) = handler.ids.profiles[1].clone();
        assert_eq!(demo_profile, "demo");
        match handler.action_for_menu_id(&demo_item) {
            Some(TrayAction::SwitchProfile(id)) => assert_eq!(id, "demo"),
            _ => panic!("profile row should map to SwitchProfile"),
        }
        match handler.action_for_menu_id(&handler.ids.exit.clone()) {
            Some(TrayAction::ExitApp) => {}
            _ => panic!("exit item should still map to ExitApp"),
        }

        // A rebuild replaces the menu; the old ids must no longer trigger anything.
        handler.set_profiles(&entries()[..1]);
        assert!(handler.action_for_menu_id(&demo_item).is_none());
        let (_, rows) = submenu_rows(&handler.menu).unwrap();
        assert_eq!(rows.len(), 1);
    }
}
