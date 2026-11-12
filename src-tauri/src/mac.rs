//! FairPlay decryption on Apple silicon Macs (SIP stays enabled).
//! iOS apps installed from the Mac App Store live in /Applications/X.app/Wrapper/X.app;
//! the bundled `fpdecrypt` helper decrypts each Mach-O with mremap_encrypted.

use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

fn wrapped_app(app: &Path) -> Option<PathBuf> {
    fs::read_dir(app.join("Wrapper"))
        .ok()?
        .flatten()
        .map(|e| e.path())
        .find(|p| p.extension().map(|e| e == "app").unwrap_or(false))
}

/// iOS/iPadOS apps installed on this Mac.
pub fn ios_apps() -> Vec<Value> {
    let mut out = Vec::new();
    for dir in ["/Applications", &format!("{}/Applications", std::env::var("HOME").unwrap_or_default())] {
        let Ok(rd) = fs::read_dir(dir) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            let Some(inner) = wrapped_app(&p) else { continue };
            let info: plist::Dictionary = plist::from_file(inner.join("Info.plist")).unwrap_or_default();
            let g = |k: &str| info.get(k).and_then(|v| v.as_string()).unwrap_or("").to_string();
            let meta: plist::Dictionary = plist::from_file(p.join("Wrapper/iTunesMetadata.plist")).unwrap_or_default();
            let name = [g("CFBundleDisplayName"), g("CFBundleName")]
                .into_iter()
                .find(|s| !s.is_empty())
                .unwrap_or_default();
            out.push(json!({
                "path": p.to_string_lossy(),
                "name": name,
                "bundleId": g("CFBundleIdentifier"),
                "version": g("CFBundleShortVersionString"),
                "build": g("CFBundleVersion"),
                "appId": meta.get("itemId").and_then(|v| v.as_signed_integer()),
            }));
        }
    }
    out
}

fn is_macho(p: &Path) -> bool {
    let mut b = [0u8; 4];
    fs::File::open(p).and_then(|mut f| f.read_exact(&mut b)).is_ok()
        && matches!(u32::from_le_bytes(b), 0xfeedfacf | 0xcafebabe | 0xbebafeca | 0xcffaedfe)
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    if let Ok(rd) = fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            let ft = e.file_type();
            if ft.as_ref().map(|t| t.is_symlink()).unwrap_or(false) {
                continue;
            }
            if p.is_dir() {
                walk(&p, out);
            } else {
                out.push(p);
            }
        }
    }
}

/// Decrypt an installed iOS app into an IPA at `out_ipa`. Returns how many binaries were decrypted.
pub fn decrypt(app_path: &Path, out_ipa: &Path, mut log: impl FnMut(String)) -> Result<usize, String> {
    let inner = wrapped_app(app_path).ok_or("Not an iOS app (no Wrapper)")?;
    let helper = std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().and_then(|d| d.parent()).map(|c| c.join("Resources/fpdecrypt")))
        .filter(|p| p.exists())
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("binaries/fpdecrypt"));
    let name = inner.file_name().unwrap().to_string_lossy().to_string();

    let mut files = Vec::new();
    walk(&inner, &mut files);
    let file = fs::File::create(out_ipa).map_err(err)?;
    let mut zip = zip::ZipWriter::new(file);
    let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let tmp = std::env::temp_dir().join(format!("ipacked-fp-{}", std::process::id()));
    fs::create_dir_all(&tmp).map_err(err)?;
    let mut decrypted = 0;

    for f in files {
        let rel = f.strip_prefix(&inner).unwrap().to_string_lossy().to_string();
        let mut data = fs::read(&f).map_err(err)?;
        if is_macho(&f) {
            let out = tmp.join("bin");
            let st = std::process::Command::new(&helper).arg(&f).arg(&out).output().map_err(err)?;
            match st.status.code() {
                Some(0) => {
                    data = fs::read(&out).map_err(err)?;
                    decrypted += 1;
                    log(format!("decrypted: {rel}"));
                }
                Some(2) => {}
                _ => {
                    let _ = fs::remove_dir_all(&tmp);
                    return Err(format!("{rel}: {}", String::from_utf8_lossy(&st.stderr).trim()));
                }
            }
        }
        zip.start_file(format!("Payload/{name}/{rel}"), opts).map_err(err)?;
        zip.write_all(&data).map_err(err)?;
    }
    if let Ok(m) = fs::read(app_path.join("Wrapper/iTunesMetadata.plist")) {
        zip.start_file("iTunesMetadata.plist", opts).map_err(err)?;
        zip.write_all(&m).map_err(err)?;
    }
    zip.finish().map_err(err)?;
    let _ = fs::remove_dir_all(&tmp);
    Ok(decrypted)
}
