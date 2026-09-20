//! Putting a *file* on the Windows clipboard (the same thing Ctrl+C on a
//! file in Explorer does), so it pastes as a file into Explorer, Slack,
//! Discord, email, etc.
//!
//! Video has no "paste the video data" clipboard format the way images do, so
//! copying the file itself is the practical equivalent. `arboard` (used for
//! image copies) doesn't support `CF_HDROP`, hence the raw Win32 calls.

use std::path::Path;
use std::time::Duration;
use windows_sys::Win32::Foundation::{GlobalFree, HANDLE};
use windows_sys::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
};
use windows_sys::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};

/// The predefined clipboard format for a list of files (`CF_HDROP`).
const CF_HDROP: u32 = 15;

/// Size of the `DROPFILES` header: `pFiles: u32`, `pt: POINT` (two `i32`),
/// `fNC: BOOL`, `fWide: BOOL` — five 4-byte fields.
const DROPFILES_HEADER_LEN: usize = 20;

/// Builds the `DROPFILES` payload for a single file: the header (with
/// `fWide` set, since the list is UTF-16) followed by the path and a
/// double-NUL terminator. Kept separate from the clipboard calls so the
/// byte layout is unit-testable.
fn build_dropfiles(path: &Path) -> Vec<u8> {
    let wide: Vec<u16> = path.as_os_str().encode_wide_compat().chain([0u16, 0u16]).collect();

    let mut bytes = Vec::with_capacity(DROPFILES_HEADER_LEN + wide.len() * 2);
    bytes.extend_from_slice(&(DROPFILES_HEADER_LEN as u32).to_le_bytes()); // pFiles: offset of the list
    bytes.extend_from_slice(&0i32.to_le_bytes()); // pt.x
    bytes.extend_from_slice(&0i32.to_le_bytes()); // pt.y
    bytes.extend_from_slice(&0i32.to_le_bytes()); // fNC
    bytes.extend_from_slice(&1i32.to_le_bytes()); // fWide: UTF-16
    for unit in wide {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    bytes
}

/// `OsStr::encode_wide` lives on a Windows-only extension trait; this keeps
/// the import in one place.
trait EncodeWideCompat {
    fn encode_wide_compat(&self) -> std::vec::IntoIter<u16>;
}
impl EncodeWideCompat for std::ffi::OsStr {
    fn encode_wide_compat(&self) -> std::vec::IntoIter<u16> {
        use std::os::windows::ffi::OsStrExt;
        self.encode_wide().collect::<Vec<u16>>().into_iter()
    }
}

/// Replaces the clipboard's contents with `path` as a copyable file.
pub fn copy_file_to_clipboard(path: &Path) -> Result<(), String> {
    if !path.exists() {
        return Err(format!("File no longer exists: {}", path.display()));
    }
    let payload = build_dropfiles(path);

    unsafe {
        // Another process (a clipboard manager, the previous owner finishing
        // up) can briefly hold the clipboard open; retry a few times rather
        // than failing on a transient collision.
        let mut opened = false;
        for _ in 0..10 {
            if OpenClipboard(std::ptr::null_mut()) != 0 {
                opened = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(30));
        }
        if !opened {
            return Err("Couldn't open the clipboard (another program is using it)".to_string());
        }

        let result = (|| {
            if EmptyClipboard() == 0 {
                return Err("Couldn't clear the clipboard".to_string());
            }

            let hmem = GlobalAlloc(GMEM_MOVEABLE, payload.len());
            if hmem.is_null() {
                return Err("Out of memory allocating clipboard data".to_string());
            }
            let ptr = GlobalLock(hmem) as *mut u8;
            if ptr.is_null() {
                GlobalFree(hmem);
                return Err("Couldn't lock clipboard memory".to_string());
            }
            std::ptr::copy_nonoverlapping(payload.as_ptr(), ptr, payload.len());
            GlobalUnlock(hmem);

            // On success the system owns `hmem`; on failure we still do.
            if SetClipboardData(CF_HDROP, hmem as HANDLE).is_null() {
                GlobalFree(hmem);
                return Err("Windows rejected the clipboard data".to_string());
            }
            Ok(())
        })();

        CloseClipboard();
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dropfiles_layout_matches_the_win32_struct() {
        let bytes = build_dropfiles(Path::new(r"C:\a\b.mp4"));

        // pFiles points just past the 20-byte header.
        assert_eq!(u32::from_le_bytes(bytes[0..4].try_into().unwrap()), 20);
        // fWide = 1 (the list is UTF-16).
        assert_eq!(i32::from_le_bytes(bytes[16..20].try_into().unwrap()), 1);

        // The list itself: the path as UTF-16, then a double NUL.
        let units: Vec<u16> = bytes[20..].chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        let expected: Vec<u16> = r"C:\a\b.mp4".encode_utf16().chain([0, 0]).collect();
        assert_eq!(units, expected);
    }

    #[test]
    fn missing_file_is_reported_not_copied() {
        let err = copy_file_to_clipboard(Path::new(r"C:\definitely\not\here.mp4")).unwrap_err();
        assert!(err.contains("no longer exists"));
    }

    /// Touches the real clipboard, so it's opt-in: it overwrites whatever the
    /// user has copied. Run with `cargo test -- --ignored copies_a_real_file`,
    /// then check with `Get-Clipboard -Format FileDropList`.
    #[test]
    #[ignore]
    fn copies_a_real_file_to_the_clipboard() {
        let path = std::env::temp_dir().join("shotgun_clipboard_file_test.mp4");
        std::fs::write(&path, b"not really an mp4").unwrap();
        copy_file_to_clipboard(&path).expect("clipboard copy should succeed");
        println!("copied: {}", path.display());
    }
}
