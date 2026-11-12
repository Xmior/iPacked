//! Thin bridge over the bundled `ipatool` binary (JSON output mode).

use std::path::PathBuf;
use std::sync::Mutex;

use serde_json::Value;
use tokio::process::Command;

static PLATFORM: Mutex<String> = Mutex::new(String::new());

/// "" (iPhone/iPad), "macos" or "appletv" — applied to every store command.
pub fn set_platform(p: &str) {
    *PLATFORM.lock().unwrap() = p.to_string();
}

pub fn platform() -> String {
    PLATFORM.lock().unwrap().clone()
}

/// Commands that accept `--platform`.
fn takes_platform(cmd: &str) -> bool {
    matches!(
        cmd,
        "list-purchases" | "search" | "list-versions" | "get-version-metadata" | "get-versions-metadata" | "download" | "purchase"
    )
}

/// Random passphrase for ipatool's encrypted file keyring (instead of the macOS
/// Keychain, which re-prompts for every unsigned/rebuilt ipatool binary).
fn keyring_passphrase() -> String {
    let home = std::env::var("HOME").unwrap_or_default();
    let dir = PathBuf::from(home).join("Library/Application Support/iPacked");
    let _ = std::fs::create_dir_all(&dir);
    let f = dir.join("ipatool-keyring.key");
    if let Ok(s) = std::fs::read_to_string(&f) {
        if !s.trim().is_empty() {
            return s.trim().to_string();
        }
    }
    let mut b = [0u8; 32];
    let _ = std::fs::File::open("/dev/urandom").and_then(|mut r| std::io::Read::read_exact(&mut r, &mut b));
    let key = hex::encode(b);
    let _ = std::fs::write(&f, &key);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o600));
    }
    key
}

/// iPacked's own ipatool session folder. ~/.ipatool is shared with any other
/// ipatool user (and older iPast builds), which re-encrypted the session with
/// a different key → "aes.KeyUnwrap(): integrity check failed" and sign-outs.
fn state_dir() -> PathBuf {
    let home = PathBuf::from(std::env::var("HOME").unwrap_or_default());
    let dir = home.join("Library/Application Support/iPacked/ipatool");
    if !dir.exists() {
        // First run with a private folder: start from the shared session.
        let _ = std::fs::create_dir_all(&dir);
        if let Ok(rd) = std::fs::read_dir(home.join(".ipatool")) {
            for e in rd.flatten() {
                if e.path().is_file() {
                    let _ = std::fs::copy(e.path(), dir.join(e.file_name()));
                }
            }
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700));
        }
    }
    dir
}

/// Arguments with iPacked's standard flags (+ platform) appended.
fn full_args(args: &[String]) -> Vec<String> {
    let mut v: Vec<String> = args.to_vec();
    v.extend(["--format", "json", "--non-interactive", "--keychain-passphrase"].map(String::from));
    v.push(keyring_passphrase());
    let p = platform();
    if !p.is_empty() && !args.iter().any(|a| a == "--platform") && args.first().map(|a| takes_platform(a)).unwrap_or(false) {
        v.extend(["--platform".to_string(), p]);
    }
    v
}

/// One-shot ipatool process.
fn command(args: &[String]) -> Command {
    let mut c = Command::new(bin_path());
    c.args(full_args(args)).env("IPATOOL_KEYRING_BACKEND", "file").env("IPATOOL_STATE_DIR", state_dir()).kill_on_drop(true);
    c
}

/// Long-lived `ipatool serve` process: the App Store signing emulator boots
/// once and stays warm (~2 s per request instead of ~15–20 s and one ~1 GB
/// process per call). It exits by itself after 90 idle seconds. While it is busy
/// (e.g. a long download) other calls fall back to a one-shot process.
struct Server {
    child: tokio::process::Child,
    stdin: tokio::process::ChildStdin,
    lines: tokio::io::Lines<tokio::io::BufReader<tokio::process::ChildStdout>>,
}

fn server_slot() -> &'static tokio::sync::Mutex<Option<Server>> {
    static S: std::sync::OnceLock<tokio::sync::Mutex<Option<Server>>> = std::sync::OnceLock::new();
    S.get_or_init(|| tokio::sync::Mutex::new(None))
}

fn spawn_server() -> Result<Server, String> {
    use tokio::io::AsyncBufReadExt;
    let mut child = Command::new(bin_path())
        .arg("serve")
        .env("IPATOOL_KEYRING_BACKEND", "file")
        .env("IPATOOL_STATE_DIR", state_dir())
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| format!("could not run ipatool: {e}"))?;
    let stdin = child.stdin.take().ok_or("stdin")?;
    let lines = tokio::io::BufReader::new(child.stdout.take().ok_or("stdout")?).lines();
    Ok(Server { child, stdin, lines })
}

/// Run through the warm server. `None` = server unavailable/busy/died → caller
/// should use a one-shot process.
async fn exec_server(args: &[String], on_line: &mut (dyn FnMut(Value) + Send)) -> Option<()> {
    use tokio::io::AsyncWriteExt;
    // Busy (a long call such as a purchases refresh or a version-name batch is
    // running) → don't queue behind it; the caller runs a one-shot process.
    let Ok(mut guard) = server_slot().try_lock() else { return None };
    if let Some(s) = guard.as_mut() {
        if s.child.try_wait().ok().flatten().is_some() {
            *guard = None;
        }
    }
    if guard.is_none() {
        *guard = spawn_server().ok();
    }
    let s = guard.as_mut()?;
    let req = serde_json::to_string(&full_args(args)).ok()? + "\n";
    if s.stdin.write_all(req.as_bytes()).await.is_err() || s.stdin.flush().await.is_err() {
        *guard = None;
        return None;
    }
    loop {
        match s.lines.next_line().await {
            Ok(Some(l)) => {
                let Ok(v) = serde_json::from_str::<Value>(l.trim()) else { continue };
                if v.get("ipacked_done").is_some() {
                    return Some(());
                }
                on_line(v);
            }
            _ => {
                // Server died mid-request (crash/idle exit race) → retry one-shot.
                *guard = None;
                return None;
            }
        }
    }
}

/// Locate the ipatool sidecar: next to the app executable when bundled,
/// otherwise the dev copy in `src-tauri/binaries`.
pub fn bin_path() -> PathBuf {
    // The app may run under Rosetta; ipatool's Apple signing emulation is ~2.5x
    // faster natively, so use the bundled arm64 copy when the CPU supports it.
    #[cfg(target_os = "macos")]
    if is_apple_silicon() {
        if let Ok(exe) = std::env::current_exe() {
            if let Some(res) = exe.parent().and_then(|d| d.parent()).map(|c| c.join("Resources/ipatool-arm64")) {
                if res.exists() {
                    return res;
                }
            }
        }
        let dev = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("binaries/ipatool-arm64");
        if dev.exists() {
            return dev;
        }
    }
    sidecar("ipatool")
}

#[cfg(target_os = "macos")]
fn is_apple_silicon() -> bool {
    std::process::Command::new("/usr/sbin/sysctl")
        .args(["-n", "hw.optional.arm64"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim() == "1")
        .unwrap_or(false)
}

/// PID of the running `run_stream` process. Only one may run: a new call (e.g.
/// the user selected another app) kills the previous one, so version-name
/// lookups no longer pile up as a dozen ~400 MB processes.
static STREAM_PID: Mutex<Option<u32>> = Mutex::new(None);

fn kill_pid(pid: u32) {
    let _ = std::process::Command::new("/bin/kill").arg(pid.to_string()).status();
}

/// Kill the running `run_stream` process, if any.
pub fn cancel_stream() {
    if let Some(pid) = STREAM_PID.lock().unwrap().take() {
        kill_pid(pid);
    }
}

/// Runs ipatool and calls `on_json` for every JSON line as it arrives.
pub async fn run_stream(args: &[String], mut on_json: impl FnMut(Value) + Send) -> Result<(), String> {
    use tokio::io::{AsyncBufReadExt, BufReader};
    let mut last_err = None;
    cancel_stream();
    let mut child = command(args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| format!("could not run ipatool: {e}"))?;
    let pid = child.id();
    *STREAM_PID.lock().unwrap() = pid;
    let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
    while let Ok(Some(l)) = lines.next_line().await {
        if let Ok(v) = serde_json::from_str::<Value>(&l) {
            if v.get("level").and_then(Value::as_str) == Some("error") {
                last_err = v.get("error").and_then(Value::as_str).map(str::to_string);
            } else {
                on_json(v);
            }
        }
    }
    let _ = child.wait().await;
    let mut g = STREAM_PID.lock().unwrap();
    if *g == pid {
        *g = None;
    }
    match last_err {
        Some(e) => Err(e),
        None => Ok(()),
    }
}

/// Locate a bundled sidecar binary by base name.
pub fn sidecar(name: &str) -> PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let p = dir.join(if cfg!(windows) { format!("{name}.exe") } else { name.to_string() });
            if p.exists() {
                return p;
            }
        }
    }
    let triple = if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        "aarch64-apple-darwin"
    } else if cfg!(target_os = "macos") {
        "x86_64-apple-darwin"
    } else {
        "x86_64-pc-windows-msvc"
    };
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("binaries")
        .join(format!("{name}-{triple}{}", if cfg!(windows) { ".exe" } else { "" }))
}

/// Runs ipatool with `--format json --non-interactive` and returns every JSON line it printed.
/// A line with `"level":"error"` turns into `Err`.
pub async fn run(args: &[String]) -> Result<Vec<Value>, String> {
    let mut lines = Vec::new();
    // Long batch jobs get their own process so the warm server stays free for
    // quick calls (sign-in, licenses, version lists, downloads' metadata).
    let long = matches!(args.first().map(String::as_str), Some("get-versions-metadata"));
    if !long && exec_server(args, &mut |v| lines.push(v)).await.is_some() {
        return finish(lines, None);
    }
    // At most two extra one-shot processes at a time (~400 MB each).
    static ONESHOT: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(2);
    let _permit = ONESHOT.acquire().await;
    let out = command(args)
        .output()
        .await
        .map_err(|e| format!("could not run ipatool: {e}"))?;
    let text = format!(
        "{}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('{') {
            if let Ok(v) = serde_json::from_str::<Value>(line) {
                lines.push(v);
            }
        }
    }
    let fail = (!out.status.success()).then(|| format!("ipatool exit code {:?}: {}", out.status.code(), text.trim()));
    finish(lines, fail)
}

fn finish(lines: Vec<Value>, fail: Option<String>) -> Result<Vec<Value>, String> {
    if let Some(v) = lines.iter().find(|v| v.get("level").and_then(Value::as_str) == Some("error")) {
        let msg = v
            .get("error")
            .or_else(|| v.get("message"))
            .and_then(Value::as_str)
            .unwrap_or("unknown ipatool error");
        if msg.contains("integrity check failed") || msg.contains("failed to get account") {
            return Err("Not signed in to the App Store (the saved session can't be read). Sign in again in Settings.".into());
        }
        return Err(msg.to_string());
    }
    match fail {
        Some(f) if lines.is_empty() => Err(f),
        _ => Ok(lines),
    }
}

/// Last line that carries `key`.
pub fn find<'a>(lines: &'a [Value], key: &str) -> Option<&'a Value> {
    lines.iter().rev().find(|l| l.get(key).is_some())
}

/// One-shot ipatool child with piped stdout/stderr (for killable downloads).
pub fn spawn(args: &[String], env: &[(&str, &str)]) -> Result<tokio::process::Child, String> {
    let mut c = command(args);
    for (k, v) in env {
        c.env(k, v);
    }
    c.stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("could not run ipatool: {e}"))
}
