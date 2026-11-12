//! Archive.org importer: list the IPAs of an item, read each one's Info.plist
//! and icon with HTTP range requests (no full download), and download the
//! chosen files into the archive.

use crate::archive::{self, ArchiveEntry};
use flate2::read::DeflateDecoder;
use reqwest::header::RANGE;
use serde_json::{json, Value};
use std::io::Read;
use std::path::PathBuf;
use tauri::{AppHandle, Emitter};

fn client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .user_agent("iPacked")
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .map_err(|e| e.to_string())
}

/// `https://archive.org/details/<id>[/...]` or a bare identifier → identifier.
pub fn identifier(url: &str) -> Result<String, String> {
    let u = url.trim().trim_end_matches('/');
    let id = if let Some(i) = u.find("/details/") {
        u[i + 9..].split(['/', '?', '#']).next().unwrap_or("")
    } else if let Some(i) = u.find("/download/") {
        u[i + 10..].split(['/', '?', '#']).next().unwrap_or("")
    } else if !u.contains('/') {
        u
    } else {
        ""
    };
    if id.is_empty() {
        return Err("Invalid Archive.org link (expected …/details/<name>)".into());
    }
    Ok(id.to_string())
}

/// Files of an item ending in .ipa: `{item, base, files:[{name,size}]}`.
pub async fn list(url: &str) -> Result<Value, String> {
    let id = identifier(url)?;
    let meta: Value = client()?
        .get(format!("https://archive.org/metadata/{id}"))
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;
    let files = meta["files"].as_array().ok_or("This Archive.org item was not found or is empty")?;
    let base = match (meta["server"].as_str(), meta["dir"].as_str()) {
        (Some(s), Some(d)) => format!("https://{s}{d}"),
        _ => format!("https://archive.org/download/{id}"),
    };
    let ipas: Vec<Value> = files
        .iter()
        .filter(|f| f["name"].as_str().map(|n| n.to_lowercase().ends_with(".ipa")).unwrap_or(false))
        .map(|f| json!({ "name": f["name"], "size": f["size"].as_str().and_then(|s| s.parse::<u64>().ok()).unwrap_or(0) }))
        .collect();
    Ok(json!({ "item": id, "title": meta["metadata"]["title"], "base": base, "files": ipas }))
}

fn file_url(base: &str, name: &str) -> String {
    let enc: String = name
        .split('/')
        .map(|p| {
            p.bytes()
                .map(|b| if b.is_ascii_alphanumeric() || b"-_.~()".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") })
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("/");
    format!("{base}/{enc}")
}

async fn range(c: &reqwest::Client, url: &str, spec: String) -> Result<Vec<u8>, String> {
    let r = c.get(url).header(RANGE, spec).send().await.map_err(|e| e.to_string())?;
    if !(r.status().is_success()) {
        return Err(format!("HTTP {}", r.status()));
    }
    Ok(r.bytes().await.map_err(|e| e.to_string())?.to_vec())
}

struct CdEntry {
    name: String,
    method: u16,
    csize: u64,
    usize_: u64,
    offset: u64,
}

fn u16le(b: &[u8], i: usize) -> u16 {
    u16::from_le_bytes([b[i], b[i + 1]])
}
fn u32le(b: &[u8], i: usize) -> u32 {
    u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]])
}

async fn central_dir(c: &reqwest::Client, url: &str) -> Result<Vec<CdEntry>, String> {
    let tail = range(c, url, "bytes=-65557".into()).await?;
    let pos = (0..tail.len().saturating_sub(21)).rev().find(|&i| u32le(&tail, i) == 0x06054b50).ok_or("end of ZIP not found")?;
    let cd_size = u32le(&tail, pos + 12) as u64;
    let cd_off = u32le(&tail, pos + 16) as u64;
    if cd_off == 0xFFFF_FFFF {
        return Err("ZIP64 not supported".into());
    }
    let cd = range(c, url, format!("bytes={}-{}", cd_off, cd_off + cd_size - 1)).await?;
    let mut out = Vec::new();
    let mut i = 0;
    while i + 46 <= cd.len() && u32le(&cd, i) == 0x02014b50 {
        let n = u16le(&cd, i + 28) as usize;
        let x = u16le(&cd, i + 30) as usize;
        let k = u16le(&cd, i + 32) as usize;
        out.push(CdEntry {
            method: u16le(&cd, i + 10),
            csize: u32le(&cd, i + 20) as u64,
            usize_: u32le(&cd, i + 24) as u64,
            offset: u32le(&cd, i + 42) as u64,
            name: String::from_utf8_lossy(&cd[i + 46..i + 46 + n]).to_string(),
        });
        i += 46 + n + x + k;
    }
    Ok(out)
}

async fn entry_bytes(c: &reqwest::Client, url: &str, e: &CdEntry) -> Result<Vec<u8>, String> {
    // Local header is 30 bytes + name + extra; extra may differ from the CD's, so over-read.
    let raw = range(c, url, format!("bytes={}-{}", e.offset, e.offset + 30 + 1024 + e.csize)).await?;
    if raw.len() < 30 || u32le(&raw, 0) != 0x04034b50 {
        return Err("corrupt ZIP entry".into());
    }
    let start = 30 + u16le(&raw, 26) as usize + u16le(&raw, 28) as usize;
    let data = raw.get(start..start + e.csize as usize).ok_or("short read")?;
    match e.method {
        0 => Ok(data.to_vec()),
        8 => {
            let mut out = Vec::with_capacity(e.usize_ as usize);
            DeflateDecoder::new(data).read_to_end(&mut out).map_err(|e| e.to_string())?;
            Ok(out)
        }
        m => Err(format!("compression {m} not supported")),
    }
}

/// Name, bundle ID, versions, min iOS and icon of one remote IPA (cached).
pub async fn inspect(base: &str, item: &str, name: &str) -> Result<Value, String> {
    let key = format!("ia:{item}/{name}");
    if let Some(v) = archive::cache_get(&key) {
        return Ok(v);
    }
    let c = client()?;
    let url = file_url(base, name);
    let cd = central_dir(&c, &url).await?;
    let info_e = cd
        .iter()
        .find(|e| e.name.starts_with("Payload/") && e.name.ends_with(".app/Info.plist") && e.name.matches('/').count() == 2)
        .ok_or("no Info.plist")?;
    let app_dir = info_e.name.trim_end_matches("Info.plist").to_string();
    let info: plist::Dictionary = plist::from_bytes(&entry_bytes(&c, &url, info_e).await?).map_err(|e| e.to_string())?;
    let s = |k: &str| info.get(k).and_then(|v| v.as_string()).map(String::from);
    let mut name_out = s("CFBundleDisplayName").or(s("CFBundleName"));
    if let Some(m) = cd.iter().find(|e| e.name == "iTunesMetadata.plist") {
        if let Ok(b) = entry_bytes(&c, &url, m).await {
            if let Ok(d) = plist::from_bytes::<plist::Dictionary>(&b) {
                if let Some(n) = d.get("itemName").and_then(|v| v.as_string()) {
                    name_out = Some(n.to_string());
                }
            }
        }
    }
    // Icon: largest PNG in the .app root whose name starts with a declared icon name (or "Icon").
    let mut bases: Vec<String> = vec!["Icon".into(), "AppIcon".into()];
    for k in ["CFBundleIconFile"] {
        if let Some(v) = s(k) {
            bases.push(v.trim_end_matches(".png").to_string());
        }
    }
    if let Some(arr) = info.get("CFBundleIconFiles").and_then(|v| v.as_array()) {
        bases.extend(arr.iter().filter_map(|v| v.as_string()).map(|v| v.trim_end_matches(".png").to_string()));
    }
    let icon_e = cd
        .iter()
        .filter(|e| {
            e.name.starts_with(&app_dir)
                && e.name[app_dir.len()..].matches('/').count() == 0
                && e.name.to_lowercase().ends_with(".png")
                && bases.iter().any(|b| e.name[app_dir.len()..].starts_with(b.as_str()))
        })
        .max_by_key(|e| e.usize_);
    let mut icon_path = None;
    if let Some(ie) = icon_e {
        if let Ok(png) = entry_bytes(&c, &url, ie).await {
            use sha2::Digest;
            let h = hex::encode(sha2::Sha256::digest(format!("{item}/{name}").as_bytes()))[..16].to_string();
            let p = archive::root().join("icons").join(format!("ia-{h}.png"));
            if std::fs::write(&p, crate::cgbi::normalize(png)).is_ok() {
                icon_path = Some(p.to_string_lossy().to_string());
            }
        }
    }
    let v = json!({
        "file": name,
        "name": name_out.unwrap_or_else(|| name.trim_end_matches(".ipa").to_string()),
        "bundleId": s("CFBundleIdentifier").unwrap_or_default(),
        "version": s("CFBundleShortVersionString").unwrap_or_default(),
        "build": s("CFBundleVersion").unwrap_or_default(),
        "minIos": s("MinimumOSVersion").unwrap_or_default(),
        "iconPath": icon_path,
    });
    archive::cache_set(&key, v.clone());
    Ok(v)
}


/// Download one file into the archive, emitting `ia-progress {file, bytes, total}`.
pub async fn download(app: &AppHandle, base: &str, item: &str, name: &str) -> Result<ArchiveEntry, String> {
    use std::io::Write;
    let c = reqwest::Client::builder().user_agent("iPacked").build().map_err(|e| e.to_string())?;
    let mut r = c.get(file_url(base, name)).send().await.map_err(|e| e.to_string())?;
    if !r.status().is_success() {
        return Err(format!("HTTP {}", r.status()));
    }
    let total = r.content_length().unwrap_or(0);
    let tmp_dir = archive::root().join(".ia");
    std::fs::create_dir_all(&tmp_dir).map_err(|e| e.to_string())?;
    let tmp: PathBuf = tmp_dir.join(format!("{}.part", name.replace('/', "_")));
    let mut f = std::fs::File::create(&tmp).map_err(|e| e.to_string())?;
    let mut got = 0u64;
    let mut last = std::time::Instant::now();
    loop {
        let chunk = match r.chunk().await {
            Ok(Some(c)) => c,
            Ok(None) => break,
            Err(e) => {
                let _ = std::fs::remove_file(&tmp);
                return Err(e.to_string());
            }
        };
        f.write_all(&chunk).map_err(|e| e.to_string())?;
        got += chunk.len() as u64;
        if last.elapsed().as_millis() > 250 {
            last = std::time::Instant::now();
            let _ = app.emit("ia-progress", json!({ "file": name, "bytes": got, "total": total }));
        }
    }
    drop(f);
    let _ = app.emit("ia-progress", json!({ "file": name, "bytes": got, "total": total, "stage": "analyzing" }));
    let source = format!("Archive.org · {item}");
    let key = format!("ia:{item}/{name}");
    tokio::task::spawn_blocking(move || {
        let mut e = archive::analyze(&tmp, &source)?;
        if e.icon_path.is_none() {
            if let Some(src) = archive::cache_get(&key).and_then(|v| v["iconPath"].as_str().map(String::from)) {
                let dst = archive::root().join("icons").join(format!("{}.png", e.sha256));
                if std::fs::copy(&src, &dst).is_ok() {
                    e.icon_path = Some(dst.to_string_lossy().to_string());
                }
            }
        }
        let dest = archive::named_dest(&e, "ipa");
        std::fs::rename(&tmp, &dest).or_else(|_| std::fs::copy(&tmp, &dest).map(|_| ())).map_err(|e| e.to_string())?;
        let _ = std::fs::remove_file(&tmp);
        e.file_path = dest.to_string_lossy().to_string();
        archive::upsert(e)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    #[test]
    #[ignore]
    fn live() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let l = super::list(&std::env::var("IPACKED_IA_ITEM").unwrap()).await.unwrap();
            let files = l["files"].as_array().unwrap();
            println!("item={} files={}", l["item"], files.len());
            for f in files.iter().take(3) {
                let n = f["name"].as_str().unwrap();
                let t = std::time::Instant::now();
                let v = super::inspect(l["base"].as_str().unwrap(), l["item"].as_str().unwrap(), n).await;
                println!("{n} -> {:?} ({:?})", v, t.elapsed());
            }
        });
    }
}
