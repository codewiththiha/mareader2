//! Puts the icon in the Windows exe, where Explorer and shortcuts look.

// The linker reads the `.res` itself: no resource compiler needed.

use std::error::Error;
use std::path::PathBuf;

const ICON: &str = "icons/icon.ico";
const RT_ICON: u16 = 3;
const RT_GROUP_ICON: u16 = 14;
/// English (United States), as resource compilers default to.
const LANGUAGE: u16 = 0x0409;
const GROUP_ID: u16 = 1;

fn main() -> Result<(), Box<dyn Error>> {
    println!("cargo::rerun-if-changed={ICON}");
    let windows = std::env::var("CARGO_CFG_TARGET_OS").is_ok_and(|os| os == "windows");
    let msvc = std::env::var("CARGO_CFG_TARGET_ENV").is_ok_and(|env| env == "msvc");
    if !(windows && msvc) {
        return Ok(());
    }
    let dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR")?);
    let res = resources(&std::fs::read(dir.join(ICON))?)?;
    let out = PathBuf::from(std::env::var("OUT_DIR")?).join("mareader.res");
    std::fs::write(&out, res)?;
    println!("cargo::rustc-link-arg-bins={}", out.display());
    Ok(())
}

/// A `.res` file holding each image of `ico` and the group naming them.
fn resources(ico: &[u8]) -> Result<Vec<u8>, Box<dyn Error>> {
    let word = |at: usize| -> Result<u16, Box<dyn Error>> {
        let b = ico.get(at..at + 2).ok_or("short .ico")?;
        Ok(u16::from_le_bytes(b.try_into()?))
    };
    let dword = |at: usize| -> Result<u32, Box<dyn Error>> {
        let b = ico.get(at..at + 4).ok_or("short .ico")?;
        Ok(u32::from_le_bytes(b.try_into()?))
    };
    if word(0)? != 0 || word(2)? != 1 {
        return Err("not an .ico".into());
    }
    let count = word(4)?;
    // A `.res` starts with an empty entry that marks its format.
    let mut res = entry(0, 0, &[], 0);
    let mut group = [0, 1, count].map(u16::to_le_bytes).concat();
    for i in 0..count {
        let at = 6 + 16 * usize::from(i);
        let head = ico.get(at..at + 12).ok_or("short .ico")?;
        let (size, offset) = (dword(at + 8)?, dword(at + 12)?);
        let start = usize::try_from(offset)?;
        let image = ico
            .get(start..start + usize::try_from(size)?)
            .ok_or("an image runs past the .ico")?;
        let id = i + 1;
        // The .ico's entry, with the image's resource id for its offset.
        group.extend_from_slice(head);
        group.extend_from_slice(&id.to_le_bytes());
        res.extend(entry(RT_ICON, id, image, 0x1010));
    }
    res.extend(entry(RT_GROUP_ICON, GROUP_ID, &group, 0x1030));
    Ok(res)
}

/// One resource: a header numbering its type and id, then its data.
fn entry(kind: u16, id: u16, data: &[u8], flags: u16) -> Vec<u8> {
    let language = if kind == 0 { 0 } else { LANGUAGE };
    let size = u32::try_from(data.len()).unwrap_or(u32::MAX);
    let mut out = Vec::with_capacity(32 + data.len() + 3);
    out.extend_from_slice(&size.to_le_bytes());
    out.extend_from_slice(&32u32.to_le_bytes());
    for half in [0xFFFF, kind, 0xFFFF, id] {
        out.extend_from_slice(&u16::to_le_bytes(half));
    }
    // Data version, flags, language, version and characteristics.
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&flags.to_le_bytes());
    out.extend_from_slice(&language.to_le_bytes());
    out.extend_from_slice(&[0; 8]);
    out.extend_from_slice(data);
    out.resize(out.len().next_multiple_of(4), 0);
    out
}
