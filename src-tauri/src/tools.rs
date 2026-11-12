//! External CLI helpers: libimobiledevice (fallback for legacy iOS that needs
//! SSLv3/TLS1.0, which rustls can't speak), bagbak/frida and Clutch dumpers.

use std::path::{Path, PathBuf};
use std::process::Stdio;

use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

/// GUI apps on macOS don't inherit the shell PATH, so search the usual places.
pub fn which(name: &str) -> Option<PathBuf> {
    let home = std::env::var("HOME").unwrap_or_default();
    let mut dirs: Vec<PathBuf> = std::env::var("PATH")
        .unwrap_or_default()
        .split(if cfg!(windows) { ';' } else { ':' })
        .map(PathBuf::from)
        .collect();
    for d in ["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin"] {
        dirs.push(d.into());
    }
    dirs.push(PathBuf::from(&home).join(".npm-global/bin"));
    dirs.push(PathBuf::from(&home).join(".local/bin"));
    let exe = if cfg!(windows) { format!("{name}.exe") } else { name.to_string() };
    dirs.into_iter().map(|d| d.join(&exe)).find(|p| p.is_file())
}

fn env_path() -> String {
    format!(
        "/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:{}",
        std::env::var("PATH").unwrap_or_default()
    )
}

/// Run a tool to completion; Err carries stderr/stdout.
pub async fn run(tool: &str, args: &[&str]) -> Result<Vec<u8>, String> {
    let bin = which(tool).ok_or_else(|| format!("'{tool}' not found. {}", install_hint(tool)))?;
    crate::devlog::log(format!("$ {tool} {}", args.join(" ")));
    let out = Command::new(bin)
        .args(args)
        .env("PATH", env_path())
        .output()
        .await
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        let msg = String::from_utf8_lossy(&out.stderr).to_string() + &String::from_utf8_lossy(&out.stdout);
        crate::devlog::log(format!("  ✗ {tool} failed: {}", msg.trim()));
        return Err(format!("{tool}: {}", msg.trim()));
    }
    let o = String::from_utf8_lossy(&out.stdout);
    if !o.trim_start().starts_with("<?xml") {
        crate::devlog::log(format!("  {}", o.trim().chars().take(2000).collect::<String>()));
    } else {
        crate::devlog::log(format!("  ok ({} bytes plist)", out.stdout.len()));
    }
    Ok(out.stdout)
}

/// Run a tool and feed every output line (stdout+stderr) to `on_line`.
pub async fn run_lines(
    tool: &str,
    args: &[String],
    mut on_line: impl FnMut(&str),
) -> Result<String, String> {
    let bin = which(tool).ok_or_else(|| format!("'{tool}' not found. {}", install_hint(tool)))?;
    crate::devlog::log(format!("$ {tool} {}", args.join(" ")));
    let mut child = Command::new(bin)
        .args(args)
        .env("PATH", env_path())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;
    let mut out = BufReader::new(child.stdout.take().unwrap()).lines();
    let mut err = BufReader::new(child.stderr.take().unwrap()).lines();
    let mut all = String::new();
    loop {
        tokio::select! {
            l = out.next_line() => match l { Ok(Some(l)) => { crate::devlog::log(format!("  {l}")); on_line(&l); all += &l; all.push('\n'); } _ => break },
            l = err.next_line() => if let Ok(Some(l)) = l { crate::devlog::log(format!("  {l}")); on_line(&l); all += &l; all.push('\n'); },
        }
    }
    while let Ok(Some(l)) = err.next_line().await {
        on_line(&l);
        all += &l;
        all.push('\n');
    }
    let st = child.wait().await.map_err(|e| e.to_string())?;
    if !st.success() {
        crate::devlog::log(format!("  ✗ {tool} exited with {st}"));
        return Err(format!("{tool} failed:\n{}", tail(&all, 8)));
    }
    Ok(all)
}

fn tail(s: &str, n: usize) -> String {
    let v: Vec<&str> = s.lines().collect();
    v[v.len().saturating_sub(n)..].join("\n")
}

pub fn install_hint(tool: &str) -> &'static str {
    match tool {
        "ideviceinstaller" | "afcclient" | "iproxy" => "Install: brew install ideviceinstaller libimobiledevice",
        "bagbak" => "Install: npm install -g bagbak (the device needs a jailbreak + frida-server)",
        "ssh" | "scp" => "OpenSSH is required",
        "irecovery" => "Install: brew install libirecovery",
        "ideviceenterrecovery" => "Install: brew install libimobiledevice",
        _ => "",
    }
}

/// True for the rustls handshake failures legacy devices (iOS ≤ ~9) produce.
pub fn is_legacy_tls(err: &str) -> bool {
    let e = err.to_lowercase();
    e.contains("handshake") || e.contains("alertreceived") || e.contains("socket io") || e.contains("tls")
}

pub fn which_status() -> serde_json::Value {
    let f = |t: &str| which(t).map(|p| p.to_string_lossy().to_string());
    serde_json::json!({
        "ideviceinstaller": f("ideviceinstaller"),
        "afcclient": f("afcclient"),
        "iproxy": f("iproxy"),
        "bagbak": f("bagbak"),
        "frida": f("frida"),
        "ssh": f("ssh"),
    })
}

pub fn newest_ipa(dir: &Path) -> Option<PathBuf> {
    std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().map(|e| e.eq_ignore_ascii_case("ipa")).unwrap_or(false))
        .max_by_key(|p| p.metadata().and_then(|m| m.modified()).ok())
}
