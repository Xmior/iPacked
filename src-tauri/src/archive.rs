//! Local archive of IPA files and the metadata extracted from them.

use std::collections::HashMap;
use std::fs;
use std::io::{Read, Seek};
use std::path::{Path, PathBuf};

use goblin::mach::{Mach, MachO, SingleArch, load_command::CommandVariant};
use md5::Md5;
use plist::{Dictionary, Value};
use serde::{Deserialize, Serialize};
use sha1::Sha1;
use sha2::{Digest, Sha256};
use zip::ZipArchive;

use crate::cgbi;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveEntry {
    pub id: String,
    pub app_id: Option<i64>,
    pub bundle_id: String,
    pub name: String,
    pub version: String,
    pub build: String,
    pub external_version_id: Option<String>,
    /// Original release date of this version (ISO 8601) when known.
    pub release_date: Option<String>,
    pub min_ios: Option<String>,
    pub architectures: Vec<String>,
    pub device_family: Vec<String>,
    /// `Some(true)` if the main binary is still FairPlay encrypted.
    pub encrypted: Option<bool>,
    pub file_path: String,
    pub file_size: u64,
    pub md5: String,
    pub sha1: String,
    pub sha256: String,
    pub icon_path: Option<String>,
    /// Where the file came from, e.g. "App Store (ipatool) · user@example.com".
    pub source: String,
    /// When it was added to the archive (ISO 8601).
    pub archived_at: String,
    /// Apple ID that bought it (from iTunesMetadata.plist), if any.
    #[serde(default)]
    pub apple_id: Option<String>,
    /// File mtime (unix secs) when last analysed; lets a rescan skip unchanged files.
    #[serde(default)]
    pub mtime: i64,
    #[serde(default)]
    pub scan_v: u32,
    /// Build date of this version (zip timestamp of the main binary). Store
    /// metadata often carries the app's *first* release date instead.
    #[serde(default)]
    pub build_date: Option<String>,
    /// sha256 of the FairPlay original this decrypted IPA was dumped from.
    #[serde(default)]
    pub dumped_from: Option<String>,
    /// Name under the icon on the device (CFBundleDisplayName / CFBundleName).
    #[serde(default)]
    pub app_name: Option<String>,
    /// App Store name (iTunesMetadata itemName) when known.
    #[serde(default)]
    pub store_name: Option<String>,
    /// ios / tvos / watchos / visionos / imessage / stickers / macos (None = not checked yet).
    #[serde(default)]
    pub platform: Option<String>,
    /// iOS app that carries an Apple Watch app inside (Watch/ folder).
    #[serde(default)]
    pub has_watch: bool,
}

const SCAN_V: u32 = 2;

pub fn root() -> PathBuf {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| ".".into());
    let p = PathBuf::from(home).join("Documents").join("iPacked");
    fs::create_dir_all(p.join("icons")).ok();
    p
}

fn index_path() -> PathBuf {
    root().join("index.json")
}

/// Source labels written by older (Turkish) versions → English.
fn english_source(s: &str) -> String {
    const MAP: [(&str, &str); 6] = [
        ("Cihaz dökümü (Clutch, şifresiz)", "Device dump (Clutch, decrypted)"),
        ("Cihaz dökümü (bagbak/frida, şifresiz)", "Device dump (bagbak/frida, decrypted)"),
        ("Cihaz arşivi (ideviceinstaller)", "Device archive (ideviceinstaller)"),
        ("Yerel dosya (içe aktarıldı)", "Local file (imported)"),
        ("Mac FairPlay çözümü (mremap_encrypted)", "Mac FairPlay decryption (mremap_encrypted)"),
        ("Elle eklendi", "Added manually"),
    ];
    let mut out = s.to_string();
    for (a, b) in MAP {
        out = out.replace(a, b);
    }
    out
}

pub fn load() -> Vec<ArchiveEntry> {
    let mut v: Vec<ArchiveEntry> = fs::read(index_path())
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default();
    for e in v.iter_mut() {
        e.source = english_source(&e.source);
    }
    v
}

pub fn save(entries: &[ArchiveEntry]) -> Result<(), String> {
    let data = serde_json::to_vec_pretty(entries).map_err(|e| e.to_string())?;
    fs::write(index_path(), data).map_err(|e| e.to_string())
}

/// Insert or replace (same sha256) an entry.
pub fn upsert(entry: ArchiveEntry) -> Result<ArchiveEntry, String> {
    let mut all = load();
    all.retain(|e| e.sha256 != entry.sha256);
    all.push(entry.clone());
    save(&all)?;
    Ok(entry)
}

pub fn remove(id: &str, delete_file: bool) -> Result<(), String> {
    let mut all = load();
    if let Some(e) = all.iter().find(|e| e.id == id) {
        if delete_file {
            let _ = fs::remove_file(&e.file_path);
            if let Some(i) = &e.icon_path {
                let _ = fs::remove_file(i);
            }
        }
    }
    all.retain(|e| e.id != id);
    save(&all)
}

fn hashes(path: &Path) -> Result<(u64, String, String, String), String> {
    let mut f = fs::File::open(path).map_err(|e| e.to_string())?;
    let (mut m, mut s1, mut s2) = (Md5::new(), Sha1::new(), Sha256::new());
    let mut buf = vec![0u8; 1 << 20];
    let mut total = 0u64;
    loop {
        let n = f.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        total += n as u64;
        m.update(&buf[..n]);
        s1.update(&buf[..n]);
        s2.update(&buf[..n]);
    }
    Ok((
        total,
        hex::encode(m.finalize()),
        hex::encode(s1.finalize()),
        hex::encode(s2.finalize()),
    ))
}

fn read_entry<R: Read + Seek>(zip: &mut ZipArchive<R>, name: &str) -> Option<Vec<u8>> {
    let mut f = zip.by_name(name).ok()?;
    let mut v = Vec::with_capacity(f.size() as usize);
    f.read_to_end(&mut v).ok()?;
    Some(v)
}

fn s(d: &Dictionary, k: &str) -> Option<String> {
    d.get(k).and_then(Value::as_string).map(str::to_string)
}

fn cpu_name(cputype: u32, sub: u32) -> String {
    use goblin::mach::constants::cputype::*;
    match (cputype, sub & 0x00ff_ffff) {
        (CPU_TYPE_ARM64, 2) => "arm64e".into(),
        (CPU_TYPE_ARM64, _) => "arm64".into(),
        (CPU_TYPE_ARM64_32, _) => "arm64_32".into(),
        (CPU_TYPE_ARM, 11) => "armv7s".into(),
        (CPU_TYPE_ARM, 12) => "armv7k".into(),
        (CPU_TYPE_ARM, 9) => "armv7".into(),
        (CPU_TYPE_ARM, 6) => "armv6".into(),
        (CPU_TYPE_ARM, _) => "arm".into(),
        (CPU_TYPE_X86_64, _) => "x86_64".into(),
        (CPU_TYPE_X86, _) => "i386".into(),
        (t, s) => format!("cpu{t}/{s}"),
    }
}

fn macho_info(m: &MachO, archs: &mut Vec<String>, enc: &mut Option<bool>) {
    archs.push(cpu_name(m.header.cputype, m.header.cpusubtype));
    for lc in &m.load_commands {
        let id = match lc.command {
            CommandVariant::EncryptionInfo32(c) => Some(c.cryptid),
            CommandVariant::EncryptionInfo64(c) => Some(c.cryptid),
            _ => None,
        };
        if let Some(id) = id {
            *enc = Some(enc.unwrap_or(false) || id != 0);
        }
    }
}

fn binary_info(bytes: &[u8]) -> (Vec<String>, Option<bool>) {
    let mut archs = Vec::new();
    let mut enc = None;
    match Mach::parse(bytes) {
        Ok(Mach::Binary(m)) => macho_info(&m, &mut archs, &mut enc),
        Ok(Mach::Fat(fat)) => {
            for a in fat.into_iter().flatten() {
                if let SingleArch::MachO(m) = a {
                    macho_info(&m, &mut archs, &mut enc);
                }
            }
        }
        Err(_) => {}
    }
    (archs, enc)
}

fn icon_names(info: &Dictionary) -> Vec<String> {
    let mut names = Vec::new();
    let mut push_files = |d: Option<&Value>| {
        if let Some(arr) = d
            .and_then(Value::as_dictionary)
            .and_then(|d| d.get("CFBundlePrimaryIcon"))
            .and_then(Value::as_dictionary)
            .and_then(|d| d.get("CFBundleIconFiles"))
            .and_then(Value::as_array)
        {
            names.extend(arr.iter().filter_map(Value::as_string).map(str::to_string));
        }
    };
    push_files(info.get("CFBundleIcons"));
    push_files(info.get("CFBundleIcons~ipad"));
    if let Some(arr) = info.get("CFBundleIconFiles").and_then(Value::as_array) {
        names.extend(arr.iter().filter_map(Value::as_string).map(str::to_string));
    }
    if let Some(n) = s(info, "CFBundleIconFile") {
        names.push(n);
    }
    names
        .into_iter()
        .map(|n| n.trim_end_matches(".png").to_string())
        .collect()
}

/// Analyse an IPA and build an archive entry (without moving the file).
pub fn analyze(path: &Path, source: &str) -> Result<ArchiveEntry, String> {
    let file = fs::File::open(path).map_err(|e| e.to_string())?;
    let mut zip = ZipArchive::new(file).map_err(|e| format!("Invalid IPA: {e}"))?;
    let names: Vec<String> = (0..zip.len())
        .filter_map(|i| zip.by_index(i).ok().map(|f| f.name().to_string()))
        .collect();

    let info_name = names
        .iter()
        .find(|n| {
            n.starts_with("Payload/")
                && n.ends_with(".app/Info.plist")
                && n.matches('/').count() == 2
        })
        .cloned()
        .ok_or("Info.plist not found")?;
    let app_dir = info_name.trim_end_matches("Info.plist").to_string();
    let info: Dictionary = plist::from_bytes(&read_entry(&mut zip, &info_name).unwrap_or_default())
        .map_err(|e| format!("could not read Info.plist: {e}"))?;

    let itunes: Option<Dictionary> = read_entry(&mut zip, "iTunesMetadata.plist")
        .and_then(|b| plist::from_bytes(&b).ok());

    let build_date = s(&info, "CFBundleExecutable").and_then(|exe| zip_date(&mut zip, &format!("{app_dir}{exe}")));
    let (archs, encrypted) = s(&info, "CFBundleExecutable")
        .and_then(|exe| read_entry(&mut zip, &format!("{app_dir}{exe}")))
        .map(|b| binary_info(&b))
        .unwrap_or_default();

    let device_family = info
        .get("UIDeviceFamily")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_unsigned_integer())
                .map(|n| match n {
                    1 => "iPhone".to_string(),
                    2 => "iPad".to_string(),
                    3 => "TV".to_string(),
                    4 => "Watch".to_string(),
                    n => format!("#{n}"),
                })
                .collect()
        })
        .unwrap_or_default();

    let (file_size, md5, sha1, sha256) = hashes(path)?;
    let bundle_id = s(&info, "CFBundleIdentifier").unwrap_or_default();
    let version = s(&info, "CFBundleShortVersionString").unwrap_or_default();
    let build = s(&info, "CFBundleVersion").unwrap_or_default();

    // Icon: largest PNG matching any declared icon name.
    let mut best: Option<(u64, String)> = None;
    for base in icon_names(&info) {
        let prefix = format!("{app_dir}{base}");
        for n in names.iter().filter(|n| n.starts_with(&prefix) && n.ends_with(".png")) {
            let size = zip.by_name(n).map(|f| f.size()).unwrap_or(0);
            if best.as_ref().map(|b| size > b.0).unwrap_or(true) {
                best = Some((size, n.clone()));
            }
        }
    }
    if best.is_none() {
        for n in names.iter().filter(|n| {
            n.starts_with(&app_dir) && !n[app_dir.len()..].contains('/') && n.to_lowercase().ends_with(".png") && n.to_lowercase().contains("icon")
        }) {
            let size = zip.by_name(n).map(|f| f.size()).unwrap_or(0);
            if best.as_ref().map(|b| size > b.0).unwrap_or(true) {
                best = Some((size, n.clone()));
            }
        }
    }
    let (platform, has_watch) = platform_of(&info, &names, &app_dir);
    let mut icon_path = None;
    if platform == "ios" || platform == "imessage" || platform == "stickers" {
        icon_path = best.and_then(|(_, n)| read_entry(&mut zip, &n)).and_then(|png| {
            let p = root().join("icons").join(format!("{sha256}.png"));
            fs::write(&p, cgbi::normalize(png)).ok()?;
            Some(p.to_string_lossy().to_string())
        });
    }
    if icon_path.is_none() {
        // tvOS / watchOS / visionOS icons live in Assets.car and have their own
        // shapes: let macOS render the bundle icon.
        let p = root().join("icons").join(format!("{sha256}.png"));
        if render_bundle_icon(&mut zip, &names, &app_dir, &p) {
            icon_path = Some(p.to_string_lossy().to_string());
        }
    }

    let itunes_s = |k: &str| itunes.as_ref().and_then(|d| s(d, k));
    let ext_id = itunes
        .as_ref()
        .and_then(|d| d.get("softwareVersionExternalIdentifier"))
        .and_then(|v| v.as_unsigned_integer())
        .map(|n| n.to_string());
    let app_id = itunes
        .as_ref()
        .and_then(|d| d.get("itemId"))
        .and_then(|v| v.as_signed_integer());
    let apple_id = itunes.as_ref().and_then(apple_id_of);

    Ok(ArchiveEntry {
        id: sha256[..16].to_string(),
        app_id,
        name: itunes_s("itemName")
            .or_else(|| s(&info, "CFBundleDisplayName"))
            .or_else(|| s(&info, "CFBundleName"))
            .unwrap_or_else(|| bundle_id.clone()),
        bundle_id,
        version,
        build,
        external_version_id: ext_id,
        release_date: itunes_s("releaseDate"),
        min_ios: s(&info, "MinimumOSVersion"),
        architectures: archs,
        device_family,
        encrypted,
        file_path: path.to_string_lossy().to_string(),
        file_size,
        md5,
        sha1,
        sha256,
        icon_path,
        source: source.to_string(),
        archived_at: chrono::Utc::now().to_rfc3339(),
        apple_id,
        mtime: mtime_of(path),
        platform: Some(platform),
        has_watch,
        scan_v: SCAN_V,
        build_date,
        dumped_from: None,
        app_name: s(&info, "CFBundleDisplayName").or_else(|| s(&info, "CFBundleName")).filter(|x| !x.is_empty()),
        store_name: itunes_s("itemName"),
    })
}

fn zip_date<R: Read + Seek>(zip: &mut ZipArchive<R>, name: &str) -> Option<String> {
    let d = zip.by_name(name).ok()?.last_modified()?;
    if d.year() < 2007 {
        return None;
    }
    Some(format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z", d.year(), d.month(), d.day(), d.hour(), d.minute(), d.second()))
}

/// Cheap build-date read for entries analysed before the field existed.
pub fn build_date_of(path: &Path) -> Option<String> {
    let mut zip = ZipArchive::new(fs::File::open(path).ok()?).ok()?;
    let names: Vec<String> = (0..zip.len()).filter_map(|i| zip.by_index(i).ok().map(|f| f.name().to_string())).collect();
    let info_name = names.iter().find(|n| n.starts_with("Payload/") && n.ends_with(".app/Info.plist") && n.matches('/').count() == 2)?.clone();
    let app_dir = info_name.trim_end_matches("Info.plist").to_string();
    let info: Dictionary = plist::from_bytes(&read_entry(&mut zip, &info_name)?).ok()?;
    let exe = s(&info, "CFBundleExecutable")?;
    zip_date(&mut zip, &format!("{app_dir}{exe}"))
}

// ---------- user preferences (Application Support/iPacked/prefs.json) ----------
pub const DEFAULT_TEMPLATE: &str = "<Appname> v<version> (<BuildNo>) (<ExternalVersionID>)";

fn prefs_path() -> PathBuf {
    let d = PathBuf::from(std::env::var("HOME").unwrap_or_default()).join("Library/Application Support/iPacked");
    fs::create_dir_all(&d).ok();
    d.join("prefs.json")
}

pub fn prefs() -> serde_json::Value {
    let mut v: serde_json::Value = fs::read(prefs_path()).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or(serde_json::json!({}));
    let defaults = serde_json::json!({
        "nameTemplate": DEFAULT_TEMPLATE,
        "deleteOriginalAfterDump": false,
        "replaceDumpedWithOriginal": false,
        "autoVersionHistory": false,
        "removeAfterDump": true,
        "iaUrl": "",
    });
    for (k, d) in defaults.as_object().unwrap() {
        if v.get(k).is_none() {
            v[k] = d.clone();
        }
    }
    v
}

pub fn prefs_save(v: &serde_json::Value) -> Result<(), String> {
    fs::write(prefs_path(), serde_json::to_vec_pretty(v).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}

/// File name from the user's template: <name> <version> <BuildNo> <BundleID>
/// <ExternalVersionID>. Empty tokens drop their brackets/separators.
pub fn template_name(tpl: &str, e: &ArchiveEntry) -> String {
    let tpl = if tpl.trim().is_empty() { DEFAULT_TEMPLATE } else { tpl };
    let ext_id = e.external_version_id.clone().unwrap_or_default();
    let mut out = tpl.to_string();
    let store = e.store_name.clone().unwrap_or_else(|| e.name.clone());
    let appn = e.app_name.clone().unwrap_or_else(|| e.name.clone());
    for (k, v) in [
        ("<Appname>", appn.as_str()),
        ("<AppName>", appn.as_str()),
        ("<name>", store.as_str()),
        ("<version>", e.version.as_str()),
        ("<BuildNo>", e.build.as_str()),
        ("<BundleID>", e.bundle_id.as_str()),
        ("<ExternalVersionID>", ext_id.as_str()),
    ] {
        out = out.replace(k, v);
        out = out.replace(&k.to_lowercase(), v);
    }
    // Tidy what empty tokens leave behind.
    for junk in ["()", "[]", "{}", "( )"] {
        out = out.replace(junk, "");
    }
    while out.contains("__") { out = out.replace("__", "_"); }
    while out.contains("  ") { out = out.replace("  ", " "); }
    let out: String = out
        .chars()
        .map(|c| if "/\\:*?\"<>|".contains(c) || c.is_control() { '_' } else { c })
        .collect();
    let out = out.trim_matches(|c: char| c == '_' || c == ' ' || c == '-' || c == '.').to_string();
    if out.is_empty() { safe(&e.bundle_id) } else { out }
}

/// Where an entry's file should live: `<ipa|pkg>/<bundle>/<template>[.decrypted].<ext>`,
/// with " (2)" etc. on clashes with another file.
pub fn named_dest(e: &ArchiveEntry, ext: &str) -> PathBuf {
    let dir = root().join(ext).join(safe(&e.bundle_id));
    fs::create_dir_all(&dir).ok();
    let tpl = prefs()["nameTemplate"].as_str().unwrap_or(DEFAULT_TEMPLATE).to_string();
    let mut base = template_name(&tpl, e);
    if e.dumped_from.is_some() || (e.encrypted == Some(false) && ext == "ipa" && !tpl.contains("decrypted")) {
        base.push_str(".decrypted");
    }
    let mut p = dir.join(format!("{base}.{ext}"));
    let mut n = 2;
    while p.exists() && p.to_string_lossy() != e.file_path {
        p = dir.join(format!("{base} ({n}).{ext}"));
        n += 1;
    }
    p
}

/// Rename every archived file to the current template. Returns how many moved.
pub fn rename_all() -> Result<usize, String> {
    let mut list = load();
    let mut moved = 0;
    for e in list.iter_mut() {
        let cur = PathBuf::from(&e.file_path);
        if !cur.exists() {
            continue;
        }
        let ext = if e.file_path.to_lowercase().ends_with(".pkg") { "pkg" } else { "ipa" };
        let dest = named_dest(e, ext);
        if dest != cur && fs::rename(&cur, &dest).is_ok() {
            e.file_path = dest.to_string_lossy().to_string();
            e.mtime = mtime_of(&dest);
            moved += 1;
        }
    }
    save(&list)?;
    Ok(moved)
}

/// Purchaser's Apple ID inside an iTunesMetadata dictionary.
pub fn apple_id_of(d: &Dictionary) -> Option<String> {
    s(d, "appleId")
        .or_else(|| s(d, "apple-id"))
        .or_else(|| {
            d.get("com.apple.iTunesStore.downloadInfo")
                .and_then(Value::as_dictionary)
                .and_then(|x| x.get("accountInfo"))
                .and_then(Value::as_dictionary)
                .and_then(|a| s(a, "AppleID"))
        })
        .or_else(|| s(d, "userName").filter(|u| u.contains('@')))
        .filter(|x| !x.is_empty())
}

fn mtime_of(p: &Path) -> i64 {
    fs::metadata(p)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Mac App Store download (.pkg, xar): no Info.plist to read without
/// expanding, so the entry is built from the store metadata the caller has.
pub fn analyze_pkg(path: &Path, source: &str, bundle_id: &str, name: &str, version: &str) -> Result<ArchiveEntry, String> {
    let (file_size, md5, sha1, sha256) = hashes(path)?;
    let mut e = ArchiveEntry {
        id: sha256[..16].to_string(),
        bundle_id: bundle_id.to_string(),
        name: name.to_string(),
        version: version.to_string(),
        build: version.to_string(),
        device_family: vec!["Mac".into()],
        platform: Some("macos".into()),
        file_path: path.to_string_lossy().to_string(),
        file_size,
        md5,
        sha1,
        sha256,
        source: source.to_string(),
        archived_at: chrono::Utc::now().to_rfc3339(),
        mtime: mtime_of(path),
        scan_v: SCAN_V,
        ..Default::default()
    };
    // Read the app inside (bundle id, minimum macOS, architectures, icon);
    // skipped for very large packages to keep downloads fast.
    if file_size < 3_000_000_000 {
        if let Ok(m) = mac_pkg_info(path) {
            apply_mac_info(&mut e, &m);
            if let Some(png) = m.icon_png {
                let p = root().join("icons").join(format!("{}.png", e.sha256));
                if fs::write(&p, png).is_ok() {
                    e.icon_path = Some(p.to_string_lossy().to_string());
                }
            }
        }
    }
    Ok(e)
}

fn safe(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_alphanumeric() || ".-_".contains(c) { c } else { '_' })
        .collect()
}

/// Destination inside the archive: `ipa/<bundle>/<ver>_<id>.ipa`, Mac apps
/// under `pkg/<bundle>/…pkg`.
pub fn dest_for(bundle_id: &str, version: &str, ext_id: &str) -> PathBuf {
    dest_ext(bundle_id, version, ext_id, "ipa")
}

pub fn dest_ext(bundle_id: &str, version: &str, ext_id: &str, ext: &str) -> PathBuf {
    let dir = root().join(ext).join(safe(bundle_id));
    fs::create_dir_all(&dir).ok();
    dir.join(format!("{}_{}.{ext}", safe(version), safe(ext_id)))
}

const RESERVED: &[&str] = &["ipa", "pkg", "icons", "Blobs"];

/// Older layouts kept `<bundle>/` folders right in the archive root: move
/// their files under `ipa/` or `pkg/`.
fn migrate() {
    let r = root();
    for d in fs::read_dir(&r).into_iter().flatten().flatten() {
        let name = d.file_name().to_string_lossy().to_string();
        if name.starts_with('.') || RESERVED.contains(&name.as_str()) || !d.path().is_dir() {
            continue;
        }
        for f in fs::read_dir(d.path()).into_iter().flatten().flatten() {
            let p = f.path();
            let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
            if ext == "ipa" || ext == "pkg" {
                let to_dir = r.join(&ext).join(&name);
                let _ = fs::create_dir_all(&to_dir);
                let _ = fs::rename(&p, to_dir.join(f.file_name()));
            }
        }
        // Drop the old folder once only Finder's .DS_Store is left in it.
        let left: Vec<_> = fs::read_dir(d.path()).into_iter().flatten().flatten().map(|f| f.file_name()).collect();
        if left.iter().all(|n| n == ".DS_Store") {
            let _ = fs::remove_file(d.path().join(".DS_Store"));
            let _ = fs::remove_dir(d.path());
        }
    }
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        if p.is_dir() {
            walk(&p, out);
        } else if matches!(p.extension().and_then(|e| e.to_str()).map(|e| e.to_lowercase()).as_deref(), Some("ipa") | Some("pkg")) {
            out.push(p);
        }
    }
}

#[derive(Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ScanReport {
    pub added: usize,
    pub missing: usize,
    pub duplicates: Vec<String>,
}

/// Re-read the archive folder: files the user deleted drop out, new or
/// changed files get analysed straight from the .ipa, extra info that only
/// came from the store (version id, release date, source) is kept in a
/// sha256-keyed cache so it survives a file being removed and put back.
/// Byte-identical copies (same size + sha256) are deleted, keeping the
/// oldest, and logged in `deleted.log`.
pub fn rescan() -> ScanReport {
    migrate();
    let r = root();
    let old = load();
    let mut rep = ScanReport::default();
    // Remember store-only info for every entry we know.
    for e in &old {
        cache_set(&format!("extra:{}", e.sha256), serde_json::json!({
            "appId": e.app_id, "externalVersionId": e.external_version_id, "releaseDate": e.release_date,
            "source": e.source, "archivedAt": e.archived_at, "name": e.name, "version": e.version,
            "dumpedFrom": e.dumped_from, "appleId": e.apple_id, "storeName": e.store_name,
        }));
    }
    let mut files = Vec::new();
    walk(&r.join("ipa"), &mut files);
    walk(&r.join("pkg"), &mut files);
    let mut out: Vec<ArchiveEntry> = Vec::new();
    for p in files {
        let ps = p.to_string_lossy().to_string();
        let size = fs::metadata(&p).map(|m| m.len()).unwrap_or(0);
        let mt = mtime_of(&p);
        if let Some(e) = old.iter().find(|e| e.file_path == ps && e.file_size == size && (e.mtime == 0 || e.mtime == mt) && e.scan_v >= SCAN_V) {
            let mut e = e.clone();
            platform_upgrade(&mut e);
            if e.build_date.is_none() && !ps.to_lowercase().ends_with(".pkg") {
                e.build_date = build_date_of(&p);
            }
            out.push(e);
            continue;
        }
        let is_pkg = ps.to_lowercase().ends_with(".pkg");
        let fresh = if is_pkg {
            let bundle = p.parent().and_then(|d| d.file_name()).map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            let ver = p.file_stem().map(|n| n.to_string_lossy().split('_').next().unwrap_or("").to_string()).unwrap_or_default();
            analyze_pkg(&p, "Archive folder", &bundle, &bundle, &ver)
        } else {
            analyze(&p, "Archive folder")
        };
        let Ok(mut e) = fresh else { continue };
        let prev = old.iter().find(|o| o.sha256 == e.sha256).cloned();
        let extra = cache_get(&format!("extra:{}", e.sha256));
        if let Some(o) = &prev {
            e.id = o.id.clone();
        } else {
            rep.added += 1;
        }
        if let Some(x) = extra {
            let st = |k: &str| x[k].as_str().map(String::from);
            e.app_id = e.app_id.or(x["appId"].as_i64());
            e.external_version_id = e.external_version_id.or(st("externalVersionId"));
            e.release_date = e.release_date.or(st("releaseDate"));
            e.dumped_from = e.dumped_from.or(st("dumpedFrom"));
            e.store_name = e.store_name.or(st("storeName"));
            e.apple_id = e.apple_id.or(st("appleId"));
            if let Some(v) = st("source") { e.source = v; }
            if let Some(v) = st("archivedAt") { e.archived_at = v; }
            if is_pkg {
                if let Some(v) = st("name") { e.name = v; }
            }
            if is_pkg || e.version.is_empty() {
                if let Some(v) = st("version").filter(|v| !v.is_empty()) {
                    if e.build.is_empty() || is_pkg { e.build = v.clone(); }
                    e.version = v;
                }
            }
        }
        out.push(e);
    }
    rep.missing = old.iter().filter(|o| !out.iter().any(|e| e.sha256 == o.sha256)).count();
    // Duplicates: identical bytes → keep the oldest.
    out.sort_by(|a, b| a.archived_at.cmp(&b.archived_at));
    let mut seen: HashMap<(u64, String), String> = HashMap::new();
    let mut keep = Vec::new();
    for e in out {
        let k = (e.file_size, e.sha256.clone());
        if let Some(first) = seen.get(&k) {
            if fs::remove_file(&e.file_path).is_ok() {
                let line = format!(
                    "{}\t{} {}\t{}\t(same as: {})\tsha256 {}\n",
                    chrono::Local::now().format("%Y-%m-%d %H:%M:%S"),
                    e.name, e.version, e.file_path, first, e.sha256
                );
                use std::io::Write;
                if let Ok(mut f) = fs::OpenOptions::new().create(true).append(true).open(r.join("deleted.log")) {
                    let _ = f.write_all(line.as_bytes());
                }
                if let Some(d) = Path::new(&e.file_path).parent() {
                    let _ = fs::remove_dir(d);
                }
                rep.duplicates.push(format!("{} {}", e.name, e.version));
            }
            continue;
        }
        seen.insert(k, e.file_path.clone());
        keep.push(e);
    }
    let _ = save(&keep);
    rep
}

/// Small JSON key/value cache (version metadata etc.).
/// Metadata cache (cache.json). Kept in memory once loaded; writes are batched
/// (flushed at most every 3 s from a background thread) instead of re-reading
/// and rewriting the whole file on every call.
struct Cache {
    map: HashMap<String, serde_json::Value>,
    dirty: bool,
}

fn cache() -> &'static std::sync::Mutex<Cache> {
    static C: std::sync::OnceLock<std::sync::Mutex<Cache>> = std::sync::OnceLock::new();
    C.get_or_init(|| {
        // Paths saved before the rename (…/Documents/iPast/…) → current folder.
        let old = format!("/Documents/{}/", ["i", "Past"].concat());
        let mut map: HashMap<String, serde_json::Value> = fs::read_to_string(root().join("cache.json"))
            .ok()
            .map(|t| t.replace(&old, "/Documents/iPacked/"))
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default();
        // Drop entries from older formats that nothing reads any more.
        const OLD: [&str; 4] = ["lookup2:", "ipsw3:", "meta:", "appledb:iOS"];
        map.retain(|k, _| !OLD.iter().any(|p| k.starts_with(p)) && !(k.starts_with("appledb:") && k != "appledb:index"));
        std::thread::spawn(|| loop {
            std::thread::sleep(std::time::Duration::from_secs(3));
            cache_flush();
        });
        std::sync::Mutex::new(Cache { map, dirty: true })
    })
}

pub fn cache_flush() {
    let bytes = {
        let mut c = match cache().lock() {
            Ok(c) => c,
            Err(_) => return,
        };
        if !c.dirty {
            return;
        }
        c.dirty = false;
        serde_json::to_vec(&c.map).ok()
    };
    if let Some(b) = bytes {
        let p = root().join("cache.json");
        let tmp = p.with_extension("json.tmp");
        if fs::write(&tmp, b).is_ok() {
            let _ = fs::rename(&tmp, &p);
        }
    }
}

pub fn cache_get(key: &str) -> Option<serde_json::Value> {
    cache().lock().ok()?.map.get(key).cloned()
}

pub fn cache_set(key: &str, val: serde_json::Value) {
    if let Ok(mut c) = cache().lock() {
        c.map.insert(key.to_string(), val);
        c.dirty = true;
    }
}


#[cfg(test)]
mod scan_tests {
    #[test]
    #[ignore]
    fn rescan_copy() {
        let r = super::rescan();
        println!("{}", serde_json::to_string(&r).unwrap());
        let all = super::load();
        for e in &all {
            println!("{} {} | {} | apple={:?} | {}", e.name, e.version, e.file_path.split("/iPacked/").last().unwrap(), e.apple_id, e.source);
        }
        let r2 = super::rescan();
        println!("second: {}", serde_json::to_string(&r2).unwrap());
    }
}

#[cfg(test)]
mod name_tests {
    use super::*;
    #[test]
    fn template() {
        let mut e = ArchiveEntry { name: "Minecraft".into(), version: "0.3.3".into(), build: "0.3.3.1".into(), bundle_id: "com.mojang.minecraftpe".into(), external_version_id: Some("9470435".into()), ..Default::default() };
        assert_eq!(template_name(DEFAULT_TEMPLATE, &e), "Minecraft v0.3.3 (0.3.3.1) (9470435)");
        assert_eq!(template_name("<name> (<version>)", &e), "Minecraft (0.3.3)");
        e.external_version_id = None;
        assert_eq!(template_name(DEFAULT_TEMPLATE, &e), "Minecraft v0.3.3 (0.3.3.1)");
        e.name = "A/B: C".into();
        assert_eq!(template_name("<name>", &e), "A_B_ C");
        e.app_name = Some("Phone Name".into());
        e.store_name = Some("Store Name".into());
        assert_eq!(template_name("<Appname> - <name>", &e), "Phone Name - Store Name");
    }
}


/// Re-extract an archived app's icon from its IPA (the home-screen icon the app
/// declares; iTunesArtwork only when the app has none). For Mac .pkg files the
/// app's .icns is pulled out of the package.
pub fn refresh_icon(id: &str) -> Result<ArchiveEntry, String> {
    let mut all = load();
    let i = all.iter().position(|e| e.id == id).ok_or("Archive entry not found")?;
    let path = PathBuf::from(&all[i].file_path);
    let png = if path.extension().map(|x| x.eq_ignore_ascii_case("pkg")).unwrap_or(false) {
        let info = mac_pkg_info(&path)?;
        apply_mac_info(&mut all[i], &info);
        info.icon_png.ok_or("No app icon found in the package")?
    } else {
        let fresh = analyze(&path, &all[i].source)?;
        match fresh.icon_path {
            Some(p) => fs::read(p).map_err(|e| e.to_string())?,
            None => {
                let file = fs::File::open(&path).map_err(|e| e.to_string())?;
                let mut zip = ZipArchive::new(file).map_err(|e| format!("Invalid IPA: {e}"))?;
                ["iTunesArtwork@2x", "iTunesArtwork"]
                    .iter()
                    .find_map(|n| read_entry(&mut zip, n))
                    .ok_or("No icon found in the IPA")?
            }
        }
    };
    let stamp = chrono::Utc::now().timestamp();
    let dest = root().join("icons").join(format!("{}-{stamp}.png", &all[i].sha256));
    if let Some(old) = &all[i].icon_path {
        if old.contains(&all[i].sha256) && old.ends_with(".png") && PathBuf::from(old) != dest {
            let _ = fs::remove_file(old);
        }
    }
    fs::write(&dest, cgbi::normalize(png)).map_err(|e| e.to_string())?;
    all[i].icon_path = Some(dest.to_string_lossy().to_string());
    let out = all[i].clone();
    save(&all)?;
    Ok(out)
}

#[derive(Default)]
pub struct MacInfo {
    pub bundle_id: Option<String>,
    pub name: Option<String>,
    pub version: Option<String>,
    pub build: Option<String>,
    pub min_macos: Option<String>,
    pub category: Option<String>,
    pub archs: Vec<String>,
    pub icon_png: Option<Vec<u8>>,
}

fn apply_mac_info(e: &mut ArchiveEntry, m: &MacInfo) {
    if let Some(v) = &m.bundle_id { if e.bundle_id.chars().all(|c| c.is_ascii_digit()) || e.bundle_id.is_empty() { e.bundle_id = v.clone(); } }
    if let Some(v) = &m.version { if e.version.is_empty() { e.version = v.clone(); } }
    if let Some(v) = &m.build { if e.build.is_empty() || e.build == e.version { e.build = v.clone(); } }
    if m.min_macos.is_some() { e.min_ios = m.min_macos.clone(); }
    if !m.archs.is_empty() { e.architectures = m.archs.clone(); }
    if let Some(v) = &m.name { if e.app_name.is_none() { e.app_name = Some(v.clone()); } }
}

/// Read the main app's Info.plist, architectures and icon out of a Mac .pkg
/// (expanded with pkgutil into a temp folder, removed afterwards).
pub fn mac_pkg_info(pkg: &Path) -> Result<MacInfo, String> {
    let tmp = std::env::temp_dir().join(format!("ipacked-pkg-{}", std::process::id()));
    let _ = fs::remove_dir_all(&tmp);
    let st = std::process::Command::new("pkgutil")
        .arg("--expand-full")
        .arg(pkg)
        .arg(&tmp)
        .output()
        .map_err(|e| format!("could not run pkgutil: {e}"))?;
    if !st.status.success() {
        let _ = fs::remove_dir_all(&tmp);
        return Err(format!("pkgutil failed: {}", String::from_utf8_lossy(&st.stderr).trim()));
    }
    // Shallowest *.app/Contents/Info.plist = the main app.
    let mut best: Option<PathBuf> = None;
    let mut stack = vec![tmp.clone()];
    while let Some(d) = stack.pop() {
        if let Ok(rd) = fs::read_dir(&d) {
            for en in rd.flatten() {
                let p = en.path();
                if !p.is_dir() || p.is_symlink() {
                    continue;
                }
                if p.extension().map(|x| x == "app").unwrap_or(false) && p.join("Contents/Info.plist").exists() {
                    let depth = p.components().count();
                    if best.as_ref().map(|b| depth < b.components().count()).unwrap_or(true) {
                        best = Some(p.clone());
                    }
                    continue;
                }
                stack.push(p);
            }
        }
    }
    let app = match best {
        Some(a) => a,
        None => {
            let _ = fs::remove_dir_all(&tmp);
            return Err("No .app found inside the package".into());
        }
    };
    let info: Dictionary = plist::from_file(app.join("Contents/Info.plist")).map_err(|e| format!("could not read Info.plist: {e}"))?;
    let mut m = MacInfo {
        bundle_id: s(&info, "CFBundleIdentifier"),
        name: s(&info, "CFBundleDisplayName").or_else(|| s(&info, "CFBundleName")),
        version: s(&info, "CFBundleShortVersionString"),
        build: s(&info, "CFBundleVersion"),
        min_macos: s(&info, "LSMinimumSystemVersion"),
        category: s(&info, "LSApplicationCategoryType"),
        ..Default::default()
    };
    if let Some(exe) = s(&info, "CFBundleExecutable") {
        if let Ok(b) = fs::read(app.join("Contents/MacOS").join(exe)) {
            m.archs = binary_info(&b).0;
        }
    }
    let icns = s(&info, "CFBundleIconFile")
        .map(|n| if n.ends_with(".icns") { n } else { format!("{n}.icns") })
        .map(|n| app.join("Contents/Resources").join(n))
        .filter(|p| p.exists())
        .or_else(|| {
            fs::read_dir(app.join("Contents/Resources")).ok()?.flatten().map(|e| e.path()).find(|p| p.extension().map(|x| x == "icns").unwrap_or(false))
        });
    if let Some(icns) = icns {
        let out = tmp.join("icon.png");
        let ok = std::process::Command::new("sips")
            .args(["-s", "format", "png", "-Z", "512"])
            .arg(&icns)
            .arg("--out")
            .arg(&out)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        if ok {
            m.icon_png = fs::read(&out).ok();
        }
    }
    let _ = fs::remove_dir_all(&tmp);
    Ok(m)
}

#[cfg(test)]
mod pkg_live {
    #[test]
    #[ignore]
    fn pkg_info_live() {
        let p = std::env::var("IPACKED_PKG").unwrap();
        let m = super::mac_pkg_info(std::path::Path::new(&p)).unwrap();
        println!("{:?} {:?} {:?} {:?} {:?} icon={}", m.bundle_id, m.version, m.build, m.min_macos, m.archs, m.icon_png.map(|x| x.len()).unwrap_or(0));
    }
}


/// Platform of an IPA from its Info.plist and file list.
fn platform_of(info: &Dictionary, names: &[String], app_dir: &str) -> (String, bool) {
    let plats: Vec<String> = info
        .get("CFBundleSupportedPlatforms")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_string).map(str::to_lowercase).collect())
        .unwrap_or_default();
    let fams: Vec<u64> = info
        .get("UIDeviceFamily")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_unsigned_integer).collect())
        .unwrap_or_default();
    let has_watch = names.iter().any(|n| n.starts_with(&format!("{app_dir}Watch/")));
    let p = if plats.iter().any(|p| p == "appletvos") || fams.contains(&3) {
        "tvos"
    } else if plats.iter().any(|p| p == "watchos") || fams.contains(&4) {
        "watchos"
    } else if plats.iter().any(|p| p == "xros") || fams.contains(&7) {
        "visionos"
    } else if info.get("LSApplicationLaunchProhibited").and_then(Value::as_boolean).unwrap_or(false) {
        // Messages-only app: sticker pack or iMessage app.
        if names.iter().any(|n| n.contains(".stickerpack/") || n.contains("Stickers.xcstickers")) {
            "stickers"
        } else {
            "imessage"
        }
    } else {
        "ios"
    };
    (p.to_string(), has_watch)
}

/// bundle-icon helper (scripts/bundle-icon.swift), shipped in Resources.
fn bundle_icon_tool() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let res = exe.parent()?.parent()?.join("Resources/bundle-icon");
    if res.exists() {
        return Some(res);
    }
    let dev = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("binaries/bundle-icon");
    dev.exists().then_some(dev)
}

/// Unpack the app's top-level files (Info.plist, Assets.car, PNGs) into a temp
/// bundle and let macOS draw its icon (correct tvOS / watchOS / visionOS shape).
fn render_bundle_icon<R: Read + Seek>(zip: &mut ZipArchive<R>, names: &[String], app_dir: &str, out: &Path) -> bool {
    let Some(tool) = bundle_icon_tool() else { return false };
    let app_name = app_dir.trim_end_matches('/').rsplit('/').next().unwrap_or("App.app").to_string();
    let tmp = std::env::temp_dir().join(format!("ipacked-icon-{}-{}", std::process::id(), chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)));
    let bundle = tmp.join(&app_name);
    if fs::create_dir_all(&bundle).is_err() {
        return false;
    }
    for n in names.iter().filter(|n| n.starts_with(app_dir) && !n[app_dir.len()..].contains('/') && n.len() > app_dir.len()) {
        let rest = &n[app_dir.len()..];
        let lower = rest.to_lowercase();
        if lower == "info.plist" || lower.ends_with(".car") || lower.ends_with(".png") {
            if let Some(b) = read_entry(zip, n) {
                let _ = fs::write(bundle.join(rest), b);
            }
        }
    }
    let ok = std::process::Command::new(tool)
        .arg(&bundle)
        .arg(out)
        .arg("512")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
        && out.exists();
    let _ = fs::remove_dir_all(&tmp);
    ok
}

/// Fill platform info for entries archived before it existed (no re-hashing).
pub fn platform_upgrade(e: &mut ArchiveEntry) {
    if e.platform.is_some() {
        return;
    }
    if e.file_path.to_lowercase().ends_with(".pkg") {
        e.platform = Some("macos".into());
        return;
    }
    let Ok(file) = fs::File::open(&e.file_path) else { return };
    let Ok(mut zip) = ZipArchive::new(file) else { return };
    let names: Vec<String> = (0..zip.len()).filter_map(|i| zip.by_index(i).ok().map(|f| f.name().to_string())).collect();
    let Some(info_name) = names.iter().find(|n| n.starts_with("Payload/") && n.ends_with(".app/Info.plist") && n.matches('/').count() == 2).cloned() else { return };
    let app_dir = info_name.trim_end_matches("Info.plist").to_string();
    let Some(info) = read_entry(&mut zip, &info_name).and_then(|b| plist::from_bytes::<Dictionary>(&b).ok()) else { return };
    let (p, w) = platform_of(&info, &names, &app_dir);
    if p == "tvos" || p == "watchos" || p == "visionos" {
        let out = root().join("icons").join(format!("{}-{}.png", e.sha256, p));
        if render_bundle_icon(&mut zip, &names, &app_dir, &out) {
            e.icon_path = Some(out.to_string_lossy().to_string());
        }
    }
    e.platform = Some(p);
    e.has_watch = w;
}

#[cfg(test)]
mod plat_live {
    #[test]
    #[ignore]
    fn platform_live() {
        for p in std::env::var("IPACKED_IPAS").unwrap().split('|') {
            let mut e = super::ArchiveEntry { file_path: p.into(), sha256: "test".into(), ..Default::default() };
            super::platform_upgrade(&mut e);
            println!("{:?} watch={} icon={:?}", e.platform, e.has_watch, e.icon_path);
        }
    }
}
