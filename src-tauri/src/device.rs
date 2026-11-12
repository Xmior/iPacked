//! Connected iOS devices: listing, installed apps, installing IPAs (direct or
//! Impactor-style re-signed with an Apple ID).

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use idevice::IdeviceService;
use idevice::installation_proxy::InstallationProxyClient;
use idevice::usbmuxd::{Connection, UsbmuxdAddr, UsbmuxdConnection};
use idevice::utils::installation;
use plist::Value;
use plume_core::auth::{Account, TwoFactorAction, TwoFactorMethod};
use plume_core::developer::DeveloperSession;
use plume_core::{AnisetteConfiguration, CertificateIdentity};
use plume_store::AccountStore;
use plume_utils::{Device, Package, Signer, SignerMode, SignerOptions, get_device_for_id};
use serde::Serialize;
use serde_json::json;
use tauri::{AppHandle, Emitter};

use crate::tools;

fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

/// Impactor data dir (shared format with PlumeImpactor: accounts.json, anisette, certs).
pub fn data_path() -> PathBuf {
    let p = crate::archive::root().join(".impactor");
    std::fs::create_dir_all(&p).ok();
    p
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceInfo {
    id: String,
    udid: String,
    name: String,
    connection: String,
}

pub async fn list() -> Result<Vec<DeviceInfo>, String> {
    let mut mux = UsbmuxdConnection::default().await.map_err(|e| {
        format!("Could not connect to usbmuxd ({e}). Connect the device over USB and tap 'Trust'.")
    })?;
    let devs = mux.get_devices().await.map_err(err)?;
    let mut out = Vec::new();
    for d in devs {
        let conn = match d.connection_type {
            Connection::Usb => "USB",
            Connection::Network(_) => "Wi-Fi",
            _ => "?",
        }
        .to_string();
        let dev = Device::new(d).await;
        out.push(DeviceInfo {
            id: dev.device_id.to_string(),
            udid: dev.udid.clone(),
            name: dev.name.clone(),
            connection: conn,
        });
    }
    // usbmuxd reports one entry per transport; keep USB first, dedupe by UDID.
    out.sort_by_key(|d| d.connection != "USB");
    let mut seen = std::collections::HashSet::new();
    out.retain(|d| seen.insert(d.udid.clone()));
    Ok(out)
}

/// UDIDs usbmuxd currently sees (cheap; used for hot-plug detection).
pub async fn udids() -> Option<Vec<String>> {
    let mut mux = UsbmuxdConnection::default().await.ok()?;
    let mut v: Vec<String> = mux.get_devices().await.ok()?.into_iter().map(|d| d.udid).collect();
    v.sort();
    v.dedup();
    Some(v)
}

/// Watch usbmuxd and emit `devices-changed` as soon as a device is plugged in or removed.
pub fn watch(app: tauri::AppHandle) {
    use tauri::Emitter;
    tauri::async_runtime::spawn(async move {
        let mut last: Option<Vec<String>> = None;
        loop {
            let now = udids().await.unwrap_or_default();
            if last.as_ref() != Some(&now) {
                if last.is_some() {
                    crate::devlog::log(format!("usbmuxd: devices changed → {}", if now.is_empty() { "none".into() } else { now.join(", ") }));
                    let _ = app.emit("devices-changed", &now);
                }
                last = Some(now);
            }
            tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
        }
    });
}

async fn device(id: &str) -> Result<Device, String> {
    get_device_for_id(id).await.map_err(err)
}

pub async fn apps(id: &str) -> Result<Vec<serde_json::Value>, String> {
    apps_kind(id, "User").await
}

/// Installed apps of one type ("User" or "System").
pub async fn apps_kind(id: &str, kind: &str) -> Result<Vec<serde_json::Value>, String> {
    let dev = device(id).await?;
    let ud = dev.usbmuxd_device.clone().ok_or("Device not connected")?;
    let provider = ud.to_provider(UsbmuxdAddr::from_env_var().unwrap_or_default(), "ipacked");
    let native = async {
        let mut ic = InstallationProxyClient::connect(&provider).await.map_err(err)?;
        ic.get_apps(Some(kind), None).await.map_err(err)
    }
    .await;
    let apps: Vec<(String, Value)> = match native {
        Ok(a) => a.into_iter().collect(),
        Err(e) if tools::is_legacy_tls(&e) => {
            // Legacy iOS: ask libimobiledevice instead.
            let xml = tools::run("ideviceinstaller", &["-u", &dev.udid, "list", if kind == "System" { "--system" } else { "--user" }, "--xml"]).await?;
            let arr: Vec<Value> = plist::from_bytes(&xml).map_err(err)?;
            arr.into_iter()
                .filter_map(|v| {
                    let bid = v.as_dictionary()?.get("CFBundleIdentifier")?.as_string()?.to_string();
                    Some((bid, v))
                })
                .collect()
        }
        Err(e) => return Err(e),
    };
    let mut out: Vec<serde_json::Value> = apps
        .into_iter()
        .map(|(bid, info)| {
            let d = info.as_dictionary().cloned().unwrap_or_default();
            let g = |k: &str| d.get(k).and_then(Value::as_string).unwrap_or("").to_string();
            let name = [g("CFBundleDisplayName"), g("CFBundleName"), bid.clone()]
                .into_iter()
                .find(|s| !s.is_empty())
                .unwrap_or_default();
            // Purchaser: AppleId attribute, else the embedded iTunesMetadata.
            let apple_id = Some(g("AppleId")).filter(|x| !x.is_empty()).or_else(|| {
                d.get("iTunesMetadata")
                    .and_then(Value::as_data)
                    .and_then(|b| plist::from_bytes::<plist::Dictionary>(b).ok())
                    .and_then(|m| crate::archive::apple_id_of(&m))
            });
            json!({
                "bundleId": bid,
                "name": name,
                "version": g("CFBundleShortVersionString"),
                "build": g("CFBundleVersion"),
                "minIos": g("MinimumOSVersion"),
                "appleId": apple_id.unwrap_or_default(),
                "container": g("Container"),
                "path": g("Path"),
                "signer": g("SignerIdentity"),
                "appStore": d.contains_key("iTunesMetadata") || g("SignerIdentity").contains("Apple iPhone OS Application Signing"),
            })
        })
        .collect();
    out.sort_by(|a, b| {
        a["name"]
            .as_str()
            .unwrap_or("")
            .to_lowercase()
            .cmp(&b["name"].as_str().unwrap_or("").to_lowercase())
    });
    Ok(out)
}

fn progress(app: &AppHandle, stage: &str, pct: i32) {
    crate::devlog::log(format!("install: {stage} ({pct}%)"));
    let _ = app.emit("install-progress", json!({ "stage": stage, "percent": pct }));
}

/// Install an IPA. `resign == false` sends the IPA untouched (App Store IPAs from
/// ipatool keep their FairPlay signature and install for the account that owns them).
/// `resign == true` follows Impactor: sign with the saved Apple ID and install.
/// Legacy iOS doesn't hand out iTunesMetadata through installation_proxy:
/// read `<Container>/iTunesMetadata.plist` over AFC2 (jailbroken devices).
/// Returns bundle id → purchaser Apple ID ("" = App Store app, ID unknown).
pub async fn apple_ids(id: &str, apps: Vec<(String, String)>) -> serde_json::Map<String, serde_json::Value> {
    let mut out = serde_json::Map::new();
    let Ok(udid) = udid_of(id).await else { return out };
    let tmp = std::env::temp_dir().join(format!("ipacked-meta-{}", std::process::id()));
    let mut afc2_checked = false;
    for (bid, container) in apps {
        if container.is_empty() {
            continue;
        }
        let remote = format!("{container}/iTunesMetadata.plist");
        let local = std::path::PathBuf::from(format!("{}-{}.plist", tmp.display(), out.len()));
        if legacy(&["afc-get", &udid, "afc2", &remote, &local.to_string_lossy()]).await.is_ok() {
            let aid = plist::from_file::<_, plist::Dictionary>(&local).ok().and_then(|d| crate::archive::apple_id_of(&d));
            out.insert(bid, json!(aid.unwrap_or_default()));
        } else if !afc2_checked {
            // First failure: no AFC2 at all → stop instead of trying every app.
            afc2_checked = true;
            if legacy(&["afc-ls", &udid, "afc2", "/"]).await.is_err() {
                break;
            }
        }
        let _ = std::fs::remove_file(&local);
    }
    out
}

/// Remove an app from the device (installation_proxy; ideviceinstaller on legacy iOS).
pub async fn uninstall(id: &str, bundle_id: &str) -> Result<(), String> {
    let dev = device(id).await?;
    let ud = dev.usbmuxd_device.clone().ok_or("Device not connected")?;
    let provider = ud.to_provider(UsbmuxdAddr::from_env_var().unwrap_or_default(), "ipacked_uninstall");
    let native = async {
        let mut ic = InstallationProxyClient::connect(&provider).await.map_err(err)?;
        ic.uninstall(bundle_id, None).await.map_err(err)
    }
    .await;
    match native {
        Err(e) if tools::is_legacy_tls(&e) => {
            tools::run("ideviceinstaller", &["-u", &dev.udid, "uninstall", bundle_id]).await.map(|_| ())
        }
        r => r,
    }
}

pub async fn install(app: AppHandle, id: String, ipa: String, resign: bool) -> Result<(), String> {
    let dev = device(&id).await?;
    let ud = dev.usbmuxd_device.clone().ok_or("Device not connected")?;

    if !resign {
        // iTunes-style: hand installd the IPA's own iTunesMetadata.plist and
        // FairPlay SINF as client options, like Windows iTunes / ideviceinstaller
        // do, so an App Store (FairPlay) IPA installs as a purchased app tied to
        // its Apple ID (shows in Purchased, can update).
        let opts = itunes_options(Path::new(&ipa));
        progress(&app, if opts.is_some() { "Installing with SINF + iTunesMetadata" } else { "Uploading to device" }, 0);
        let provider = ud.to_provider(UsbmuxdAddr::from_env_var().unwrap_or_default(), "ipacked_install");
        let a = app.clone();
        let native = installation::install_package_with_callback(
            &provider,
            &ipa,
            opts,
            move |(p, _): (u64, ())| {
                let a = a.clone();
                async move { progress(&a, "Installing", p as i32) }
            },
            (),
        )
        .await
        .map_err(err);
        if let Err(e) = native {
            if !tools::is_legacy_tls(&e) {
                return Err(e);
            }
            // Legacy iOS → ideviceinstaller, which prints "... (NN%)" lines.
            let a = app.clone();
            tools::run_lines(
                "ideviceinstaller",
                &["-u".into(), dev.udid.clone(), "install".into(), ipa.clone()],
                move |l| {
                    if let Some(p) = l.rsplit('(').next().and_then(|t| t.split('%').next()).and_then(|n| n.trim().parse().ok()) {
                        progress(&a, "Installing (libimobiledevice)", p);
                    }
                },
            )
            .await?;
        }
        progress(&app, "Done", 100);
        return Ok(());
    }

    progress(&app, "Signing in to Apple ID", 0);
    let session = dev_session().await?;
    let teams = session.qh_list_teams().await.map_err(err)?.teams;
    let team_id = teams.first().ok_or("No team found on the account")?.team_id.clone();

    progress(&app, "Preparing certificate", 5);
    let cert = CertificateIdentity::new_with_session(&session, data_path(), None, &team_id, false, None)
        .await
        .map_err(err)?;
    let options = SignerOptions { mode: SignerMode::Pem, ..Default::default() };
    let mut signer = Signer::new(Some(cert), options);

    progress(&app, "Unpacking IPA", 10);
    let pkg = Package::new(PathBuf::from(&ipa)).map_err(err)?;
    let bundle = pkg.get_package_bundle().map_err(err)?;

    progress(&app, "Signing", 20);
    signer.modify_bundle(&bundle, &Some(team_id.clone())).await.map_err(err)?;
    session.qh_ensure_device(&team_id, &dev.name, &dev.udid).await.map_err(err)?;
    signer.register_bundle(&bundle, &session, &team_id, false).await.map_err(err)?;
    signer.sign_bundle(&bundle).await.map_err(err)?;

    progress(&app, "Installing", 40);
    let a = app.clone();
    dev.install_app(bundle.bundle_dir(), move |p| {
        let a = a.clone();
        async move { progress(&a, "Installing", 40 + p * 60 / 100) }
    })
    .await
    .map_err(err)?;
    pkg.remove_package_stage();
    progress(&app, "Done", 100);
    Ok(())
}

/// Run the bundled legacy helper (OpenSSL-based idevice, speaks SSLv3/TLS1.0).
pub async fn legacy(args: &[&str]) -> Result<String, String> {
    crate::devlog::log(format!("$ ipacked-legacy {}", args.join(" ")));
    let out = tokio::process::Command::new(crate::ipatool::sidecar("ipacked-legacy"))
        .args(args)
        .output()
        .await
        .map_err(err)?;
    if !out.status.success() {
        let e = String::from_utf8_lossy(&out.stderr).trim().to_string();
        crate::devlog::log(format!("  ✗ {e}"));
        return Err(e);
    }
    let o = String::from_utf8_lossy(&out.stdout).trim().to_string();
    crate::devlog::log(format!("  ok ({} lines)", o.lines().count()));
    Ok(o)
}

/// App icon PNG from SpringBoard, cached under the archive's icons/device folder.
pub async fn icon(id: &str, bundle_id: &str) -> Result<String, String> {
    let dir = crate::archive::root().join("icons").join("device");
    std::fs::create_dir_all(&dir).ok();
    let out = dir.join(format!("{bundle_id}.png"));
    if out.exists() {
        return Ok(out.to_string_lossy().to_string());
    }
    let dev = device(id).await?;
    let ud = dev.usbmuxd_device.clone().ok_or("Device not connected")?;
    let provider = ud.to_provider(UsbmuxdAddr::from_env_var().unwrap_or_default(), "ipacked_icon");
    let native = async {
        let mut sb = idevice::springboardservices::SpringBoardServicesClient::connect(&provider)
            .await
            .map_err(err)?;
        sb.get_icon_pngdata(bundle_id.to_string()).await.map_err(err)
    }
    .await;
    match native {
        Ok(png) => std::fs::write(&out, png).map_err(err)?,
        Err(e) if tools::is_legacy_tls(&e) => {
            legacy(&["icon", &dev.udid, bundle_id, &out.to_string_lossy()]).await?;
        }
        Err(e) => return Err(e),
    }
    Ok(out.to_string_lossy().to_string())
}

// ---------- Apple ID (developer) account, Impactor style ----------

fn store_path() -> Option<PathBuf> {
    Some(data_path().join("accounts.json"))
}

pub async fn dev_account() -> Option<String> {
    let s = AccountStore::load(&store_path()).await.ok()?;
    s.selected_account().map(|a| a.email().clone())
}

pub async fn dev_logout() -> Result<(), String> {
    let mut s = AccountStore::load(&store_path()).await.map_err(err)?;
    if let Some(email) = s.selected_account().map(|a| a.email().clone()) {
        s.accounts_remove(&email).await.map_err(err)?;
    }
    Ok(())
}

async fn dev_session() -> Result<DeveloperSession, String> {
    let s = AccountStore::load(&store_path()).await.map_err(err)?;
    let acc = s
        .selected_account()
        .ok_or("Sign in with an Apple ID for signing in Settings first")?
        .clone();
    let cfg = AnisetteConfiguration::default().set_configuration_path(data_path());
    DeveloperSession::new(acc.adsid().clone(), acc.xcode_gs_token().clone(), cfg)
        .await
        .map_err(err)
}

/// Pending 2FA answer channel, filled by `dev_submit_2fa`.
pub type TfaSlot = Arc<Mutex<Option<mpsc::Sender<String>>>>;

pub async fn dev_login(app: AppHandle, slot: TfaSlot, email: String, password: String) -> Result<String, String> {
    let (u, p) = (email.clone(), password);
    let login = move || Ok::<_, String>((u.clone(), p.clone()));
    let app2 = app.clone();
    let tfa = move |req: plume_core::auth::TwoFactorRequest| -> Result<TwoFactorAction, String> {
        let (tx, rx) = mpsc::channel();
        *slot.lock().unwrap() = Some(tx);
        let method = match req.method {
            TwoFactorMethod::Sms => "sms",
            TwoFactorMethod::Device => "device",
        };
        let _ = app2.emit(
            "dev-2fa",
            json!({ "method": method, "canSms": !req.trusted_phone_numbers.is_empty() }),
        );
        let code = rx
            .recv_timeout(Duration::from_secs(300))
            .map_err(|_| "2FA code timed out".to_string())?;
        if code.eq_ignore_ascii_case("sms") {
            if let Some(ph) = req.trusted_phone_numbers.first() {
                return Ok(TwoFactorAction::SendSms(ph.id));
            }
        }
        Ok(TwoFactorAction::SubmitCode(code))
    };
    let cfg = AnisetteConfiguration::default().set_configuration_path(data_path());
    // Account::login blocks inside the 2FA closure; keep it off the async workers.
    let account = tokio::task::spawn_blocking(move || {
        tauri::async_runtime::block_on(Account::login(login, tfa, cfg))
    })
    .await
    .map_err(err)?
    .map_err(err)?;
    let mut s = AccountStore::load(&store_path()).await.map_err(err)?;
    s.accounts_add_from_session(email.clone(), account).await.map_err(err)?;
    Ok(email)
}

// ---------- Export installed apps from the device ----------

/// Pull an installed app off the device as an IPA into `out_dir`.
/// - `archive`: ideviceinstaller archive (iOS ≤ 6, no jailbreak; keeps the
///   original App Store signature + sinf, i.e. still FairPlay encrypted).
/// - `bagbak`: decrypting dump via frida (jailbreak + frida-server).
/// - `clutch`: Clutch over SSH (jailbreak, Clutch + OpenSSH on device, key auth).
pub async fn export(
    app: AppHandle,
    id: &str,
    bundle_id: &str,
    method: &str,
    out_dir: PathBuf,
) -> Result<(PathBuf, String), String> {
    let dev = device(id).await?;
    let udid = dev.udid.clone();
    std::fs::create_dir_all(&out_dir).map_err(err)?;
    let a = app.clone();
    let log = move |l: &str| {
        let _ = a.emit("export-log", json!({ "line": l }));
    };

    match method {
        "archive" => {
            let before = tools::newest_ipa(&out_dir);
            tools::run_lines(
                "ideviceinstaller",
                &[
                    "-u".into(), udid.clone(), "archive".into(), bundle_id.into(),
                    "--app-only".into(), format!("--copy={}", out_dir.display()), "--remove".into(),
                ],
                log,
            )
            .await?;
            let ipa = tools::newest_ipa(&out_dir)
                .filter(|p| Some(p) != before.as_ref() || p.exists())
                .ok_or("Archive file not found (this method only works on iOS 6 and older)")?;
            Ok((ipa, format!("Device archive (ideviceinstaller) · {}", dev.name)))
        }
        "bagbak" => {
            let out = out_dir.join(format!("{bundle_id}.decrypted.ipa"));
            let _ = std::fs::remove_file(&out);
            tools::run_lines(
                "bagbak",
                &["-f".into(), "-o".into(), out.to_string_lossy().to_string(), bundle_id.into()],
                log,
            )
            .await?;
            let ipa = if out.exists() { out } else { tools::newest_ipa(&out_dir).ok_or("bagbak produced no output")? };
            Ok((ipa, format!("Device dump (bagbak/frida, decrypted) · {}", dev.name)))
        }
        "clutch" => {
            // SSH to the device's own IP when set (e.g. 192.168.x.x), else USB via iproxy.
            let conn = crate::ssh::Conn::open(&udid).await?;
            log(&format!("SSH: {}", conn.label()));
            let bin = crate::ssh::clutch_path(&conn)
                .await?
                .ok_or("Clutch is not on the device. Install it as /bin/clutch with \"Install Clutch\" in Devices.")?;
            log(&format!("Clutch: {bin}"));
            let out = crate::ssh::clutch_dump(&conn, &bin, bundle_id, &out_dir, &log).await?;
            Ok((out, format!("Device dump (Clutch, decrypted) · {}", dev.name)))
        }
        _ => Err(format!("Unknown method: {method}")),
    }
}

pub async fn udid_of(id: &str) -> Result<String, String> {
    Ok(device(id).await?.udid)
}

/// `{ iTunesMetadata: <data>, ApplicationSINF: <data> }` from an App Store IPA.
fn itunes_options(ipa: &Path) -> Option<plist::Value> {
    use std::io::Read;
    let mut z = zip::ZipArchive::new(std::fs::File::open(ipa).ok()?).ok()?;
    let mut read = |name: &str| -> Option<Vec<u8>> {
        let mut f = z.by_name(name).ok()?;
        let mut v = Vec::new();
        f.read_to_end(&mut v).ok()?;
        Some(v)
    };
    let names: Vec<String> = {
        let z2 = zip::ZipArchive::new(std::fs::File::open(ipa).ok()?).ok()?;
        z2.file_names().map(str::to_string).collect()
    };
    let mut d = plist::Dictionary::new();
    if let Some(m) = read("iTunesMetadata.plist") {
        d.insert("iTunesMetadata".into(), plist::Value::Data(m));
    }
    // Payload/X.app/SC_Info/<exe>.sinf (top-level app only, not extensions).
    if let Some(n) = names.iter().find(|n| {
        let p: Vec<&str> = n.split('/').collect();
        p.len() == 4 && p[0] == "Payload" && p[1].ends_with(".app") && p[2] == "SC_Info" && p[3].ends_with(".sinf")
    }) {
        if let Some(s) = read(n) {
            d.insert("ApplicationSINF".into(), plist::Value::Data(s));
        }
    }
    (!d.is_empty()).then_some(plist::Value::Dictionary(d))
}

/// iOS version, model and CPU of a device (libimobiledevice works on every iOS).
pub async fn details(id: &str) -> Result<serde_json::Value, String> {
    let dev = device(id).await?;
    let udid = dev.udid.clone();
    let get = |k: &'static str| {
        let udid = udid.clone();
        async move {
            crate::tools::run("ideviceinfo", &["-u", &udid, "-k", k])
                .await
                .map(|b| String::from_utf8_lossy(&b).trim().to_string())
                .unwrap_or_default()
        }
    };
    let (ver, pt, cpu, name) = tokio::join!(get("ProductVersion"), get("ProductType"), get("CPUArchitecture"), get("DeviceName"));
    Ok(json!({ "udid": udid, "name": if name.is_empty() { dev.name } else { name }, "iosVersion": ver, "productType": pt, "cpu": cpu }))
}
