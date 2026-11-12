//! SHSH blob collection: per-device registry, scanning blob folders (iPacked,
//! blobsaver, Legacy iOS Kit, …), sorting blobs into per-device folders and
//! saving new blobs with tsschecker.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct BlobDevice {
    /// Decimal ECID.
    pub ecid: String,
    pub name: String,
    pub product_type: String,
    pub board_config: String,
    #[serde(default)]
    pub note: String,
    #[serde(default)]
    pub apnonce: String,
    #[serde(default)]
    pub generator: String,
    #[serde(default)]
    pub udid: String,
    /// User-picked SF symbol name and colour for the list icon.
    #[serde(default)]
    pub icon: String,
    #[serde(default)]
    pub color: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct BlobSettings {
    /// Extra folders to scan (blobsaver output, Legacy iOS Kit "saved/shsh", …).
    pub sources: Vec<String>,
    pub devices: Vec<BlobDevice>,
}

pub fn root() -> PathBuf {
    let p = crate::archive::root().join("Blobs");
    fs::create_dir_all(&p).ok();
    p
}

fn settings_path() -> PathBuf {
    root().join("blobs.json")
}

pub fn load() -> BlobSettings {
    fs::read(settings_path())
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

pub fn save(s: &BlobSettings) -> Result<(), String> {
    fs::write(settings_path(), serde_json::to_vec_pretty(s).map_err(err)?).map_err(err)
}

/// Normalise an ECID given in decimal or hex (0x…) to decimal.
pub fn ecid_dec(s: &str) -> Option<String> {
    let t = s.trim();
    let v = if let Some(h) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
        u64::from_str_radix(h, 16).ok()?
    } else if t.chars().all(|c| c.is_ascii_digit()) {
        t.parse::<u64>().ok()?
    } else if t.chars().all(|c| c.is_ascii_hexdigit()) {
        u64::from_str_radix(t, 16).ok()?
    } else {
        return None;
    };
    Some(v.to_string())
}

pub fn device_folder(d: &BlobDevice) -> PathBuf {
    let safe: String = d
        .name
        .chars()
        .map(|c| if c.is_alphanumeric() || " -_".contains(c) { c } else { '_' })
        .collect();
    let p = root().join(format!("{}_{}", d.ecid, safe.trim()));
    fs::create_dir_all(&p).ok();
    p
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BlobFile {
    pub path: String,
    pub file_name: String,
    pub ecid: Option<String>,
    pub product_type: Option<String>,
    pub board: Option<String>,
    pub version: Option<String>,
    pub build: Option<String>,
    pub apnonce: Option<String>,
    pub generator: Option<String>,
    pub size: u64,
    pub modified: Option<String>,
    pub source: String,
}

fn is_product_type(t: &str) -> bool {
    let prefixes = ["iPhone", "iPad", "iPod", "AppleTV", "Watch", "AudioAccessory", "RealityDevice"];
    prefixes.iter().any(|p| t.starts_with(p)) && t.contains(',')
}

fn is_version(t: &str) -> bool {
    let parts: Vec<&str> = t.split('.').collect();
    (2..=3).contains(&parts.len()) && parts.iter().all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()))
}

fn is_build(t: &str) -> bool {
    // e.g. 9B206, 18G82, 21A5248v
    let b = t.as_bytes();
    let digits = b.iter().take_while(|c| c.is_ascii_digit()).count();
    (1..=2).contains(&digits)
        && b.get(digits).map(|c| c.is_ascii_uppercase()).unwrap_or(false)
        && b[digits + 1..].first().map(|c| c.is_ascii_digit()).unwrap_or(false)
        && b[digits + 1..].iter().all(|c| c.is_ascii_alphanumeric())
}

/// Parse blob file names from tsschecker/blobsaver
/// (`ECID_iPhone10,3_d22ap_14.7.1-18G82_<apnonce>.shsh2`), Legacy iOS Kit
/// (`ECID-iPad1,1-5.1.1-9B206.shsh`) and similar variants.
pub fn parse_name(name: &str) -> BlobFile {
    let stem = name
        .trim_end_matches(".shsh2")
        .trim_end_matches(".shsh")
        .trim_end_matches(".plist");
    let mut f = BlobFile {
        path: String::new(),
        file_name: name.to_string(),
        ecid: None,
        product_type: None,
        board: None,
        version: None,
        build: None,
        apnonce: None,
        generator: None,
        size: 0,
        modified: None,
        source: String::new(),
    };
    for tok in stem.split(['_', '-', ' ']).filter(|t| !t.is_empty()) {
        if f.product_type.is_none() && is_product_type(tok) {
            f.product_type = Some(tok.to_string());
        } else if f.version.is_none() && is_version(tok) {
            f.version = Some(tok.to_string());
        } else if f.build.is_none() && is_build(tok) {
            f.build = Some(tok.to_string());
        } else if f.board.is_none() && tok.to_lowercase().ends_with("ap") && tok.len() <= 8 && tok.chars().next().map(|c| c.is_ascii_alphabetic()).unwrap_or(false) {
            f.board = Some(tok.to_lowercase());
        } else if f.ecid.is_none() && (tok.len() >= 9 && tok.len() <= 20) && (tok.chars().all(|c| c.is_ascii_digit()) || tok.starts_with("0x")) {
            f.ecid = ecid_dec(tok);
        } else if tok.len() >= 32 && tok.chars().all(|c| c.is_ascii_hexdigit()) {
            f.apnonce = Some(tok.to_string());
        } else if tok.starts_with("0x") && tok.len() == 18 {
            f.generator = Some(tok.to_string());
        }
    }
    f
}

/// Generator stored in an .shsh2 (blobsaver/tsschecker add it as a top-level key).
fn read_generator(path: &Path) -> Option<String> {
    let v: plist::Value = plist::from_file(path).ok()?;
    v.as_dictionary()?.get("generator")?.as_string().map(str::to_string)
}

fn walk(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    if depth > 6 {
        return;
    }
    let Ok(rd) = fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            walk(&p, depth + 1, out);
        } else if p
            .extension()
            .map(|x| x.eq_ignore_ascii_case("shsh2") || x.eq_ignore_ascii_case("shsh"))
            .unwrap_or(false)
        {
            out.push(p);
        }
    }
}

/// Scan the iPacked collection plus every configured source folder.
pub fn scan() -> Vec<BlobFile> {
    let s = load();
    let mut roots = vec![(root(), "iPacked".to_string())];
    for src in &s.sources {
        roots.push((PathBuf::from(src), src.clone()));
    }
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for (r, label) in roots {
        let mut files = Vec::new();
        walk(&r, 0, &mut files);
        for p in files {
            let canon = fs::canonicalize(&p).unwrap_or(p.clone());
            if !seen.insert(canon) {
                continue;
            }
            let mut f = parse_name(&p.file_name().unwrap_or_default().to_string_lossy());
            // Files inside an iPacked device folder inherit its ECID.
            if f.ecid.is_none() {
                if let Some(dir) = p.parent().and_then(|d| d.file_name()).map(|d| d.to_string_lossy().to_string()) {
                    f.ecid = dir.split('_').next().and_then(ecid_dec);
                }
            }
            if f.generator.is_none() {
                f.generator = read_generator(&p);
            }
            let md = fs::metadata(&p).ok();
            f.size = md.as_ref().map(|m| m.len()).unwrap_or(0);
            f.modified = md
                .and_then(|m| m.modified().ok())
                .map(|t| chrono::DateTime::<chrono::Utc>::from(t).to_rfc3339());
            f.path = p.to_string_lossy().to_string();
            f.source = label.clone();
            out.push(f);
        }
    }
    out
}

/// Copy every blob whose ECID matches a registered device into that device's
/// iPacked folder ("collection"). Returns how many files were copied.
pub fn collect() -> Result<usize, String> {
    let s = load();
    let by_ecid: HashMap<String, &BlobDevice> = s.devices.iter().map(|d| (d.ecid.clone(), d)).collect();
    let mut n = 0;
    for f in scan() {
        if f.source == "iPacked" {
            continue;
        }
        let Some(dev) = f.ecid.as_ref().and_then(|e| by_ecid.get(e)) else { continue };
        let dest = device_folder(dev).join(&f.file_name);
        if !dest.exists() {
            fs::copy(&f.path, &dest).map_err(err)?;
            n += 1;
        }
    }
    Ok(n)
}

/// Firmware list for a product type from ipsw.me: full IPSW releases merged with
/// the OTA feed (tvOS / audioOS releases that only ship as OTA, and betas).
/// Each entry: version, buildid, signed, releasedate, ota, beta, url.
pub async fn firmwares(product_type: &str) -> Result<Value, String> {
    let key = format!("ipsw4:{product_type}");
    let now = chrono::Utc::now().timestamp();
    if let Some(v) = crate::archive::cache_get(&key) {
        if now - v["fetchedAt"].as_i64().unwrap_or(0) < 6 * 3600 {
            return Ok(v);
        }
    }
    let get = |kind: &'static str| {
        let url = format!("https://api.ipsw.me/v4/device/{product_type}?type={kind}");
        async move {
            let r = reqwest::get(&url).await.ok()?;
            if !r.status().is_success() {
                return None;
            }
            r.json::<Value>().await.ok()
        }
    };
    let (ipsw, ota) = tokio::join!(get("ipsw"), get("ota"));
    if ipsw.is_none() && ota.is_none() {
        return Err(format!("ipsw.me doesn't know this model: {product_type}"));
    }
    let mut out: Vec<Value> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for f in ipsw.as_ref().and_then(|b| b["firmwares"].as_array()).cloned().unwrap_or_default() {
        let build = f["buildid"].as_str().unwrap_or_default().to_string();
        if seen.insert(build) {
            out.push(json!({
                "version": f["version"], "buildid": f["buildid"], "signed": f["signed"],
                "releasedate": f["releasedate"], "ota": false, "beta": false, "url": f["url"],
            }));
        }
    }
    for f in ota.as_ref().and_then(|b| b["firmwares"].as_array()).cloned().unwrap_or_default() {
        let build = f["buildid"].as_str().unwrap_or_default().to_string();
        // OTA feeds list one entry per prerequisite build; keep the full ones.
        if !f["prerequisitebuildid"].as_str().unwrap_or("").is_empty() && seen.contains(&build) {
            continue;
        }
        let beta = f["releasetype"].as_str() == Some("Beta");
        let version = f["marketingversion"].as_str().filter(|s| !s.is_empty()).or(f["version"].as_str()).unwrap_or("");
        // Newer OTA feeds tag versions "9.9.<real version>" (9.9.26.7.3 = 26.7.3).
        let version = version.strip_prefix("9.9.").unwrap_or(version);
        let version = if beta && !version.to_lowercase().contains("beta") { format!("{version} beta") } else { version.to_string() };
        if let Some(prev) = out.iter_mut().find(|x| x["buildid"].as_str() == Some(build.as_str())) {
            // Same build seen as OTA: just merge the signed flag.
            if f["signed"].as_bool() == Some(true) && prev["ota"].as_bool() == Some(true) {
                prev["signed"] = json!(true);
            }
            continue;
        }
        seen.insert(build);
        out.push(json!({
            "version": version, "buildid": f["buildid"], "signed": f["signed"],
            "releasedate": f["releasedate"].as_str().or(f["uploaddate"].as_str()),
            "ota": true, "beta": beta, "url": f["url"],
        }));
    }
    for b in appledb_betas(product_type, &out).await {
        let build = b["buildid"].as_str().unwrap_or_default().to_string();
        if seen.insert(build) {
            out.push(b);
        }
    }
    out.sort_by(|a, b| b["releasedate"].as_str().unwrap_or("").cmp(a["releasedate"].as_str().unwrap_or("")));
    let body = ipsw.or(ota).unwrap_or_default();
    let v = json!({ "fetchedAt": now, "name": body["name"], "boards": body["boards"], "firmwares": out });
    crate::archive::cache_set(&key, v.clone());
    Ok(v)
}

/// Pull just BuildManifest.plist out of a remote IPSW/OTA zip with HTTP range
/// requests (a few hundred KB instead of a multi-GB download).
fn os_names(product_type: &str) -> &'static [&'static str] {
    if product_type.starts_with("iPhone") || product_type.starts_with("iPod") {
        &["iOS"]
    } else if product_type.starts_with("iPad") {
        &["iPadOS", "iOS"]
    } else if product_type.starts_with("AppleTV") {
        &["tvOS"]
    } else if product_type.starts_with("Watch") {
        &["watchOS"]
    } else if product_type.starts_with("AudioAccessory") {
        &["audioOS"]
    } else if product_type.starts_with("RealityDevice") {
        &["visionOS"]
    } else {
        &["iOS"]
    }
}

/// Beta-style build numbers end in a lowercase letter (24A5380h).
fn is_beta_build(b: &str) -> bool {
    b.chars().last().map(|c| c.is_ascii_lowercase()).unwrap_or(false) && b.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false)
}

fn build_major(b: &str) -> u32 {
    b.chars().take_while(|c| c.is_ascii_digit()).collect::<String>().parse().unwrap_or(0)
}

/// One AppleDB build entry (cached forever; builds don't change) shaped like a
/// firmwares() row for this device, or None when it doesn't apply to it.
pub async fn appledb_build(product_type: &str, build: &str) -> Option<Value> {
    for os in os_names(product_type) {
        let key = format!("appledb2:{os};{build}");
        let d = match crate::archive::cache_get(&key) {
            Some(v) => v,
            None => {
                let url = format!("https://api.appledb.dev/ios/{os};{build}.json");
                let r = reqwest::get(&url).await.ok()?;
                if !r.status().is_success() {
                    continue;
                }
                let full: Value = r.json().await.ok()?;
                // Keep only what iPacked uses (full entries are ~60 KB each).
                let sources: Vec<Value> = full["sources"].as_array().cloned().unwrap_or_default().iter().map(|s| {
                    let link = s["links"].as_array().and_then(|l| l.iter().find_map(|x| x["url"].as_str().filter(|u| u.starts_with("https")).map(str::to_string)));
                    json!({ "type": s["type"], "deviceMap": s["deviceMap"], "links": [{ "url": link }] })
                }).collect();
                let v = json!({ "version": full["version"], "released": full["released"], "beta": full["beta"], "deviceMap": full["deviceMap"], "sources": sources });
                crate::archive::cache_set(&key, v.clone());
                v
            }
        };
        let devs = d["deviceMap"].as_array().cloned().unwrap_or_default();
        if !devs.iter().any(|x| x.as_str() == Some(product_type)) {
            continue;
        }
        let mut url = None;
        let mut ota = false;
        for kind in ["ipsw", "ota"] {
            if url.is_some() {
                break;
            }
            for s in d["sources"].as_array().cloned().unwrap_or_default() {
                if s["type"].as_str() == Some(kind) && s["deviceMap"].as_array().map(|a| a.iter().any(|x| x.as_str() == Some(product_type))).unwrap_or(false) {
                    url = s["links"].as_array().and_then(|l| l.iter().find_map(|x| x["url"].as_str().filter(|u| u.starts_with("https")).map(str::to_string)));
                    ota = kind == "ota";
                    if url.is_some() {
                        break;
                    }
                }
            }
        }
        return Some(json!({
            "version": d["version"], "buildid": build, "signed": Value::Null,
            "releasedate": d["released"], "ota": ota, "beta": d["beta"].as_bool().unwrap_or(is_beta_build(build)),
            "url": url, "source": "appledb",
        }));
    }
    None
}

/// Recent betas from AppleDB (ipsw.me doesn't list most of them): beta builds of
/// the latest two major releases this device runs.
async fn appledb_betas(product_type: &str, known: &[Value]) -> Vec<Value> {
    let idx: Vec<String> = match crate::archive::cache_get("appledb:index") {
        Some(v) if chrono::Utc::now().timestamp() - v["t"].as_i64().unwrap_or(0) < 6 * 3600 => serde_json::from_value(v["keys"].clone()).unwrap_or_default(),
        _ => {
            let Ok(r) = reqwest::get("https://api.appledb.dev/ios/index.json").await else { return vec![] };
            let keys: Vec<String> = r.json().await.unwrap_or_default();
            crate::archive::cache_set("appledb:index", json!({ "t": chrono::Utc::now().timestamp(), "keys": keys }));
            keys
        }
    };
    let top = known.iter().filter_map(|f| f["buildid"].as_str()).map(build_major).max().unwrap_or(0);
    let oses = os_names(product_type);
    let mut builds: Vec<String> = idx
        .iter()
        .filter_map(|k| k.split_once(';'))
        .filter(|(os, b)| oses.contains(os) && is_beta_build(b) && top > 0 && build_major(b) + 1 >= top)
        .map(|(_, b)| b.to_string())
        .collect();
    builds.sort();
    builds.dedup();
    builds.truncate(80);
    let futs = builds.iter().map(|b| appledb_build(product_type, b));
    futures::future::join_all(futs).await.into_iter().flatten().collect()
}

fn is_zip_url(u: &str) -> bool {
    let l = u.to_lowercase();
    l.ends_with(".ipsw") || l.ends_with(".zip")
}

pub async fn remote_build_manifest(url: &str, out: &std::path::Path) -> Result<(), String> {
    let client = reqwest::Client::new();
    let range = |start: u64, end: u64| {
        let client = client.clone();
        let url = url.to_string();
        async move {
            let r = client
                .get(&url)
                .header("Range", format!("bytes={start}-{end}"))
                .send()
                .await
                .map_err(err)?;
            if r.status().as_u16() != 206 {
                return Err(format!("server doesn't support range requests ({})", r.status()));
            }
            r.bytes().await.map(|b| b.to_vec()).map_err(err)
        }
    };
    let head = client.head(url).send().await.map_err(err)?;
    let size: u64 = head
        .headers()
        .get("content-length")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse().ok())
        .ok_or("could not get the file size")?;
    let tail_len = size.min(128 * 1024);
    let tail = range(size - tail_len, size - 1).await?;
    let le16 = |b: &[u8], o: usize| u16::from_le_bytes([b[o], b[o + 1]]) as u64;
    let le32 = |b: &[u8], o: usize| u32::from_le_bytes(b[o..o + 4].try_into().unwrap()) as u64;
    let le64 = |b: &[u8], o: usize| u64::from_le_bytes(b[o..o + 8].try_into().unwrap());
    let eocd = (0..tail.len().saturating_sub(21)).rev().find(|&i| tail[i..i + 4] == [0x50, 0x4b, 5, 6]).ok_or("end of zip not found")?;
    let (mut cd_size, mut cd_off) = (le32(&tail, eocd + 12), le32(&tail, eocd + 16));
    if cd_off == 0xFFFF_FFFF || cd_size == 0xFFFF_FFFF {
        // ZIP64: locator sits right before EOCD.
        let loc = eocd.checked_sub(20).filter(|&l| tail[l..l + 4] == [0x50, 0x4b, 6, 7]).ok_or("no zip64 locator")?;
        let z64 = le64(&tail, loc + 8);
        let rec = if z64 >= size - tail_len { tail[(z64 - (size - tail_len)) as usize..].to_vec() } else { range(z64, z64 + 55).await? };
        cd_size = le64(&rec, 40);
        cd_off = le64(&rec, 48);
    }
    let cd = range(cd_off, cd_off + cd_size - 1).await?;
    let mut i = 0usize;
    let mut best: Option<(u64, u64, u64, u16, String)> = None;
    while i + 46 <= cd.len() && cd[i..i + 4] == [0x50, 0x4b, 1, 2] {
        let method = le16(&cd, i + 10) as u16;
        let mut csize = le32(&cd, i + 20);
        let mut usize_ = le32(&cd, i + 24);
        let (nl, xl, cl) = (le16(&cd, i + 28) as usize, le16(&cd, i + 30) as usize, le16(&cd, i + 32) as usize);
        let mut lho = le32(&cd, i + 42);
        let name = String::from_utf8_lossy(&cd[i + 46..i + 46 + nl]).to_string();
        // ZIP64 extra field (0x0001) carries the real sizes/offset.
        let mut x = i + 46 + nl;
        let xe = x + xl;
        while x + 4 <= xe {
            let (id, len) = (le16(&cd, x), le16(&cd, x + 2) as usize);
            if id == 1 {
                let mut p = x + 4;
                if usize_ == 0xFFFF_FFFF { usize_ = le64(&cd, p); p += 8; }
                if csize == 0xFFFF_FFFF { csize = le64(&cd, p); p += 8; }
                if lho == 0xFFFF_FFFF { lho = le64(&cd, p); }
            }
            x += 4 + len;
        }
        if name.ends_with("BuildManifest.plist") {
            let rank = |n: &str| if n == "BuildManifest.plist" { 0 } else if n.contains("AssetData/boot/") { 1 } else { 2 };
            if best.as_ref().map(|b| rank(&name) < rank(&b.4)).unwrap_or(true) {
                best = Some((lho, csize, usize_, method, name));
            }
        }
        i += 46 + nl + xl + cl;
    }
    let (lho, csize, _, method, _) = best.ok_or("no BuildManifest.plist in the archive")?;
    let lh = range(lho, lho + 29).await?;
    let data_start = lho + 30 + le16(&lh, 26) + le16(&lh, 28);
    let raw = range(data_start, data_start + csize - 1).await?;
    let data = match method {
        0 => raw,
        8 => {
            let mut v = Vec::new();
            std::io::Read::read_to_end(&mut flate2::read::DeflateDecoder::new(&raw[..]), &mut v).map_err(err)?;
            v
        }
        m => return Err(format!("unsupported compression: {m}")),
    };
    // OTA zips carry a binary plist; tsschecker only loads XML ones.
    let v: plist::Value = plist::from_bytes(&data).map_err(|e| format!("could not read BuildManifest: {e}"))?;
    v.to_file_xml(out).map_err(err)
}

/// tsschecker path: bundled in Resources, or the dev copy.
pub fn tsschecker() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().and_then(|d| d.parent()).map(|c| c.join("Resources/tsschecker")))
        .filter(|p| p.exists())
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("binaries/tsschecker"))
}

/// Save a blob for one version into the device's collection folder.
/// `ota`: OTA-only build (tvOS etc.) → tsschecker `-o`. `url`: when given (betas,
/// or builds tsschecker's own database lacks) its BuildManifest is fetched
/// remotely and passed with `-m`.
pub async fn save_blob(dev: &BlobDevice, version: &str, build: &str, ota: bool, beta: bool, url: Option<String>) -> Result<String, String> {
    let dir = device_folder(dev);
    let _ = std::fs::create_dir_all(&dir);
    let mut args: Vec<String> = vec![
        "--nocache".into(), "-s".into(),
        "-d".into(), dev.product_type.clone(),
        "-e".into(), dev.ecid.clone(),
        "-B".into(), dev.board_config.clone(),
        "-Z".into(), build.to_string(),
        "--save-path".into(), dir.to_string_lossy().to_string(),
    ];
    if !dev.apnonce.is_empty() {
        args.extend(["--apnonce".into(), dev.apnonce.clone()]);
    }
    let manifest = std::env::temp_dir().join(format!("ipacked-{build}-BuildManifest.plist"));
    if ota {
        args.push("-o".into());
    }
    let generator = if dev.generator.is_empty() { "0x1111111111111111".to_string() } else { dev.generator.clone() };
    args.extend(["--generator".into(), generator]);
    let mut url = url.filter(|u| !u.is_empty());
    // OTA links (.aea) are encrypted Apple Archives, not zips: their
    // BuildManifest can't be read remotely. Use the build's IPSW instead.
    if url.as_deref().map(|u| !is_zip_url(u)).unwrap_or(true) && (beta || url.is_some()) {
        if let Some(u) = appledb_build(&dev.product_type, build).await.and_then(|v| v["url"].as_str().map(str::to_string)).filter(|u| is_zip_url(u)) {
            url = Some(u);
        } else if beta {
            return Err(format!("{version} ({build}) is only published as an encrypted OTA (.aea) for this device, so its BuildManifest can't be read. No IPSW exists for this build."));
        }
    }
    // Betas: tsschecker's own database doesn't know them → manifest first.
    // Others: by build number, then fall back to the remote manifest.
    let mut tries: Vec<bool> = if beta { vec![true] } else { vec![false] };
    if !beta && url.is_some() {
        tries.push(true);
    }
    let mut last = String::new();
    for use_manifest in tries {
        let mut a = args.clone();
        if use_manifest {
            let u = url.as_deref().ok_or("No download URL for this version")?;
            remote_build_manifest(u, &manifest).await.map_err(|e| format!("could not fetch BuildManifest: {e}"))?;
            if let Some(p) = a.iter().position(|x| x == "-Z") {
                a.drain(p..p + 2);
            }
            a.extend(["-m".into(), manifest.to_string_lossy().to_string()]);
        }
        let out = tokio::process::Command::new(tsschecker())
            .args(&a)
            .output()
            .await
            .map_err(|e| format!("could not run tsschecker: {e}"))?;
        let _ = std::fs::remove_file(&manifest);
        let log = String::from_utf8_lossy(&out.stdout).to_string() + &String::from_utf8_lossy(&out.stderr);
        if log.contains("Saved shsh blobs") || log.contains("Saved signing ticket") {
            return Ok(format!("Blob saved: {version} ({build})"));
        }
        if log.contains("not being signed") {
            return Err(format!("{version} ({build}) is no longer signed"));
        }
        let tail: Vec<&str> = log.lines().filter(|l| !l.trim().is_empty()).collect();
        last = tail[tail.len().saturating_sub(6)..].join("\n");
    }
    Err(last)
}

/// ECID, model, board and name of a connected device (lockdown via libimobiledevice
/// first — it works for every iOS version — then the native client).
pub async fn read_connected(udid: &str) -> Result<BlobDevice, String> {
    let mut d = BlobDevice { udid: udid.to_string(), ..Default::default() };
    let get = |k: &'static str| {
        let udid = udid.to_string();
        async move {
            crate::tools::run("ideviceinfo", &["-u", &udid, "-k", k])
                .await
                .map(|b| String::from_utf8_lossy(&b).trim().to_string())
        }
    };
    d.ecid = ecid_dec(&get("UniqueChipID").await?).ok_or("could not read ECID")?;
    d.product_type = get("ProductType").await?;
    d.board_config = get("HardwareModel").await?.to_lowercase();
    d.name = get("DeviceName").await.unwrap_or_default();
    Ok(d)
}


fn b64_hex(s: &str, reverse: bool) -> Option<String> {
    use base64::Engine;
    let mut b = base64::engine::general_purpose::STANDARD.decode(s.trim()).ok()?;
    if b.is_empty() {
        return None;
    }
    if reverse {
        b.reverse();
    }
    Some(hex::encode(b))
}

async fn info_key(udid: &str, k: &str) -> Option<String> {
    crate::tools::run("ideviceinfo", &["-u", udid, "-k", k])
        .await
        .ok()
        .map(|b| String::from_utf8_lossy(&b).trim().to_string())
        .filter(|s| !s.is_empty())
}

async fn irecovery(args: &[&str]) -> Result<String, String> {
    crate::tools::run("irecovery", args).await.map(|b| String::from_utf8_lossy(&b).to_string())
}

/// Port of blobsaver's GetApnonceTask (LibimobiledeviceUtil.java), using the
/// libimobiledevice / libirecovery command-line tools:
/// enter recovery → read NONC → "reset" → read NONC again (frozen check) →
/// setenv auto-boot true + saveenv + reboot → wait for the device + unlock →
/// read BootNonce (= generator, little-endian). If the nonce wasn't frozen the
/// whole round runs once more, after reading ApNonce in normal mode first.
pub async fn read_apnonce(udid: &str, progress: impl Fn(&str)) -> Result<(String, String), String> {
    use std::time::Duration;
    crate::tools::which("irecovery").ok_or("'irecovery' not found. Install: brew install libirecovery")?;
    let wait_recovery = || async {
        // blobsaver: 60 s timeout.
        for _ in 0..60 {
            if let Ok(q) = irecovery(&["-q"]).await {
                if let Some(n) = q.lines().find_map(|l| l.trim().strip_prefix("NONC: ")) {
                    return Some(n.trim().to_lowercase());
                }
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
        None
    };
    let mut try_again = false;
    loop {
        if try_again {
            progress("Reading APNonce in normal mode…");
            let _ = info_key(udid, "ApNonce").await;
        }
        progress("Entering recovery mode… This can take up to 60 seconds");
        crate::tools::run("ideviceenterrecovery", &[udid]).await?;
        let apnonce = wait_recovery().await.ok_or("Device didn't show up in recovery mode within 60 seconds")?;
        progress("Reading APNonce…");
        let _ = irecovery(&["-c", "reset"]).await;
        tokio::time::sleep(Duration::from_secs(2)).await;
        let again = wait_recovery().await.ok_or("Device didn't come back to recovery mode after reset")?;
        let mut retry = false;
        if again == apnonce {
            progress("Successfully got APNonce, exiting recovery mode…");
        } else if try_again {
            progress("Warning: got APNonce, but it is not frozen. The generator may not be set correctly. Exiting recovery mode…");
        } else {
            progress("APNonce is not frozen, trying again. Exiting recovery mode…");
            retry = true;
        }
        let _ = irecovery(&["-c", "setenv auto-boot true"]).await;
        let _ = irecovery(&["-c", "saveenv"]).await;
        let _ = irecovery(&["-c", "reboot"]).await;
        tokio::time::sleep(Duration::from_secs(3)).await;
        // Device back on usbmuxd (80 s), then lockdown after unlock (120 s).
        let mut back = false;
        for _ in 0..80 {
            tokio::time::sleep(Duration::from_secs(1)).await;
            if crate::device::udids().await.map(|v| v.iter().any(|u| u == udid)).unwrap_or(false) {
                back = true;
                break;
            }
        }
        if !back {
            return Err("Device didn't come back from recovery mode within 80 seconds".into());
        }
        progress("Please unlock your device");
        let mut generator = None;
        for _ in 0..120 {
            if let Some(g) = info_key(udid, "BootNonce").await.and_then(|v| b64_hex(&v, true)) {
                generator = Some(format!("0x{g}"));
                break;
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
        let generator = generator.ok_or("Couldn't read the generator (device still locked?)")?;
        if retry {
            try_again = true;
            continue;
        }
        progress("Success");
        return Ok((apnonce, generator));
    }
}

#[cfg(test)]
mod live {
    #[tokio::test]
    #[ignore]
    async fn betas_live() {
        let v = super::firmwares("iPhone14,7").await.unwrap();
        for f in v["firmwares"].as_array().unwrap().iter().filter(|f| f["beta"] == true).take(12) { println!("{} {} {} {}", f["version"], f["buildid"], f["releasedate"], f["url"].as_str().map(|u| &u[u.len()-30..]).unwrap_or("-")); }
    }
}

#[cfg(test)]
mod live_manifest {
    #[tokio::test]
    #[ignore]
    async fn beta_manifest_live() {
        let v = super::appledb_build("iPhone14,7", "24A5380h").await.unwrap();
        let u = v["url"].as_str().unwrap().to_string();
        println!("{u}");
        let out = std::env::temp_dir().join("bm-test.plist");
        super::remote_build_manifest(&u, &out).await.unwrap();
        println!("ok {}", std::fs::metadata(&out).unwrap().len());
    }
}
