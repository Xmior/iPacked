//! ipacked-legacy <cmd> <udid> ...
//!   afc2-put <udid> <local> <remote>   upload via AFC2 (jailbroken, root fs)
//!   icon <udid> <bundle-id> <out.png>  SpringBoard icon PNG
use idevice::{IdeviceService, afc::AfcClient, afc::opcode::AfcFopenMode, springboardservices::SpringBoardServicesClient, usbmuxd::{UsbmuxdAddr, UsbmuxdConnection}};

#[tokio::main]
async fn main() {
    let a: Vec<String> = std::env::args().collect();
    if let Err(e) = run(&a).await {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

async fn run(a: &[String]) -> Result<(), String> {
    let cmd = a.get(1).ok_or("usage: ipacked-legacy <afc2-put|icon> <udid> ...")?;
    let udid = a.get(2).ok_or("udid missing")?;
    let mut mux = UsbmuxdConnection::default().await.map_err(|e| e.to_string())?;
    let dev = mux.get_device(udid).await.map_err(|e| e.to_string())?;
    let p = dev.to_provider(UsbmuxdAddr::default(), "ipacked-legacy");
    match cmd.as_str() {
        "afc2-put" => {
            let (local, remote) = (a.get(3).ok_or("local missing")?, a.get(4).ok_or("remote missing")?);
            let bytes = tokio::fs::read(local).await.map_err(|e| e.to_string())?;
            let mut afc = AfcClient::new_afc2(&p).await.map_err(|e| format!("Could not open AFC2 (needs jailbreak + afc2add): {e}"))?;
            if let Some(dir) = std::path::Path::new(remote).parent() {
                let mut cur = String::new();
                for part in dir.to_string_lossy().split('/').filter(|s| !s.is_empty()) {
                    cur += "/";
                    cur += part;
                    let _ = afc.mk_dir(cur.clone()).await;
                }
            }
            let mut f = afc.open(remote.clone(), AfcFopenMode::WrOnly).await.map_err(|e| e.to_string())?;
            f.write_entire(&bytes).await.map_err(|e| e.to_string())?;
            f.close().await.map_err(|e| e.to_string())?;
            println!("ok {remote}");
        }
        "icon" => {
            let (bid, out) = (a.get(3).ok_or("bundle missing")?, a.get(4).ok_or("out missing")?);
            let mut sb = SpringBoardServicesClient::connect(&p).await.map_err(|e| e.to_string())?;
            let png = sb.get_icon_pngdata(bid.clone()).await.map_err(|e| e.to_string())?;
            tokio::fs::write(out, png).await.map_err(|e| e.to_string())?;
            println!("ok {out}");
        }
        // afc-<op> <udid> <afc|afc2> ...  — file browser
        c if c.starts_with("afc-") => {
            let mode = a.get(3).ok_or("mode missing")?;
            let mut afc = if mode == "afc2" {
                AfcClient::new_afc2(&p).await.map_err(|e| format!("Could not open AFC2 (needs jailbreak + Apple File Conduit \"2\"): {e}"))?
            } else if let Some(bid) = mode.strip_prefix("ha:") {
                // App sandbox via house_arrest (iTunes File Sharing): whole
                // container first, Documents only if the OS refuses that.
                use idevice::house_arrest::HouseArrestClient;
                let ha = HouseArrestClient::connect(&p).await.map_err(|e| e.to_string())?;
                match ha.vend_container(bid.to_string()).await {
                    Ok(c) => c,
                    Err(_) => HouseArrestClient::connect(&p).await.map_err(|e| e.to_string())?
                        .vend_documents(bid.to_string()).await
                        .map_err(|e| format!("Could not access app data ({bid}): {e}"))?,
                }
            } else {
                AfcClient::connect(&p).await.map_err(|e| e.to_string())?
            };
            let e = |e: idevice::IdeviceError| e.to_string();
            match c {
                "afc-ls" => {
                    let dir = a.get(4).ok_or("path missing")?.trim_end_matches('/').to_string();
                    let mut names = afc.list_dir(if dir.is_empty() { "/".to_string() } else { dir.clone() }).await.map_err(e)?;
                    names.retain(|n| n != "." && n != "..");
                    names.sort_by_key(|n| n.to_lowercase());
                    for n in names {
                        let full = format!("{dir}/{n}");
                        let (kind, size, mtime, target) = match afc.get_file_info(full.clone()).await {
                            Ok(i) => (i.st_ifmt.clone(), i.size, i.modified.and_utc().timestamp(), i.st_link_target.clone().unwrap_or_default()),
                            Err(_) => ("?".into(), 0, 0, String::new()),
                        };
                        println!("{}\t{}\t{}\t{}\t{}", n.replace(['\t', '\n'], " "), kind, size, mtime, target);
                    }
                }
                "afc-get" => {
                    let (remote, local) = (a.get(4).ok_or("remote missing")?, a.get(5).ok_or("local missing")?);
                    let mut f = afc.open(remote.clone(), AfcFopenMode::RdOnly).await.map_err(e)?;
                    let b = f.read_entire().await.map_err(e)?;
                    f.close().await.map_err(e)?;
                    tokio::fs::write(local, b).await.map_err(|e| e.to_string())?;
                    println!("ok");
                }
                "afc-put" => {
                    let (local, remote) = (a.get(4).ok_or("local missing")?, a.get(5).ok_or("remote missing")?);
                    let bytes = tokio::fs::read(local).await.map_err(|e| e.to_string())?;
                    let mut f = afc.open(remote.clone(), AfcFopenMode::WrOnly).await.map_err(e)?;
                    f.write_entire(&bytes).await.map_err(e)?;
                    f.close().await.map_err(e)?;
                    println!("ok");
                }
                "afc-mkdir" => {
                    afc.mk_dir(a.get(4).ok_or("path missing")?.clone()).await.map_err(e)?;
                    println!("ok");
                }
                "afc-rm" => {
                    afc.remove_all(a.get(4).ok_or("path missing")?.clone()).await.map_err(e)?;
                    println!("ok");
                }
                "afc-mv" => {
                    afc.rename(a.get(4).ok_or("from missing")?.clone(), a.get(5).ok_or("to missing")?.clone()).await.map_err(e)?;
                    println!("ok");
                }
                _ => return Err(format!("unknown command {c}")),
            }
        }
        _ => return Err(format!("unknown command {cmd}")),
    }
    Ok(())
}
