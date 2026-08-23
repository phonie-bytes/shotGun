use std::env;
use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_WRITE};
use winreg::RegKey;

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const APP_NAME: &str = "shotGun";

/// Checks if shotGun is registered to run on Windows startup
pub fn is_autostart_enabled() -> bool {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    if let Ok(run_key) = hkcu.open_subkey_with_flags(RUN_KEY, KEY_READ) {
        let val: Result<String, _> = run_key.get_value(APP_NAME);
        return val.is_ok();
    }
    false
}

/// Enables or disables shotGun running on Windows startup
pub fn set_autostart(enable: bool) -> Result<(), String> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (run_key, _) = hkcu
        .create_subkey_with_flags(RUN_KEY, KEY_WRITE | KEY_READ)
        .map_err(|e| format!("Failed to access Windows Startup registry: {e}"))?;

    if enable {
        let current_exe = env::current_exe().map_err(|e| format!("Failed to resolve executable path: {e}"))?;
        let exe_str = format!("\"{}\"", current_exe.to_string_lossy());
        run_key
            .set_value(APP_NAME, &exe_str)
            .map_err(|e| format!("Failed to register startup key: {e}"))?;
    } else {
        let _ = run_key.delete_value(APP_NAME);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_autostart_check_does_not_panic() {
        let _ = is_autostart_enabled();
    }
}
