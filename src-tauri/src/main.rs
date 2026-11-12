#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod archive;
mod blobs;
mod cgbi;
mod device;
mod ipatool;
mod ia;
mod devlog;
mod mac;
mod tools;
mod ssh;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use archive::ArchiveEntry;
use serde_json::{Value, json};
use tauri::{AppHandle, Emitter, State};

struct AppState {
    tfa: device::TfaSlot,
}

fn a(v: impl Into<String>) -> String {
    v.into()
}

// ---------------- App Store (ipatool) ----------------

#[tauri::command]
async fn store_info() -> Result<Value, String> {
    match ipatool::run(&[a("auth"), a("info")]).await {
        Ok(lines) => {
            let mut v = ipatool::find(&lines, "email").cloned().unwrap_or(Value::Null);
            if let Some(sf) = v["storeFront"].as_str().map(str::to_string) {
                v["country"] = json!(storefront_country(&sf));
            }
            Ok(v)
        }
        Err(_) => Ok(Value::Null),
    }
}

/// Two-letter country of an App Store storefront id ("143480-1,32" → "tr").
fn storefront_country(sf: &str) -> String {
    let id = sf.split(['-', ',']).next().unwrap_or("");
    const T: [(&str, &str); 30] = [
        ("143441", "us"), ("143480", "tr"), ("143444", "gb"), ("143443", "de"), ("143442", "fr"), ("143462", "jp"),
        ("143463", "hk"), ("143470", "tw"), ("143465", "cn"), ("143466", "kr"), ("143455", "ca"), ("143460", "au"),
        ("143450", "it"), ("143454", "es"), ("143469", "ru"), ("143467", "in"), ("143503", "br"), ("143468", "mx"),
        ("143452", "nl"), ("143456", "se"), ("143458", "dk"), ("143447", "fi"), ("143449", "ie"), ("143461", "nz"),
        ("143464", "sg"), ("143475", "th"), ("143471", "vn"), ("143476", "id"), ("143474", "ph"), ("143561", "ng"),
    ];
    T.iter().find(|(k, _)| *k == id).map(|(_, c)| c.to_string()).unwrap_or_default()
}

/// Returns `{status: "ok", ...}` or `{status: "need2fa"}`.
#[tauri::command]
async fn store_login(email: String, password: String, code: Option<String>) -> Result<Value, String> {
    let mut args = vec![a("auth"), a("login"), a("-e"), email, a("-p"), password];
    if let Some(c) = code.filter(|c| !c.trim().is_empty()) {
        args.push(a("--auth-code"));
        args.push(c.trim().to_string());
    }
    let lines = ipatool::run(&args).await?;
    if let Some(ok) = ipatool::find(&lines, "email") {
        let mut v = ok.clone();
        v["status"] = json!("ok");
        return Ok(v);
    }
    let needs = lines.iter().any(|l| {
        l.get("message").and_then(Value::as_str).map(|m| m.contains("2FA")).unwrap_or(false)
    });
    let had_code = args.iter().any(|x| x == "--auth-code");
    let unverified = lines.iter().any(|l| {
        l.get("error").and_then(Value::as_str).map(|m| m.contains("did not complete verification")).unwrap_or(false)
    });
    if unverified || (needs && had_code) {
        Err("Wrong Apple ID email or password, or the verification code expired. Check them and try again.".into())
    } else if needs {
        Ok(json!({ "status": "need2fa" }))
    } else {
        Err(format!("Sign-in failed: {}", serde_json::to_string(&lines).unwrap_or_default()))
    }
}

/// Whether ipatool's one-time Apple framework cache (needed for login) exists.
#[tauri::command]
fn store_assets_ready() -> bool {
    let home = std::env::var("HOME").unwrap_or_default();
    std::fs::read_dir(PathBuf::from(home).join("Library/Caches/ipatool/sap"))
        .map(|mut d| d.any(|e| e.map(|e| e.path().is_dir()).unwrap_or(false)))
        .unwrap_or(false)
}

#[tauri::command]
async fn store_logout() -> Result<(), String> {
    ipatool::run(&[a("auth"), a("revoke")]).await.map(|_| ())
}

/// Every app owned by the account (all pages). Emits `purchases-progress`
/// {loaded, total} per page and caches the result for an instant next start.
#[tauri::command]
async fn store_purchases(app: AppHandle) -> Result<Vec<Value>, String> {
    // The store may be switched while pages load: cache under the one we started with.
    let plat = ipatool::platform();
    let mut all = Vec::new();
    for page in 1..=50 {
        let lines = ipatool::run(&[
            a("list-purchases"),
            a("-l"),
            a("100"),
            a("-p"),
            page.to_string(),
        ])
        .await?;
        let Some(res) = ipatool::find(&lines, "apps") else { break };
        let apps = res["apps"].as_array().cloned().unwrap_or_default();
        let total = res["totalCount"].as_u64().unwrap_or(0) as usize;
        let n = apps.len();
        all.extend(apps);
        let _ = app.emit("purchases-progress", json!({ "loaded": all.len(), "total": total }));
        if n == 0 || all.len() >= total {
            break;
        }
    }
    if plat == ipatool::platform() {
        archive::cache_set(&format!("purchases:{plat}"), json!(all));
    }
    Ok(all)
}

/// Last full purchase list (for showing the library immediately).
#[tauri::command]
fn store_purchases_cached() -> Vec<Value> {
    archive::cache_get(&format!("purchases:{}", ipatool::platform()))
        .and_then(|v| v.as_array().cloned())
        .unwrap_or_default()
}

#[tauri::command]
fn store_set_platform(platform: String) {
    ipatool::set_platform(&platform);
}

/// Obtain a license (required before downloading, even for free apps).
#[tauri::command]
async fn store_purchase(app_id: i64) -> Result<(), String> {
    match ipatool::run(&[a("purchase"), a("-i"), app_id.to_string()]).await {
        Ok(_) => Ok(()),
        Err(e) if e.to_lowercase().contains("already") => Ok(()),
        Err(e) if e.contains("Account Not In This Store") => Err("This app is sold only in another country's App Store. Apple gives licenses only to Apple IDs from that country, so your account can't get it. Sign in with an Apple ID from that country (a free one works for free apps).".into()),
        Err(e) if e.contains("temporarily unavailable") => Err("Apple no longer sells this app (it was removed from the App Store), so no new license can be taken. Only Apple IDs that got it before it was removed can still download it.".into()),
        Err(e) => Err(e),
    }
}

/// Every app by a developer on every platform (iTunes lookup by artistId).
/// Each app gets `platform`: ios / macos / tvos / watchos (an app may appear
/// under several, e.g. an iPhone app with a Watch app).
#[tauri::command]
async fn artist_apps(artist_id: i64, country: Option<String>) -> Result<Vec<Value>, String> {
    let mut out: Vec<Value> = Vec::new();
    let mut countries: Vec<String> = vec![String::new()];
    if let Some(c) = country.filter(|c| c.len() == 2) {
        countries.insert(0, format!("&country={}", c.to_lowercase()));
    }
    countries.push("&country=tr".into());
    for c in countries {
        for entity in ["software", "iPadSoftware", "macSoftware"] {
            let url = format!("https://itunes.apple.com/lookup?id={artist_id}&entity={entity}&limit=200{c}");
            let Ok(r) = reqwest::get(&url).await else { continue };
            let body: Value = r.json().await.unwrap_or_default();
            for r in body["results"].as_array().cloned().unwrap_or_default() {
                if r["wrapperType"] != "software" {
                    continue;
                }
                let devs: Vec<String> = r["supportedDevices"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default();
                let mut plats: Vec<&str> = Vec::new();
                if r["kind"] == "mac-software" {
                    plats.push("macos");
                } else {
                    if devs.is_empty() || devs.iter().any(|d| d.starts_with("iPhone") || d.starts_with("iPad") || d.starts_with("iPod")) {
                        plats.push("ios");
                    }
                    if devs.iter().any(|d| d.starts_with("AppleTV")) {
                        plats.push("tvos");
                    }
                    if devs.iter().any(|d| d.starts_with("Watch")) {
                        plats.push("watchos");
                    }
                    if devs.iter().any(|d| d.starts_with("Vision") || d.starts_with("RealityDevice")) {
                        plats.push("visionos");
                    }
                }
                for pl in plats {
                    if out.iter().any(|x: &Value| x["id"] == r["trackId"] && x["platform"] == pl) {
                        continue;
                    }
                    out.push(json!({
                        "id": r["trackId"], "bundleID": r["bundleId"], "name": r["trackName"],
                        "version": r["version"], "price": r["price"], "artwork": r["artworkUrl100"],
                        "platform": pl,
                    }));
                }
            }
        }
        if !out.is_empty() {
            break;
        }
    }
    Ok(out)
}

/// Which platforms an app ID has (universal purchases share one ID), read from
/// its App Store page ("Requires iOS 16.5", "Requires tvOS 16.5", …).
/// `{ "ios": "16.5", "macos": "15.0", "tvos": …, "watchos": …, "visionos": … }`.
#[tauri::command]
async fn app_platforms(app_id: i64, country: Option<String>) -> Result<Value, String> {
    let key = format!("plats:{app_id}");
    let now = chrono::Utc::now().timestamp();
    if let Some(v) = archive::cache_get(&key) {
        if now - v["t"].as_i64().unwrap_or(0) < 7 * 86_400 {
            return Ok(v["p"].clone());
        }
    }
    let cc = country.filter(|c| c.len() == 2).unwrap_or_else(|| "us".into());
    let client = reqwest::Client::builder()
        .user_agent("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.0 Safari/605.1.15")
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|e| e.to_string())?;
    let mut html = String::new();
    for c in [cc.as_str(), "us"] {
        if let Ok(r) = client.get(format!("https://apps.apple.com/{c}/app/id{app_id}")).send().await {
            if r.status().is_success() {
                html = r.text().await.unwrap_or_default();
                if !html.is_empty() {
                    break;
                }
            }
        }
    }
    let mut out = serde_json::Map::new();
    for (os, key) in [("iOS", "ios"), ("iPadOS", "ios"), ("macOS", "macos"), ("tvOS", "tvos"), ("watchOS", "watchos"), ("visionOS", "visionos")] {
        let needle = format!("Requires {os} ");
        if let Some(i) = html.find(&needle) {
            let v: String = html[i + needle.len()..].chars().take_while(|c| c.is_ascii_digit() || *c == '.').collect();
            out.entry(key.to_string()).or_insert(json!(v.trim_end_matches('.')));
        }
    }
    let v = Value::Object(out);
    if !html.is_empty() {
        archive::cache_set(&key, json!({ "t": now, "p": v }));
    }
    Ok(v)
}

/// Version and release date for Settings → About.
#[tauri::command]
fn app_info() -> Value {
    json!({ "version": env!("CARGO_PKG_VERSION"), "released": "2026-11-12" })
}

#[tauri::command]
fn open_url(url: String) {
    if url.starts_with("https://") || url.starts_with("macappstore://") || url.starts_with("itms-apps://") {
        #[cfg(target_os = "macos")]
        let _ = std::process::Command::new("open").arg(url).spawn();
        #[cfg(windows)]
        let _ = std::process::Command::new("explorer").arg(url).spawn();
    }
}

/// Best-effort App Store page for an archived IPA (by app ID, then bundle ID).
#[tauri::command]
async fn find_store_page(app_id: Option<i64>, bundle_id: String) -> Result<Value, String> {
    if let Some(id) = app_id {
        let l = app_lookup(id).await?;
        if l["found"] == true {
            return Ok(l);
        }
    }
    for country in LOOKUP_COUNTRIES {
        let url = format!("https://itunes.apple.com/lookup?bundleId={bundle_id}{}", if country.is_empty() { String::new() } else { format!("&country={country}") });
        let body: Value = reqwest::get(&url).await.map_err(|e| e.to_string())?.json().await.map_err(|e| e.to_string())?;
        if let Some(r) = body["results"].get(0) {
            if let Some(id) = r["trackId"].as_i64() {
                return app_lookup(id).await;
            }
        }
    }
    Ok(json!({ "found": false }))
}

// ---------------- SHSH blobs ----------------

#[tauri::command]
fn blobs_state() -> Value {
    json!({
        "settings": blobs::load(),
        "files": blobs::scan(),
        "root": blobs::root().to_string_lossy(),
    })
}

#[tauri::command]
fn blobs_set_sources(sources: Vec<String>) -> Result<(), String> {
    let mut s = blobs::load();
    s.sources = sources;
    blobs::save(&s)
}

#[tauri::command]
fn blobs_device_upsert(device: blobs::BlobDevice, old_ecid: Option<String>) -> Result<blobs::BlobDevice, String> {
    let mut d = device;
    d.ecid = blobs::ecid_dec(&d.ecid).ok_or("Invalid ECID (enter decimal or 0x… hex)")?;
    d.board_config = d.board_config.trim().to_lowercase();
    if d.name.trim().is_empty() {
        d.name = d.product_type.clone();
    }
    let mut s = blobs::load();
    let key = old_ecid.and_then(|e| blobs::ecid_dec(&e)).unwrap_or(d.ecid.clone());
    // Renaming moves the device's folder along with it.
    if let Some(old) = s.devices.iter().find(|x| x.ecid == key).cloned() {
        if old.name != d.name || old.ecid != d.ecid {
            let from = blobs::device_folder(&old);
            let to = blobs::root().join(blobs::device_folder(&d).file_name().unwrap());
            if from != to {
                let _ = std::fs::remove_dir(&to);
                std::fs::rename(&from, &to).map_err(|e| e.to_string())?;
            }
        }
    }
    s.devices.retain(|x| x.ecid != key && x.ecid != d.ecid);
    s.devices.push(d.clone());
    blobs::save(&s)?;
    blobs::device_folder(&d);
    Ok(d)
}

#[tauri::command]
fn blobs_device_remove(ecid: String) -> Result<(), String> {
    let mut s = blobs::load();
    s.devices.retain(|x| x.ecid != ecid);
    blobs::save(&s)
}

#[tauri::command]
async fn blobs_read_connected(udid: String) -> Result<blobs::BlobDevice, String> {
    blobs::read_connected(&udid).await
}

/// Read ApNonce + generator from a connected device (recovery round trip).
#[tauri::command]
async fn blobs_read_apnonce(app: tauri::AppHandle, udid: String) -> Result<Value, String> {
    let (apnonce, generator) = blobs::read_apnonce(&udid, |m| {
        devlog::log(format!("apnonce: {m}"));
        let _ = app.emit("apnonce-progress", m);
    })
    .await?;
    Ok(json!({ "apnonce": apnonce, "generator": generator }))
}

/// AppleDB info for builds only known from saved blob files (betas etc.).
#[tauri::command]
async fn blobs_build_info(product_type: String, builds: Vec<String>) -> Result<Vec<Value>, String> {
    let futs = builds.iter().map(|b| blobs::appledb_build(&product_type, b));
    Ok(futures::future::join_all(futs).await.into_iter().flatten().collect())
}

#[tauri::command]
async fn blobs_firmwares(product_type: String) -> Result<Value, String> {
    blobs::firmwares(&product_type).await
}

#[tauri::command]
async fn blobs_save(ecid: String, version: String, build: String, ota: Option<bool>, beta: Option<bool>, url: Option<String>) -> Result<String, String> {
    let s = blobs::load();
    let d = s.devices.iter().find(|x| x.ecid == ecid).ok_or("Device not found")?.clone();
    blobs::save_blob(&d, &version, &build, ota.unwrap_or(false), beta.unwrap_or(false), url).await
}

#[tauri::command]
fn blobs_collect() -> Result<usize, String> {
    blobs::collect()
}

#[tauri::command]
fn blobs_folder(ecid: String) -> Result<String, String> {
    let s = blobs::load();
    let d = s.devices.iter().find(|x| x.ecid == ecid).ok_or("Device not found")?;
    Ok(blobs::device_folder(d).to_string_lossy().to_string())
}

// ---------------- Mac FairPlay ----------------

#[tauri::command]
fn mac_ios_apps() -> Vec<Value> {
    mac::ios_apps()
}

#[tauri::command]
async fn mac_decrypt(app: AppHandle, path: String) -> Result<ArchiveEntry, String> {
    tokio::task::spawn_blocking(move || {
        let tmp = archive::root().join(".exports");
        std::fs::create_dir_all(&tmp).ok();
        let out = tmp.join("mac-decrypt.ipa");
        let n = mac::decrypt(std::path::Path::new(&path), &out, |l| {
            let _ = app.emit("export-log", json!({ "line": l }));
        })?;
        if n == 0 {
            let _ = app.emit("export-log", json!({ "line": "No encrypted binary found; IPA repacked as is." }));
        }
        let mut e = archive::analyze(&out, "Mac FairPlay decryption (mremap_encrypted)")?;
        let dest = archive::named_dest(&e, "ipa");
        std::fs::rename(&out, &dest).map_err(|e| e.to_string())?;
        e.file_path = dest.to_string_lossy().to_string();
        archive::upsert(e)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn store_search(term: String, country: Option<String>, platforms: Option<Vec<String>>) -> Result<Vec<Value>, String> {
    let mut base = vec![a("search"), term, a("-l"), a("50")];
    if let Some(c) = country.filter(|c| c.len() == 2) {
        base.extend([a("--country"), c.to_lowercase()]);
    }
    // "All" store: search every platform at once and merge (iPhone & iPad first).
    let plats = platforms.unwrap_or_else(|| vec![String::new()]);
    let runs = plats.iter().map(|p| {
        let mut args = base.clone();
        if !p.is_empty() {
            args.extend([a("--platform"), p.clone()]);
        }
        async move { ipatool::run(&args).await }
    });
    let mut out: Vec<Value> = Vec::new();
    let mut first_err = None;
    for r in futures::future::join_all(runs).await {
        match r {
            Ok(lines) => {
                for app in ipatool::find(&lines, "apps").and_then(|v| v["apps"].as_array().cloned()).unwrap_or_default() {
                    if !out.iter().any(|x| x["id"] == app["id"]) {
                        out.push(app);
                    }
                }
            }
            Err(e) => first_err = first_err.or(Some(e)),
        }
    }
    match first_err {
        Some(e) if out.is_empty() => Err(e),
        _ => Ok(out),
    }
}

#[tauri::command]
async fn app_versions(app_id: i64, refresh: bool) -> Result<Vec<String>, String> {
    let key = format!("versions:{}{app_id}", ipatool::platform());
    if !refresh {
        if let Some(v) = archive::cache_get(&key) {
            return serde_json::from_value(v).map_err(|e| e.to_string());
        }
    }
    let lines = ipatool::run(&[a("list-versions"), a("-i"), app_id.to_string()]).await?;
    let ids: Vec<String> = ipatool::find(&lines, "externalVersionIdentifiers")
        .and_then(|v| v["externalVersionIdentifiers"].as_array().cloned())
        .unwrap_or_default()
        .iter()
        .map(|v| match v {
            Value::String(s) => s.clone(),
            other => other.to_string(),
        })
        .collect();
    archive::cache_set(&key, json!(ids));
    Ok(ids)
}

/// Resolve many external version IDs in one ipatool process; emits `version-meta`
/// events `{appId, versionId, displayVersion, releaseDate}` (or `error`) as they arrive.
#[tauri::command]
async fn versions_metadata(app: AppHandle, app_id: i64, version_ids: Vec<String>) -> Result<(), String> {
    let mut todo = Vec::new();
    for v in version_ids {
        match archive::cache_get(&format!("meta2:{}{app_id}:{v}", ipatool::platform())) {
            Some(m) => {
                let mut m = m.clone();
                m["appId"] = json!(app_id);
                m["versionId"] = json!(v);
                let _ = app.emit("version-meta", m);
            }
            None => todo.push(v),
        }
    }
    if todo.is_empty() {
        return Ok(());
    }
    let args = vec![
        a("get-versions-metadata"),
        a("-i"),
        app_id.to_string(),
        a("--external-version-ids"),
        todo.join(","),
        a("--concurrency"),
        a("2"),
    ];
    ipatool::run_stream(&args, |l| {
        let Some(vid) = l.get("externalVersionID").and_then(Value::as_str) else { return };
        let mut ev = json!({ "appId": app_id, "versionId": vid });
        if let Some(e) = l.get("versionError") {
            ev["error"] = e.clone();
        } else {
            let m = json!({ "displayVersion": l["displayVersion"], "releaseDate": l["releaseDate"] });
            archive::cache_set(&format!("meta2:{}{app_id}:{vid}", ipatool::platform()), m.clone());
            ev["displayVersion"] = m["displayVersion"].clone();
            ev["releaseDate"] = m["releaseDate"].clone();
        }
        let _ = app.emit("version-meta", ev);
    })
    .await
}

/// Apps the user added by ID / link (e.g. delisted purchases not returned by Apple).
#[tauri::command]
fn manual_apps() -> Value {
    archive::cache_get("manual-apps").unwrap_or(json!([]))
}

#[tauri::command]
async fn manual_app_add(input: String) -> Result<Value, String> {
    // Accept a bare ID or an apps.apple.com link (".../id123456789").
    let digits: String = match input.rfind("id") {
        Some(i) if input.contains('/') => input[i + 2..].chars().take_while(|c| c.is_ascii_digit()).collect(),
        _ => input.chars().filter(|c| c.is_ascii_digit()).collect(),
    };
    let id: i64 = digits.parse().map_err(|_| "Enter a valid App ID or App Store link")?;
    let l = app_lookup(id).await.unwrap_or(Value::Null);
    let item = json!({
        "id": id,
        "bundleID": l.get("bundleId").cloned().unwrap_or(json!("")),
        "name": l.get("name").and_then(Value::as_str).map(str::to_string).unwrap_or(format!("App {id}")),
        "version": l.get("currentVersion").cloned().unwrap_or(json!("")),
        "manual": true,
    });
    let mut list = manual_apps().as_array().cloned().unwrap_or_default();
    list.retain(|x| x["id"].as_i64() != Some(id));
    list.push(item.clone());
    archive::cache_set("manual-apps", json!(list));
    Ok(item)
}

#[tauri::command]
fn manual_app_remove(id: i64) {
    let mut list = manual_apps().as_array().cloned().unwrap_or_default();
    list.retain(|x| x["id"].as_i64() != Some(id));
    archive::cache_set("manual-apps", json!(list));
}

/// `{displayVersion, releaseDate}` for one external version id (cached forever).
#[tauri::command]
async fn version_metadata(app_id: i64, version_id: String) -> Result<Value, String> {
    let key = format!("meta2:{}{app_id}:{version_id}", ipatool::platform());
    if let Some(v) = archive::cache_get(&key) {
        return Ok(v);
    }
    let lines = ipatool::run(&[
        a("get-version-metadata"),
        a("-i"),
        app_id.to_string(),
        a("--external-version-id"),
        version_id.clone(),
    ])
    .await?;
    let m = ipatool::find(&lines, "displayVersion").ok_or("Could not get metadata")?;
    let v = json!({ "displayVersion": m["displayVersion"], "releaseDate": m["releaseDate"] });
    archive::cache_set(&key, v.clone());
    Ok(v)
}

/// Running downloads: version id → kill switch (true = keep partial file for resume).
fn downloads() -> &'static std::sync::Mutex<std::collections::HashMap<String, tokio::sync::oneshot::Sender<bool>>> {
    static D: std::sync::OnceLock<std::sync::Mutex<std::collections::HashMap<String, tokio::sync::oneshot::Sender<bool>>>> =
        std::sync::OnceLock::new();
    D.get_or_init(Default::default)
}

/// Every file ipatool may leave for `out` (.tmp, .ipatool-encrypted/-decrypted, -parts).
fn remove_partials(out: &std::path::Path) {
    let Some(stem) = out.file_name().map(|n| n.to_string_lossy().to_string()) else { return };
    let Some(dir) = out.parent() else { return };
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        if e.file_name().to_string_lossy().starts_with(&stem) {
            let p = e.path();
            let _ = if p.is_dir() { std::fs::remove_dir_all(&p) } else { std::fs::remove_file(&p) };
        }
    }
}

/// Stop a running download. `keep` = pause (the partial .tmp stays and the
/// next download of the same version resumes from it); otherwise cancel.
#[tauri::command]
fn download_stop(version_id: String, keep: bool) -> bool {
    match downloads().lock().unwrap().remove(&version_id) {
        Some(tx) => tx.send(keep).is_ok(),
        None => false,
    }
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
async fn download_version(
    app: AppHandle,
    app_id: i64,
    version_id: String,
    display_version: Option<String>,
    release_date: Option<String>,
    purchase: bool,
    bundle_id: Option<String>,
    name: Option<String>,
    platform: Option<String>,
) -> Result<ArchiveEntry, String> {
    use tokio::io::{AsyncBufReadExt, BufReader};
    let email = store_info().await?.get("email").and_then(Value::as_str).unwrap_or("").to_string();
    let tmp_dir = archive::root().join(".downloads");
    std::fs::create_dir_all(&tmp_dir).ok();
    let mac = ipatool::platform() == "macos";
    let out = tmp_dir.join(format!("{app_id}_{version_id}.{}", if mac { "pkg" } else { "ipa" }));

    let mut args = vec![
        a("download"),
        a("-i"),
        app_id.to_string(),
        a("--external-version-id"),
        version_id.clone(),
        a("-o"),
        out.to_string_lossy().to_string(),
    ];
    if purchase {
        args.push(a("--purchase"));
    }
    // The store platform the download was queued for (the store may have been switched since).
    if let Some(p) = platform.filter(|p| !p.is_empty()) {
        args.extend([a("--platform"), p]);
    }
    // One-shot process (not the shared server) so it can be stopped.
    let mut child = ipatool::spawn(&args, &[("IPACKED_PROGRESS", "1")])?;
    let (tx, mut rx) = tokio::sync::oneshot::channel::<bool>();
    downloads().lock().unwrap().insert(version_id.clone(), tx);
    let _ = app.emit("download-progress", json!({ "versionId": version_id, "stage": "started", "appId": app_id, "name": name, "version": display_version }));

    let mut stderr = BufReader::new(child.stderr.take().unwrap()).lines();
    let mut stdout = BufReader::new(child.stdout.take().unwrap()).lines();
    let mut lines: Vec<Value> = Vec::new();
    let (mut o_done, mut e_done) = (false, false);
    let mut stopped: Option<bool> = None;
    while !(o_done && e_done) {
        tokio::select! {
            k = &mut rx, if stopped.is_none() => {
                stopped = Some(k.unwrap_or(false));
                let _ = child.kill().await;
            }
            l = stdout.next_line(), if !o_done => match l {
                Ok(Some(l)) => if let Ok(v) = serde_json::from_str::<Value>(&l) { lines.push(v) },
                _ => o_done = true,
            },
            l = stderr.next_line(), if !e_done => match l {
                Ok(Some(l)) => if let Ok(v) = serde_json::from_str::<Value>(&l) {
                    if let Some(b) = v.get("ipacked_progress") {
                        let _ = app.emit("download-progress", json!({ "versionId": version_id, "bytes": b, "total": v["total"] }));
                    } else {
                        lines.push(v);
                    }
                },
                _ => e_done = true,
            },
        }
    }
    let _ = child.wait().await;
    downloads().lock().unwrap().remove(&version_id);
    if let Some(keep) = stopped {
        if !keep {
            remove_partials(&out);
        }
        return Err(if keep { "PAUSED".into() } else { "CANCELLED".into() });
    }
    if let Some(e) = lines.iter().find(|v| v.get("level").and_then(Value::as_str) == Some("error")) {
        remove_partials(&out);
        return Err(e.get("error").and_then(Value::as_str).unwrap_or("ipatool error").to_string());
    }
    let path = ipatool::find(&lines, "output")
        .and_then(|v| v["output"].as_str())
        .map(PathBuf::from)
        .unwrap_or(out.clone());

    let _ = app.emit("download-progress", json!({ "versionId": version_id, "stage": "analyzing" }));
    let source = format!("App Store (ipatool) · {email}");
    let is_pkg = path.extension().map(|e| e == "pkg").unwrap_or(false);
    let mut entry = tokio::task::spawn_blocking({
        let path = path.clone();
        let (bid, nm, ver) = (
            bundle_id.clone().unwrap_or_else(|| app_id.to_string()),
            name.clone().unwrap_or_else(|| app_id.to_string()),
            display_version.clone().unwrap_or_default(),
        );
        move || if is_pkg { archive::analyze_pkg(&path, &source, &bid, &nm, &ver) } else { archive::analyze(&path, &source) }
    })
    .await
    .map_err(|e| e.to_string())??;

    entry.app_id = entry.app_id.or(Some(app_id));
    entry.external_version_id = Some(version_id.clone());
    if release_date.is_some() {
        entry.release_date = release_date;
    }
    let ver = display_version.unwrap_or_else(|| entry.version.clone());
    if entry.version.is_empty() {
        entry.version = ver.clone();
    }
    let dest = archive::named_dest(&entry, if is_pkg { "pkg" } else { "ipa" });
    std::fs::rename(&path, &dest).map_err(|e| e.to_string())?;
    remove_partials(&out);
    entry.file_path = dest.to_string_lossy().to_string();
    // Settings: a fresh FairPlay original replaces decrypted dumps of the same version.
    if archive::prefs()["replaceDumpedWithOriginal"].as_bool() == Some(true) && entry.encrypted == Some(true) {
        for d in archive::load().into_iter().filter(|d| {
            d.bundle_id == entry.bundle_id && d.version == entry.version && d.build == entry.build && d.encrypted == Some(false)
        }) {
            let _ = archive::remove(&d.id, true);
        }
    }
    archive::upsert(entry)
}

/// Current App Store artwork URL + basic info via the public iTunes lookup (cached).
#[tauri::command]
async fn app_lookup(app_id: i64) -> Result<Value, String> {
    // Cached for a day so "What's New" stays fresh.
    let key = format!("lookup3:{app_id}");
    let now = chrono::Utc::now().timestamp();
    if let Some(v) = archive::cache_get(&key) {
        if now - v["fetchedAt"].as_i64().unwrap_or(0) < 86_400 {
            return Ok(v);
        }
    }
    let mut r = Value::Null;
    let mut found_in = String::new();
    for country in LOOKUP_COUNTRIES {
        let url = format!("https://itunes.apple.com/lookup?id={app_id}{}", if country.is_empty() { String::new() } else { format!("&country={country}") });
        let body: Value = reqwest::get(&url)
            .await
            .map_err(|e| e.to_string())?
            .json()
            .await
            .map_err(|e| e.to_string())?;
        if let Some(x) = body["results"].get(0) {
            r = x.clone();
            found_in = if country.is_empty() { "us".into() } else { country.to_string() };
            break;
        }
    }
    let mut v = lookup_value(&r, now);
    v["country"] = json!(found_in);
    archive::cache_set(&key, v.clone());
    Ok(v)
}

/// Storefronts tried, in order, when an app isn't in the US store.
const LOOKUP_COUNTRIES: [&str; 30] = [
    "", "tr", "gb", "jp", "hk", "tw", "cn", "kr", "de", "fr", "ca", "au", "it", "es", "ru", "in", "br", "mx",
    "nl", "se", "dk", "fi", "ie", "nz", "sg", "th", "vn", "id", "ph", "ng",
];

fn lookup_value(r: &Value, now: i64) -> Value {
    json!({
        "found": !r.is_null(),
        "fetchedAt": now,
        "name": r["trackName"],
        "bundleId": r["bundleId"],
        "artwork": r["artworkUrl512"].as_str().or(r["artworkUrl100"].as_str()),
        "seller": r["sellerName"],
        "artistName": r["artistName"],
        "artistId": r["artistId"],
        "minimumOsVersion": r["minimumOsVersion"],
        "supportedDevices": r["supportedDevices"],
        "kind": r["kind"],
        "features": r["features"],
        "advisories": r["advisories"],
        "sellerUrl": r["sellerUrl"],
        "primaryGenreName": r["primaryGenreName"],
        "trackContentRating": r["trackContentRating"],
        "isGameCenterEnabled": r["isGameCenterEnabled"],
        "currency": r["currency"],
        "version": r["version"],
        "bundleIdLookup": r["bundleId"],
        "currentVersion": r["version"],
        "releaseDate": r["releaseDate"],
        "currentVersionReleaseDate": r["currentVersionReleaseDate"],
        "releaseNotes": r["releaseNotes"],
        "description": r["description"],
        "genres": r["genres"],
        "fileSizeBytes": r["fileSizeBytes"],
        "contentAdvisoryRating": r["contentAdvisoryRating"],
        "averageUserRating": r["averageUserRating"],
        "userRatingCount": r["userRatingCount"],
        "formattedPrice": r["formattedPrice"],
        "price": r["price"],
        "trackViewUrl": r["trackViewUrl"],
        "screenshotUrls": r["screenshotUrls"],
        "ipadScreenshotUrls": r["ipadScreenshotUrls"],
        "languageCodesISO2A": r["languageCodesISO2A"],
        "kind": r["kind"],
    })
}

/// Many lookups in a few requests (iTunes accepts up to ~200 comma-separated
/// ids). One request per app made Apple throttle the library (1000+ apps) and
/// icons stayed empty. Failed requests are not cached, so they retry later.
#[tauri::command]
async fn app_lookup_many(app_ids: Vec<i64>) -> Result<Value, String> {
    let now = chrono::Utc::now().timestamp();
    let mut out = serde_json::Map::new();
    let mut todo: Vec<i64> = Vec::new();
    for id in app_ids {
        if id <= 0 || out.contains_key(&id.to_string()) || todo.contains(&id) {
            continue;
        }
        match archive::cache_get(&format!("lookup3:{id}")) {
            Some(v) if now - v["fetchedAt"].as_i64().unwrap_or(0) < 86_400 => {
                out.insert(id.to_string(), v);
            }
            _ => todo.push(id),
        }
    }
    let client = reqwest::Client::builder().timeout(Duration::from_secs(30)).build().map_err(|e| e.to_string())?;
    for country in ["", "&country=tr", "&country=jp", "&country=gb"] {
        if todo.is_empty() {
            break;
        }
        let mut next = Vec::new();
        for chunk in todo.chunks(150) {
            let ids: Vec<String> = chunk.iter().map(|i| i.to_string()).collect();
            let url = format!("https://itunes.apple.com/lookup?id={}{country}", ids.join(","));
            let mut body: Option<Value> = None;
            for attempt in 0..3 {
                match client.get(&url).send().await {
                    Ok(r) if r.status().is_success() => {
                        body = r.json().await.ok();
                        break;
                    }
                    _ => tokio::time::sleep(Duration::from_millis(1500 * (attempt + 1))).await,
                }
            }
            let Some(body) = body else { continue }; // network trouble: leave uncached
            let mut found = std::collections::HashSet::new();
            for r in body["results"].as_array().cloned().unwrap_or_default() {
                let Some(id) = r["trackId"].as_i64() else { continue };
                let v = lookup_value(&r, now);
                archive::cache_set(&format!("lookup3:{id}"), v.clone());
                out.insert(id.to_string(), v);
                found.insert(id);
            }
            next.extend(chunk.iter().filter(|i| !found.contains(i)));
        }
        todo = next;
    }
    // Not in any storefront: delisted. Cache the miss too (a day).
    for id in todo {
        let v = lookup_value(&Value::Null, now);
        archive::cache_set(&format!("lookup3:{id}"), v.clone());
        out.insert(id.to_string(), v);
    }
    Ok(Value::Object(out))
}

// ---------------- Archive ----------------

#[tauri::command]
fn archive_list() -> Vec<ArchiveEntry> {
    archive::load()
}

#[tauri::command]
async fn archive_rescan() -> Result<archive::ScanReport, String> {
    tokio::task::spawn_blocking(archive::rescan).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn device_apple_ids(id: String, apps: Vec<(String, String)>) -> serde_json::Map<String, Value> {
    device::apple_ids(&id, apps).await
}

#[tauri::command]
async fn device_uninstall(id: String, bundle_id: String) -> Result<(), String> {
    device::uninstall(&id, &bundle_id).await
}

#[tauri::command]
fn archive_root() -> String {
    archive::root().to_string_lossy().to_string()
}

#[tauri::command]
async fn archive_import(path: String) -> Result<ArchiveEntry, String> {
    tokio::task::spawn_blocking(move || {
        let src = PathBuf::from(&path);
        let mut e = archive::analyze(&src, "Local file (imported)")?;
        let dest = archive::named_dest(&e, "ipa");
        if dest != src {
            std::fs::copy(&src, &dest).map_err(|e| e.to_string())?;
        }
        e.file_path = dest.to_string_lossy().to_string();
        archive::upsert(e)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
fn archive_remove(id: String, delete_file: bool) -> Result<(), String> {
    archive::remove(&id, delete_file)
}

#[tauri::command]
fn reveal(path: String) {
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg("-R").arg(path).spawn();
    #[cfg(windows)]
    let _ = std::process::Command::new("explorer").arg(format!("/select,{path}")).spawn();
}

// ---------------- Language files ----------------
// ~/Library/Application Support/iPacked/lang/<code>.json: {"_name": "English",
// "<Turkish source text>": "<translation>", ...}. Editable by hand or in
// Settings → Language → Edit; new files appear as new languages.

fn lang_dir() -> std::path::PathBuf {
    let d = std::path::PathBuf::from(std::env::var("HOME").unwrap_or_default()).join("Library/Application Support/iPacked/lang");
    let _ = std::fs::create_dir_all(&d);
    d
}

fn lang_file(code: &str) -> Result<std::path::PathBuf, String> {
    let ok = !code.is_empty() && code.len() <= 16 && code.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if !ok {
        return Err(format!("Invalid language code: {code}"));
    }
    Ok(lang_dir().join(format!("{code}.json")))
}

/// Every language file: {code: contents}. Unreadable files come back as {"_error": ...}.
#[tauri::command]
fn lang_all() -> Value {
    let mut m = serde_json::Map::new();
    if let Ok(rd) = std::fs::read_dir(lang_dir()) {
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().and_then(|x| x.to_str()) != Some("json") {
                continue;
            }
            let code = p.file_stem().and_then(|x| x.to_str()).unwrap_or_default().to_string();
            if code.starts_with('_') {
                continue;
            }
            let v = std::fs::read_to_string(&p)
                .map_err(|e| e.to_string())
                .and_then(|t| serde_json::from_str::<Value>(&t).map_err(|e| e.to_string()))
                .unwrap_or_else(|e| json!({ "_error": e }));
            m.insert(code, v);
        }
    }
    Value::Object(m)
}

#[tauri::command]
fn lang_write(code: String, data: Value) -> Result<(), String> {
    let p = if code == "_template" { lang_dir().join("_template.json") } else { lang_file(&code)? };
    let t = serde_json::to_string_pretty(&data).map_err(|e| e.to_string())?;
    std::fs::write(&p, t + "\n").map_err(|e| e.to_string())
}

#[tauri::command]
fn lang_reveal() {
    let d = lang_dir();
    let _ = std::process::Command::new("open").arg(d).spawn();
}

// ---------------- Devices / Impactor ----------------

#[tauri::command]
async fn devices_list() -> Result<Value, String> {
    device::list().await.map(|d| json!(d))
}

#[tauri::command]
async fn device_apps(id: String) -> Result<Vec<Value>, String> {
    device::apps(&id).await
}

#[tauri::command]
async fn device_install(app: AppHandle, id: String, ipa: String, resign: bool) -> Result<(), String> {
    // The signing path holds non-Send codesign types; run it on a blocking thread.
    tokio::task::spawn_blocking(move || {
        tauri::async_runtime::block_on(device::install(app, id, ipa, resign))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Export an installed app from the device and add it to the archive.
#[tauri::command]
async fn device_export(app: AppHandle, id: String, bundle_id: String, method: String, orig_id: Option<String>) -> Result<ArchiveEntry, String> {
    let tmp = archive::root().join(".exports");
    let (ipa, source) = tokio::task::spawn_blocking({
        let (app, tmp) = (app.clone(), tmp.clone());
        move || tauri::async_runtime::block_on(device::export(app, &id, &bundle_id, &method, tmp))
    })
    .await
    .map_err(|e| e.to_string())??;
    let _ = app.emit("export-log", json!({ "line": "Analyzing IPA…" }));
    tokio::task::spawn_blocking(move || {
        let mut e = archive::analyze(&ipa, &source)?;
        if let Some(n) = e.app_name.clone() {
            e.name = n;
        }
        // Dumped from an archived FairPlay original: carry over what the dump
        // loses (store IDs, release date, buyer) and link the two.
        let orig = orig_id.as_ref().and_then(|o| archive::load().into_iter().find(|x| &x.id == o));
        if let Some(o) = &orig {
            e.app_id = e.app_id.or(o.app_id);
            e.external_version_id = e.external_version_id.clone().or(o.external_version_id.clone());
            e.release_date = e.release_date.clone().or(o.release_date.clone());
            e.apple_id = e.apple_id.clone().or(o.apple_id.clone());
            // Keep the name shown on the phone; remember the App Store name.
            e.store_name = e.store_name.clone().or(o.store_name.clone()).or(Some(o.name.clone()));
            e.dumped_from = Some(o.sha256.clone());
        }
        let dest = archive::named_dest(&e, "ipa");
        std::fs::rename(&ipa, &dest).or_else(|_| std::fs::copy(&ipa, &dest).map(|_| ())).map_err(|e| e.to_string())?;
        let _ = std::fs::remove_file(&ipa);
        e.file_path = dest.to_string_lossy().to_string();
        let out = archive::upsert(e)?;
        if let Some(o) = orig {
            if out.encrypted == Some(false) && archive::prefs()["deleteOriginalAfterDump"].as_bool() == Some(true) {
                let _ = archive::remove(&o.id, true);
            }
        }
        Ok(out)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn ia_list(url: String) -> Result<Value, String> {
    ia::list(&url).await
}

#[tauri::command]
async fn ia_inspect(base: String, item: String, name: String) -> Result<Value, String> {
    ia::inspect(&base, &item, &name).await
}

#[tauri::command]
async fn ia_download(app: AppHandle, base: String, item: String, name: String) -> Result<ArchiveEntry, String> {
    ia::download(&app, &base, &item, &name).await
}

#[tauri::command]
fn devlog_tail(n: usize) -> Vec<String> {
    devlog::tail(n)
}

#[tauri::command]
fn devlog_path() -> String {
    devlog::path().to_string_lossy().to_string()
}

#[tauri::command]
fn devlog_note(line: String) {
    devlog::log(line);
}

#[tauri::command]
async fn device_system_apps(id: String) -> Result<Vec<Value>, String> {
    device::apps_kind(&id, "System").await
}

#[tauri::command]
fn prefs_get() -> Value {
    archive::prefs()
}

#[tauri::command]
fn prefs_set(prefs: Value) -> Result<Value, String> {
    let mut cur = archive::prefs();
    if let Some(o) = prefs.as_object() {
        for (k, v) in o {
            cur[k] = v.clone();
        }
    }
    archive::prefs_save(&cur)?;
    Ok(cur)
}

/// Example file name for the naming template (Settings preview).
#[tauri::command]
fn name_preview(template: String, entry: Option<ArchiveEntry>) -> String {
    let e = entry.unwrap_or_else(|| ArchiveEntry {
        name: "Minecraft: Dream it! Build it!".into(), app_name: Some("Minecraft".into()), store_name: Some("Minecraft: Dream it! Build it!".into()),
        version: "0.3.3".into(), build: "0.3.3.1".into(),
        bundle_id: "com.mojang.minecraftpe".into(), external_version_id: Some("9470435".into()), ..Default::default()
    });
    archive::template_name(&template, &e) + ".ipa"
}

#[tauri::command]
async fn archive_refresh_icon(id: String) -> Result<archive::ArchiveEntry, String> {
    tokio::task::spawn_blocking(move || archive::refresh_icon(&id)).await.map_err(|e| e.to_string())?
}

#[tauri::command]
async fn archive_rename_all() -> Result<usize, String> {
    tokio::task::spawn_blocking(archive::rename_all).await.map_err(|e| e.to_string())?
}

/// Open a folder itself (not its parent with it selected).
#[tauri::command]
fn open_path(path: String) {
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(path).spawn();
    #[cfg(windows)]
    let _ = std::process::Command::new("explorer").arg(path).spawn();
}

#[tauri::command]
async fn ssh_get(id: String) -> Result<ssh::SshCfg, String> {
    Ok(ssh::get(&device::udid_of(&id).await?))
}

#[tauri::command]
async fn ssh_set(id: String, cfg: ssh::SshCfg) -> Result<(), String> {
    ssh::set(&device::udid_of(&id).await?, cfg)
}

/// {installed, path, via} — whether Clutch exists on the device (/bin/clutch etc.).
#[tauri::command]
async fn clutch_status(id: String) -> Result<Value, String> {
    let conn = ssh::Conn::open(&device::udid_of(&id).await?).await?;
    let p = ssh::clutch_path(&conn).await?;
    Ok(json!({ "installed": p.is_some(), "path": p, "via": conn.label() }))
}

#[tauri::command]
async fn clutch_install(id: String, path: String) -> Result<String, String> {
    let conn = ssh::Conn::open(&device::udid_of(&id).await?).await?;
    ssh::clutch_install(&conn, PathBuf::from(path)).await
}

#[tauri::command]
async fn deb_install(app: AppHandle, id: String, path: String) -> Result<String, String> {
    let conn = ssh::Conn::open(&device::udid_of(&id).await?).await?;
    ssh::deb_install(&conn, PathBuf::from(path), |l| {
        let _ = app.emit("export-log", json!({ "line": l }));
    })
    .await
}

#[tauri::command]
async fn device_details(id: String) -> Result<Value, String> {
    device::details(&id).await
}

/// AFC file browser. mode: "afc" (Media) or "afc2" (whole filesystem, jailbreak).
#[tauri::command]
async fn afc_ls(id: String, mode: String, path: String) -> Result<Vec<Value>, String> {
    let udid = device::udid_of(&id).await?;
    let out = device::legacy(&["afc-ls", &udid, &mode, &path]).await?;
    Ok(out
        .lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split('\t').collect();
            (f.len() >= 4).then(|| {
                json!({
                    "name": f[0], "dir": f[1] == "S_IFDIR", "link": f[1] == "S_IFLNK",
                    "size": f[2].parse::<u64>().unwrap_or(0), "mtime": f[3].parse::<i64>().unwrap_or(0),
                    "target": f.get(4).copied().unwrap_or(""),
                })
            })
        })
        .collect())
}

#[tauri::command]
async fn afc_op(id: String, mode: String, op: String, a: String, b: Option<String>) -> Result<(), String> {
    let udid = device::udid_of(&id).await?;
    let cmd = match op.as_str() {
        "get" | "put" | "mkdir" | "rm" | "mv" => format!("afc-{op}"),
        _ => return Err("unknown operation".into()),
    };
    let mut args = vec![cmd.as_str(), udid.as_str(), mode.as_str(), a.as_str()];
    if let Some(b) = b.as_deref() {
        args.push(b);
    }
    device::legacy(&args).await.map(|_| ())
}

#[tauri::command]
fn tools_status() -> Value {
    tools::which_status()
}

#[tauri::command]
async fn device_icon(id: String, bundle_id: String) -> Result<String, String> {
    device::icon(&id, &bundle_id).await
}

#[tauri::command]
async fn dev_account() -> Option<String> {
    device::dev_account().await
}

#[tauri::command]
async fn dev_login(app: AppHandle, state: State<'_, AppState>, email: String, password: String) -> Result<String, String> {
    device::dev_login(app, state.tfa.clone(), email, password).await
}

#[tauri::command]
fn dev_submit_2fa(state: State<'_, AppState>, code: String) -> Result<(), String> {
    let tx = state.tfa.lock().unwrap().take().ok_or("No pending 2FA request")?;
    tx.send(code).map_err(|e| e.to_string())
}

#[tauri::command]
async fn dev_logout() -> Result<(), String> {
    device::dev_logout().await
}

/// Remove temp leftovers of earlier runs: ipatool metadata staging dirs in
/// $TMPDIR (whole .pkg downloads), partial downloads, Clutch/.deb scratch.
/// Only one iPacked runs at a time, so everything here is stale at startup.
fn clean_temp() -> u64 {
    fn size(p: &std::path::Path) -> u64 {
        if p.is_dir() {
            std::fs::read_dir(p).into_iter().flatten().flatten().map(|e| size(&e.path())).sum()
        } else {
            std::fs::symlink_metadata(p).map(|m| m.len()).unwrap_or(0)
        }
    }
    let mut freed = 0;
    let mut kill = |p: PathBuf| {
        freed += size(&p);
        let _ = if p.is_dir() { std::fs::remove_dir_all(&p) } else { std::fs::remove_file(&p) };
    };
    for e in std::fs::read_dir(std::env::temp_dir()).into_iter().flatten().flatten() {
        let n = e.file_name().to_string_lossy().to_string();
        if n.starts_with("ipatool-version-metadata-") || n.starts_with("ipacked-clutch-") || (n.starts_with("ipacked-") && n.ends_with("BuildManifest.plist")) {
            kill(e.path());
        }
    }
    for e in std::fs::read_dir(archive::root().join(".downloads")).into_iter().flatten().flatten() {
        kill(e.path());
    }
    for e in std::fs::read_dir(archive::root().join(".exports")).into_iter().flatten().flatten() {
        kill(e.path());
    }
    freed
}

/// Disk used by temp leftovers / free it now (Settings → "Geçici dosyaları temizle").
#[tauri::command]
fn temp_clean() -> u64 {
    clean_temp()
}

/// The app was called "iPast" before 0.1.19: move its folders to the new
/// name once and fix the absolute paths stored in its JSON files.
fn migrate_old_name() {
    let old = ["i", "Past"].concat();
    let home = std::path::PathBuf::from(std::env::var("HOME").unwrap_or_default());
    let mut moved_docs = false;
    for base in ["Documents", "Library/Application Support", "Library/Logs"] {
        let from = home.join(base).join(&old);
        let to = home.join(base).join("iPacked");
        if from.is_dir() && !to.exists() && std::fs::rename(&from, &to).is_ok() && base == "Documents" {
            moved_docs = true;
        }
    }
    if !moved_docs {
        return;
    }
    let docs = home.join("Documents/iPacked");
    let old_prefix = format!("/Documents/{old}/");
    for f in [docs.join("index.json"), docs.join("cache.json"), docs.join("Blobs/blobs.json"), home.join("Library/Application Support/iPacked/prefs.json")] {
        if let Ok(t) = std::fs::read_to_string(&f) {
            let n = t.replace(&old_prefix, "/Documents/iPacked/");
            if n != t {
                let _ = std::fs::write(&f, n);
            }
        }
    }
}

fn main() {
    migrate_old_name();
    let _ = rustls::crypto::ring::default_provider().install_default();
    std::thread::spawn(|| {
        let n = clean_temp();
        log::info!("startup cleanup freed {n} bytes");
    });
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            devlog::init(app.handle().clone());
            device::watch(app.handle().clone());
            devlog::log(format!("— iPacked {} started —", app.package_info().version));
            Ok(())
        })
        .manage(AppState { tfa: Arc::new(Mutex::new(None)) })
        .invoke_handler(tauri::generate_handler![
            temp_clean,
            lang_all,
            lang_write,
            lang_reveal,
            store_info,
            store_login,
            store_logout,
            store_assets_ready,
            store_purchases,
            store_purchases_cached,
            store_search,
            app_versions,
            version_metadata,
            versions_metadata,
            store_set_platform,
            store_purchase,
            artist_apps,
            open_url,
            find_store_page,
            mac_ios_apps,
            mac_decrypt,
            blobs_state,
            blobs_set_sources,
            blobs_device_upsert,
            blobs_device_remove,
            blobs_read_connected,
            blobs_read_apnonce,
            blobs_build_info,
            blobs_firmwares,
            blobs_save,
            archive_rescan,
            device_uninstall,
            device_apple_ids,
            blobs_collect,
            blobs_folder,
            manual_apps,
            manual_app_add,
            manual_app_remove,
            download_version,
            download_stop,
            app_lookup,
            app_lookup_many,
            archive_list,
            archive_root,
            prefs_get,
            devlog_tail,
            devlog_path,
            devlog_note,
            ia_list,
            ia_inspect,
            ia_download,
            device_system_apps,
            prefs_set,
            name_preview,
            archive_rename_all,
            archive_refresh_icon,
            app_platforms,
            app_info,
            open_path,
            archive_import,
            archive_remove,
            reveal,
            devices_list,
            device_apps,
            device_install,
            device_export,
            device_icon,
            tools_status,
            ssh_get,
            ssh_set,
            clutch_status,
            clutch_install,
            deb_install,
            device_details,
            afc_ls,
            afc_op,
            dev_account,
            dev_login,
            dev_submit_2fa,
            dev_logout,
        ])
        .build(tauri::generate_context!())
        .expect("iPacked failed to start")
        .run(|_, ev| {
            if let tauri::RunEvent::Exit = ev {
                ipatool::cancel_stream();
            }
        });
}
