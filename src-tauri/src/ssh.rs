//! SSH to a jailbroken device: over Wi-Fi/LAN (its IP, e.g. 192.168.x.x) or
//! over USB through iproxy. Password auth works without sshpass via
//! SSH_ASKPASS; key auth works as usual. Used for Clutch.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::process::Command;

fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase", default)]
pub struct SshCfg {
    /// Empty = USB (iproxy → 127.0.0.1); otherwise the device's IP / hostname.
    pub host: String,
    pub port: u16,
    pub user: String,
    /// Default jailbreak root password is "alpine". Stored 0600 next to the
    /// ipatool keyring key, never sent anywhere but the device.
    pub password: String,
}

impl Default for SshCfg {
    fn default() -> Self {
        Self { host: String::new(), port: 22, user: "root".into(), password: "alpine".into() }
    }
}

fn support_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_default();
    let d = PathBuf::from(home).join("Library/Application Support/iPacked");
    let _ = std::fs::create_dir_all(&d);
    d
}

fn cfg_file() -> PathBuf {
    support_dir().join("ssh.json")
}

fn all() -> HashMap<String, SshCfg> {
    std::fs::read(cfg_file()).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

pub fn get(udid: &str) -> SshCfg {
    all().remove(udid).unwrap_or_default()
}

pub fn set(udid: &str, cfg: SshCfg) -> Result<(), String> {
    let mut m = all();
    m.insert(udid.to_string(), cfg);
    let f = cfg_file();
    std::fs::write(&f, serde_json::to_vec_pretty(&m).map_err(err)?).map_err(err)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

fn askpass() -> PathBuf {
    let p = support_dir().join("ssh-askpass.sh");
    if !p.exists() {
        let _ = std::fs::write(&p, "#!/bin/sh\nprintf '%s\\n' \"$IPACKED_SSH_PW\"\n");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o700));
        }
    }
    p
}

/// An open route to the device (keeps the iproxy forward alive while in scope).
pub struct Conn {
    host: String,
    port: u16,
    user: String,
    password: String,
    _fwd: Option<tokio::process::Child>,
}

impl Conn {
    pub async fn open(udid: &str) -> Result<Conn, String> {
        let c = get(udid);
        if !c.host.trim().is_empty() {
            return Ok(Conn { host: c.host.trim().into(), port: c.port, user: c.user, password: c.password, _fwd: None });
        }
        let iproxy = crate::tools::which("iproxy").ok_or_else(|| crate::tools::install_hint("iproxy").to_string())?;
        let local = std::net::TcpListener::bind("127.0.0.1:0").map_err(err)?.local_addr().map_err(err)?.port();
        let fwd = Command::new(iproxy)
            .args(["-u", udid, &format!("{local}:{}", c.port)])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .map_err(err)?;
        tokio::time::sleep(Duration::from_millis(700)).await;
        Ok(Conn { host: "127.0.0.1".into(), port: local, user: c.user, password: c.password, _fwd: Some(fwd) })
    }

    pub fn label(&self) -> String {
        if self._fwd.is_some() { "USB (iproxy)".into() } else { format!("{}:{}", self.host, self.port) }
    }

    fn opts() -> Vec<String> {
        [
            "-o", "StrictHostKeyChecking=no", "-o", "UserKnownHostsFile=/dev/null", "-o", "LogLevel=ERROR",
            "-o", "ConnectTimeout=8", "-o", "NumberOfPasswordPrompts=1",
            // Old iOS OpenSSH/dropbear builds only speak these.
            "-o", "KexAlgorithms=+diffie-hellman-group1-sha1,diffie-hellman-group14-sha1",
            "-o", "HostKeyAlgorithms=+ssh-rsa", "-o", "PubkeyAcceptedAlgorithms=+ssh-rsa",
            "-o", "Ciphers=+aes128-cbc,3des-cbc",
        ]
        .map(String::from)
        .to_vec()
    }

    fn cmd(&self, prog: &str) -> Command {
        let mut c = Command::new(prog);
        c.env("SSH_ASKPASS", askpass())
            .env("SSH_ASKPASS_REQUIRE", "force")
            .env("DISPLAY", ":0")
            .env("IPACKED_SSH_PW", &self.password)
            .stdin(std::process::Stdio::null())
            .kill_on_drop(true);
        c
    }

    /// Run a remote shell command; every output line goes to `log`.
    pub async fn exec(&self, remote: &str, mut log: impl FnMut(&str)) -> Result<String, String> {
        use tokio::io::{AsyncBufReadExt, BufReader};
        crate::devlog::log(format!("$ ssh {}@{} {remote}", self.user, self.host));
        let mut c = self.cmd("ssh");
        c.args(Self::opts())
            .args(["-p", &self.port.to_string(), &format!("{}@{}", self.user, self.host), remote])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        let mut child = c.spawn().map_err(|e| format!("could not run ssh: {e}"))?;
        let mut out = BufReader::new(child.stdout.take().unwrap()).lines();
        let mut errl = BufReader::new(child.stderr.take().unwrap()).lines();
        let mut text = String::new();
        let (mut o_done, mut e_done) = (false, false);
        while !(o_done && e_done) {
            tokio::select! {
                l = out.next_line(), if !o_done => match l { Ok(Some(l)) => { crate::devlog::log(format!("  {l}")); log(&l); text += &l; text.push('\n'); } _ => o_done = true },
                l = errl.next_line(), if !e_done => match l { Ok(Some(l)) => { crate::devlog::log(format!("  {l}")); log(&l); text += &l; text.push('\n'); } _ => e_done = true },
            }
        }
        let st = child.wait().await.map_err(err)?;
        crate::devlog::log(format!("  exit {}", st.code().unwrap_or(-1)));
        if st.code() == Some(255) {
            return Err(format!(
                "SSH connection failed ({}). Is OpenSSH installed on the device and are the IP/password right?\n{}",
                self.label(),
                text.trim()
            ));
        }
        Ok(text)
    }

    pub async fn pull(&self, remote: &str, local: &std::path::Path) -> Result<(), String> {
        // Quote for the remote shell: Clutch names contain "(" and ")".
        let r = format!("'{}'", remote.replace('\'', "'\\''"));
        self.scp(&format!("{}@{}:{r}", self.user, self.host), &local.to_string_lossy()).await
    }

    pub async fn push(&self, local: &std::path::Path, remote: &str) -> Result<(), String> {
        self.scp(&local.to_string_lossy(), &format!("{}@{}:{remote}", self.user, self.host)).await
    }

    async fn scp(&self, from: &str, to: &str) -> Result<(), String> {
        // -O: classic scp protocol (old iOS OpenSSH has no sftp-server).
        let out = self
            .cmd("scp")
            .args(["-O", "-T"])
            .args(Self::opts())
            .args(["-P", &self.port.to_string(), from, to])
            .output()
            .await
            .map_err(err)?;
        if out.status.success() {
            Ok(())
        } else {
            Err(format!("scp: {}", String::from_utf8_lossy(&out.stderr).trim()))
        }
    }
}

const CLUTCH_PATHS: &str = "/var/jb/usr/bin/clutch /var/jb/usr/bin/Clutch /var/jb/usr/bin/Clutch2 /bin/clutch /usr/bin/clutch /usr/local/bin/clutch /bin/Clutch /bin/Clutch2 /usr/bin/Clutch /usr/bin/Clutch2 /usr/local/bin/Clutch";

/// Directory Clutch belongs in: /bin, or /var/jb/usr/bin on rootless jailbreaks.
const BIN_DIR: &str = "D=/bin; [ -d /var/jb/usr/bin ] && D=/var/jb/usr/bin";

/// Path of Clutch on the device. Any other spelling (Clutch, Clutch2…) or a
/// copy without the execute bit is normalised to `<bin>/clutch`, mode 755.
pub async fn clutch_path(conn: &Conn) -> Result<Option<String>, String> {
    let out = conn
        .exec(
            &format!(
                "{BIN_DIR}; for p in {CLUTCH_PATHS}; do if [ -f \"$p\" ]; then \
                 if [ \"$p\" != \"$D/clutch\" ]; then mv -f \"$p\" \"$D/clutch\" 2>/dev/null || cp -f \"$p\" \"$D/clutch\"; fi; \
                 chmod 755 \"$D/clutch\" 2>/dev/null; [ -x \"$D/clutch\" ] && echo \"FOUND:$D/clutch\" || echo \"NOEXEC:$p\"; break; fi; done; true"
            ),
            |_| {},
        )
        .await?;
    if let Some(p) = out.lines().find_map(|l| l.strip_prefix("NOEXEC:")) {
        return Err(format!("Clutch found ({}) but could not be made executable (chmod failed).", p.trim()));
    }
    Ok(out.lines().find_map(|l| l.strip_prefix("FOUND:")).map(|s| s.trim().to_string()))
}

fn is_macho(b: &[u8]) -> bool {
    b.len() > 4096
        && matches!(b[..4], [0xca, 0xfe, 0xba, 0xbe] | [0xcf, 0xfa, 0xed, 0xfe] | [0xce, 0xfa, 0xed, 0xfe] | [0xbe, 0xba, 0xfe, 0xca])
}

/// The Clutch binary out of what the user picked: a Mach-O as-is, or the
/// clutch executable inside a .deb package (ar → data.tar.*).
fn clutch_binary(local: &std::path::Path) -> Result<PathBuf, String> {
    let head = std::fs::read(local).map_err(err)?;
    if is_macho(&head) {
        return Ok(local.to_path_buf());
    }
    if !head.starts_with(b"!<arch>\n") {
        return Err("The chosen file is neither a Clutch binary (Mach-O) nor a .deb package.".into());
    }
    let work = std::env::temp_dir().join(format!("ipacked-clutch-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(work.join("data")).map_err(err)?;
    let run = |prog: &str, args: &[&str], dir: &std::path::Path| {
        std::process::Command::new(prog).args(args).current_dir(dir).output().map(|o| o.status.success()).unwrap_or(false)
    };
    if !run("/usr/bin/ar", &["x", &local.to_string_lossy()], &work) {
        return Err("could not open .deb (ar)".into());
    }
    let data = std::fs::read_dir(&work).map_err(err)?.flatten().map(|e| e.path()).find(|p| {
        p.file_name().map(|n| n.to_string_lossy().starts_with("data.tar")).unwrap_or(false)
    });
    let data = data.ok_or("no data.tar inside the .deb")?;
    if !run("/usr/bin/tar", &["-xf", &data.to_string_lossy(), "-C", "data"], &work) {
        return Err("could not unpack .deb data (tar; for data.tar.zst install zstd first)".into());
    }
    fn walk(d: &std::path::Path, out: &mut Vec<PathBuf>) {
        for e in std::fs::read_dir(d).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() { walk(&p, out) } else { out.push(p) }
        }
    }
    let mut files = Vec::new();
    walk(&work.join("data"), &mut files);
    files
        .into_iter()
        .filter(|p| p.file_name().map(|n| n.to_string_lossy().to_lowercase().starts_with("clutch")).unwrap_or(false))
        .find(|p| std::fs::read(p).map(|b| is_macho(&b)).unwrap_or(false))
        .ok_or_else(|| "no Clutch binary in the .deb".into())
}

/// Install the Clutch the user supplied (binary or .deb) as `/bin/clutch`
/// (lower-case, mode 755; /var/jb/usr/bin/clutch on rootless).
pub async fn clutch_install(conn: &Conn, local: PathBuf) -> Result<String, String> {
    let file = clutch_binary(&local)?;
    conn.push(&file, "/tmp/clutch.ipacked").await?;
    let out = conn
        .exec(
            &format!(
                "{BIN_DIR}; for o in $D/Clutch $D/Clutch2; do [ -f $o ] && rm -f $o; done; \
                 mv -f /tmp/clutch.ipacked $D/clutch && chmod 755 $D/clutch && chown root:wheel $D/clutch 2>/dev/null; \
                 [ -x $D/clutch ] && echo OK:$D/clutch"
            ),
            |_| {},
        )
        .await?;
    match out.lines().find_map(|l| l.trim().strip_prefix("OK:")) {
        Some(p) => Ok(p.to_string()),
        None => Err(format!("could not copy Clutch: {}", out.trim())),
    }
}

/// Install any .deb the user picked (dpkg over SSH). Used for dumper packages.
pub async fn deb_install(conn: &Conn, local: PathBuf, mut log: impl FnMut(&str)) -> Result<String, String> {
    conn.push(&local, "/tmp/ipacked-pkg.deb").await?;
    let out = conn
        .exec(
            "P=dpkg; [ -x /var/jb/usr/bin/dpkg ] && P=/var/jb/usr/bin/dpkg; $P -i /tmp/ipacked-pkg.deb 2>&1; R=$?; rm -f /tmp/ipacked-pkg.deb; \
             (uicache -a || uicache) >/dev/null 2>&1; echo EXIT:$R",
            &mut log,
        )
        .await?;
    if out.contains("EXIT:0") {
        Ok("Package installed".into())
    } else {
        Err(out.lines().filter(|l| !l.starts_with("EXIT:")).collect::<Vec<_>>().join("\n"))
    }
}

/// First absolute path ending in ".ipa" inside a line of dumper output.
pub fn ipa_path_in(l: &str) -> Option<String> {
    let end = l.find(".ipa")? + 4;
    let start = l[..end].rfind(|c: char| c == ' ' || c == '"' || c == '\'' || c == ':' || c == '\t').map(|i| i + 1).unwrap_or(0);
    let p = l[start..end].trim();
    p.starts_with('/').then(|| p.to_string())
}

/// Dump one app with Clutch and copy the IPA to `out_dir` (device copy removed).
pub async fn clutch_dump(conn: &Conn, bin: &str, bundle_id: &str, out_dir: &std::path::Path, log: &dyn Fn(&str)) -> Result<PathBuf, String> {
    // Only the selected app. Clutch 2.x: `clutch -n -d <bundle>`; Clutch 1.x has
    // no -d and takes the bundle directly. (iOS lacks tee/head: no pipelines.)
    let q = bundle_id.replace('\'', "");
    let cmd = format!("V=$({bin} --version 2>&1); case \"$V\" in *2.*) {bin} -n -d '{q}' 2>&1;; *) {bin} '{q}' 2>&1;; esac");
    let mut remote: Option<String> = None;
    let out = conn
        .exec(&cmd, |l| {
            log(l);
            if let Some(p) = ipa_path_in(l) {
                remote = Some(p);
            }
        })
        .await?;
    if out.contains("Permission denied") {
        return Err(format!("could not run {bin} (Permission denied). Devices → Clutch → refresh; iPacked fixes the permissions."));
    }
    if remote.is_none() && out.contains("Killed: 9") {
        // The kernel kills Clutch when FairPlay refuses to decrypt
        // (`fairplayOpen() failed, error -42005`): the device has no key for
        // the Apple ID that bought this copy until that account is signed in
        // on the device (first launch asks for the password).
        let dm = conn.exec("dmesg 2>/dev/null | grep -c 'fairplayOpen() failed'", |_| {}).await.unwrap_or_default();
        log(&format!("FairPlay refused (dmesg: {} entries)", dm.trim()));
        return Err("FAIRPLAY_AUTH: The device is not authorized to decrypt this app (FairPlay -42005). The Apple ID that bought it isn't authorized on the device.".into());
    }
    let r = remote.ok_or_else(|| format!("Clutch produced no IPA:\n{}", out.trim()))?;
    let local = out_dir.join(std::path::Path::new(&r).file_name().unwrap_or_default());
    log("Copying IPA (scp)…");
    conn.pull(&r, &local).await?;
    let _ = conn.exec(&format!("rm -f '{}'", r.replace('\'', "'\\''")), |_| {}).await;
    Ok(local)
}

#[cfg(test)]
mod device_tests {
    /// Real device: IPACKED_TEST_UDID=.. IPACKED_TEST_BUNDLE=.. cargo test clutch_live -- --ignored --nocapture
    #[tokio::test]
    #[ignore]
    async fn clutch_live() {
        let udid = std::env::var("IPACKED_TEST_UDID").unwrap();
        let bundle = std::env::var("IPACKED_TEST_BUNDLE").unwrap();
        let conn = super::Conn::open(&udid).await.unwrap();
        let bin = super::clutch_path(&conn).await.unwrap().expect("no clutch");
        println!("clutch = {bin}");
        let dir = std::env::temp_dir().join("ipacked-clutch-test");
        std::fs::create_dir_all(&dir).unwrap();
        let ipa = super::clutch_dump(&conn, &bin, &bundle, &dir, &|l| println!("> {l}")).await.unwrap();
        let len = std::fs::metadata(&ipa).unwrap().len();
        println!("OK {} ({len} bytes)", ipa.display());
        assert!(len > 10_000);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
