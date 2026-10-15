//! Shows a file in the system's file manager, selected where it can be.

use std::path::Path;

/// Starts the file manager on a helper thread; false if it couldn't start.
pub fn reveal(path: &Path) -> bool {
    let path = path.to_path_buf();
    let spawned = std::thread::Builder::new()
        .name("mareader-reveal".into())
        .spawn(move || show(&path));
    spawned.is_ok()
}

#[cfg(target_os = "macos")]
fn show(path: &Path) {
    let _ = std::process::Command::new("open")
        .arg("-R")
        .arg(path)
        .status();
}

#[cfg(windows)]
fn show(path: &Path) {
    use std::os::windows::process::CommandExt;
    // Explorer reads `/select,` and the quoted path as one argument.
    let arg = format!("/select,\"{}\"", path.display());
    let _ = std::process::Command::new("explorer").raw_arg(arg).status();
}

/// The desktop's file manager selects it; else its folder opens.
#[cfg(all(unix, not(target_os = "macos")))]
fn show(path: &Path) {
    let uri = file_uri(path);
    let selected = std::process::Command::new("dbus-send")
        .args([
            "--session",
            "--print-reply",
            "--dest=org.freedesktop.FileManager1",
            "--type=method_call",
            "/org/freedesktop/FileManager1",
            "org.freedesktop.FileManager1.ShowItems",
        ])
        .arg(format!("array:string:{uri}"))
        .arg("string:")
        .output();
    if selected.is_ok_and(|o| o.status.success()) {
        return;
    }
    let folder = path.parent().unwrap_or(path);
    let _ = std::process::Command::new("xdg-open").arg(folder).status();
}

#[cfg(target_arch = "wasm32")]
fn show(_path: &Path) {}

/// `file:///a%20b/c.pdf`: bytes outside the unreserved set escaped.
#[cfg_attr(not(all(unix, not(target_os = "macos"))), allow(dead_code))]
fn file_uri(path: &Path) -> String {
    let mut uri = String::from("file://");
    for byte in path.to_string_lossy().bytes() {
        let plain = byte.is_ascii_alphanumeric() || b"/-_.~".contains(&byte);
        match plain {
            true => uri.push(char::from(byte)),
            false => uri.push_str(&format!("%{byte:02X}")),
        }
    }
    uri
}

#[cfg(test)]
mod tests {
    #[test]
    fn uris_escape_spaces_and_commas() {
        let uri = super::file_uri(std::path::Path::new("/a b/c,d.pdf"));
        assert_eq!(uri, "file:///a%20b/c%2Cd.pdf");
    }
}
