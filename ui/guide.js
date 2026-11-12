// Step-by-step guides for dumping apps from a jailbroken device (Clutch,
// bagbak/Frida) and installing packages. Starts from an already jailbroken
// device — no jailbreak instructions here.

function showGuide(key = "clutch") {
  const m = $("#modal");
  const G = GUIDES_EN;
  const tabs = Object.entries(G)
    .map(([k, g]) => `<button class="seg ${k === key ? "on" : ""}" data-g="${k}">${esc(g.title)}</button>`)
    .join("");
  const g = G[key];
  $("#modal-body").innerHTML = `<div class="guide">
      <div class="segs">${tabs}</div>
      <p class="sub">${g.intro}</p>
      <ol class="steps">${g.steps.map(([h, b]) => `<li><b>${h}</b><div class="sel">${b}</div></li>`).join("")}</ol>
      <div class="notice">${sf("exclamationmark.triangle.fill")}<div>Use decrypted IPAs only as backups of apps you bought yourself.</div></div>
    </div><div class="row-btns"><button class="btn primary" value="ok">Close</button></div>`;
  $$("[data-g]", m).forEach((b) => (b.onclick = (e) => (e.preventDefault(), showGuide(b.dataset.g))));
  if (!m.open) m.showModal();
  m.onclose = null;
}
$("#dev-setdev").onclick = () => renderDumpCard(true);

// ---------- dump methods by iOS version ----------
// auto: iPacked can run it itself (Export / Decrypt on device).
const DUMP_METHODS = [
  { id: "archive", name: "ideviceinstaller archive", ios: [3, 6], jb: false, auto: true, guide: "archive", 
    req: ["USB connection", "ideviceinstaller on this Mac"], note: "No jailbreak. Backs up the IPA as installed (still FairPlay encrypted)." },
  { id: "clutch", name: "Clutch 1.4.x", ios: [5, 7], jb: true, auto: true, pkg: "clutch", guide: "clutch",
    req: ["Jailbreak", "OpenSSH (Cydia)", "Clutch 1.4.x file", "App authorized on the device (FairPlay)"], note: "32-bit devices. iPacked installs it as /bin/clutch and runs it over SSH." },
  { id: "clutch2", name: "Clutch 2.0.4", ios: [6, 11], jb: true, auto: true, pkg: "clutch", guide: "clutch",
    req: ["Jailbreak", "OpenSSH (Cydia)", "Clutch 2.0.4 file (github.com/KJCracks/Clutch/releases)", "App authorized on the device (FairPlay)"], note: "iPacked installs it as /bin/clutch and runs it over SSH." },
  { id: "dumpdecrypted", name: "dumpdecrypted", ios: [4, 9], jb: true, guide: "dumpdecrypted", 
    req: ["Jailbreak", "OpenSSH", "dumpdecrypted.dylib built for the device"], note: "Manual: DYLD_INSERT_LIBRARIES=dumpdecrypted.dylib <app binary> over SSH. Writes a decrypted binary that you repack yourself." },
  { id: "flexdecrypt", name: "flexdecrypt", ios: [11, 14], jb: true, pkg: "deb", guide: "flexdecrypt", 
    req: ["Jailbreak", "OpenSSH", "flexdecrypt .deb (github.com/JohnCoates/flexdecrypt)"], note: "Manual over SSH after install: flexdecrypt <binary>." },
  { id: "bfdecrypt", name: "bfdecrypt", ios: [11, 13], jb: true, pkg: "deb", guide: "bfdecrypt", 
    req: ["Jailbreak", "bfdecrypt .deb (github.com/BishopFox/bfdecrypt)"], note: "Pick the app in the device's Settings. The IPA lands in the app's Documents folder (get it in Files)." },
  { id: "crackerxi", name: "CrackerXI+", ios: [11, 16], jb: true, repo: "https://cydia.iphonecake.com", guide: "crackerxi", 
    req: ["Jailbreak", "CrackerXI+ from its repo"], note: "On-device UI. Writes IPAs to /var/mobile/Documents/CrackerXI (get them in Files)." },
  { id: "fridadump", name: "frida-ios-dump", ios: [8, 16], jb: true, guide: "fridadump",
    req: ["Jailbreak", "frida-server on the device (Frida 16 or older)", "python3 + frida-ios-dump on this Mac"], note: "Manual: python3 dump.py <bundle id> over USB. Unmaintained; doesn't work with Frida 17+." },
  { id: "bagbak", name: "bagbak + Frida", ios: [12, 17], jb: true, auto: true, repo: "https://build.frida.re", guide: "bagbak",
    req: ["Jailbreak", "frida-server on the device", "bagbak on this Mac (npm i -g bagbak)", "USB connection"], note: "iPacked runs bagbak over USB." },
  { id: "trolldecrypt", name: "TrollDecrypt", ios: [14, 17], jb: false, guide: "trolldecrypt", 
    req: ["TrollStore (iOS 14.0–16.6.1 or 17.0)", "TrollDecrypt"], note: "No jailbreak. On-device UI; decrypted IPAs are saved in its Documents folder." },
];

function dumpMethodsFor(major) {
  return DUMP_METHODS.filter((m) => major >= m.ios[0] && major <= m.ios[1]);
}

/// Devices → "Set Device": every dump method with its requirements. Methods
/// this iOS version can't use are listed but disabled.
async function renderDumpCard(toggle) {
  const id = $("#dev-select").value;
  const el = $("#dev-dump");
  if (toggle === true && !el.classList.contains("hidden")) return el.classList.add("hidden");
  if (toggle !== true && el.classList.contains("hidden")) return;
  if (!id) return el.classList.add("hidden");
  let d;
  try {
    d = await invoke("device_details", { id });
  } catch {
    return el.classList.add("hidden");
  }
  const major = parseInt(d.iosVersion) || 0;
  const ok = (m) => major >= m.ios[0] && major <= m.ios[1];
  // Best fit first: usable methods iPacked runs itself, then other usable ones
  // (narrowest iOS range first = most specific), then the rest.
  const score = (m) => (ok(m) ? 100 : 0) + (ok(m) && m.auto ? 50 : 0) + (ok(m) && !m.jb ? 5 : 0) - (m.id === "archive" ? 10 : 0) - (m.ios[1] - m.ios[0]); // archive keeps FairPlay → after real decrypters
  const ms = [...DUMP_METHODS].sort((x, y) => score(y) - score(x));
  el.classList.remove("hidden");
  el.innerHTML = `<div class="dump-head"><b>${sf("wrench.and.screwdriver")} Set Device · ${esc(d.name)} · iOS ${esc(d.iosVersion)} (${esc(d.productType)}, ${esc(d.cpu)})</b>
      <button class="btn small" data-k="deb">${sf("shippingbox")}Install .deb package…</button>
      <button class="btn small icon-only" data-k="x">${sf("xmark.circle.fill")}</button></div>
    <div class="dump-list">${ms
      .map(
        (m) => `<div class="dump-m ${ok(m) ? "" : "off"}" ${ok(m) ? "" : `title="Needs iOS ${m.ios[0]}–${m.ios[1]}; this device runs iOS ${esc(d.iosVersion)}"`}>
          <div><b>${esc(m.name)}</b> <span class="pill">iOS ${m.ios[0]}–${m.ios[1]}</span> ${ok(m) ? (m.auto ? `<span class="pill ok">Done by iPacked</span>` : `<span class="pill">manual</span>`) : `<span class="pill bad">Not for this iOS</span>`} ${m.jb ? `<span class="pill warn">jailbreak</span>` : `<span class="pill ok">no jailbreak</span>`}</div>
          <div class="sub">${esc(m.note)}${m.repo ? ` Repo: <span class="mono sel">${esc(m.repo)}</span>` : ""}</div>
          <div class="sub"><b>Needs:</b></div><ul class="req">${m.req.map((r) => `<li>${esc(r)}</li>`).join("")}</ul>
          <div class="row">${m.pkg === "clutch" ? `<button class="btn small" data-k="clutch" ${ok(m) ? "" : "disabled"}>${sf("arrow.up.doc")}Provide the Clutch file…</button>` : m.pkg === "deb" ? `<button class="btn small" data-k="deb" ${ok(m) ? "" : "disabled"}>${sf("shippingbox")}Install ${esc(m.name)} .deb…</button>` : ""}
          ${m.guide ? `<button class="btn small" data-k="guide" data-g="${m.guide}">${sf("book")}Guide</button>` : ""}</div></div>`
      )
      .join("")}</div>`;
  $$("[data-k]", el).forEach(
    (b) =>
      (b.onclick = async () => {
        const k = b.dataset.k;
        if (k === "x") return el.classList.add("hidden");
        if (k === "guide") return showGuide(b.dataset.g);
        if (k === "clutch") return clutchFileDialog();
        const f = await T.dialog.open({ multiple: false, filters: [{ name: "Debian package", extensions: ["deb"] }] });
        if (!f) return;
        const log = $("#dev-log");
        log.textContent = `Installing package: ${f.split("/").pop()}\n`;
        log.classList.remove("hidden");
        try {
          toast(await invoke("deb_install", { id, path: f }));
        } catch (e) {
          if (/SSH/.test(String(e))) sshMissing(e);
          else fail("Package install failed: " + e);
        }
      })
  );
}

// ---------- AFC file browser: Finder / iFunBox style ----------
S.fs = { mode: "afc", path: "/", hist: [], fwd: [], sel: new Set(), sort: "name", asc: true, view: "list" };
const FS_KIND = [
  [/\.(png|jpe?g|gif|heic|bmp|tiff?|webp)$/i, "photo", "Image"],
  [/\.(mp3|m4a|aac|wav|caf|aiff?|m4r)$/i, "music.note", "Audio"],
  [/\.(mov|mp4|m4v|avi)$/i, "film", "Video"],
  [/\.(ipa|deb|zip|tar|gz|tgz|bz2|xz|7z|rar)$/i, "doc.zipper", "Archive"],
  [/\.(txt|log|plist|json|xml|strings|h|m|c|sh|conf|md)$/i, "doc.text", "Text"],
  [/\.(sqlite|sqlitedb|db)$/i, "externaldrive", "Database"],
  [/\.app$/i, "app.fill", "App"],
];
function fsKind(f) {
  if (f.dir) return /\.app$/i.test(f.name) ? ["app.fill", "App"] : ["folder.fill", "Folder"];
  if (f.link) return ["link", "Alias"];
  const k = FS_KIND.find(([r]) => r.test(f.name));
  return k ? [k[1], k[2]] : ["doc", (f.name.includes(".") ? f.name.split(".").pop().toUpperCase() + " " : "") + "File"];
}
const fsJoin = (cwd, n) => (cwd === "/" ? "" : cwd) + "/" + n;

function fsGo(path, mode) {
  if (path === S.fs.path && (!mode || mode === S.fs.mode)) return renderFs();
  S.fs.hist.push({ mode: S.fs.mode, path: S.fs.path });
  S.fs.fwd = [];
  if (mode) S.fs.mode = mode;
  S.fs.path = path || "/";
  S.fs.sel.clear();
  renderFs();
}

async function renderFs() {
  const id = $("#dev-select").value;
  const el = $("#dev-fs");
  if (!id || el.classList.contains("hidden")) return;
  const dev = S.devices.find((x) => x.id === id);
  const fs = S.fs;
  const parts = fs.path.split("/").filter(Boolean);
  const haApp = fs.mode.startsWith("ha:") ? (S.deviceApps || []).find((a) => "ha:" + a.bundleId === fs.mode) : null;
  const crumbs = (fs.mode.startsWith("ha:")
    ? [`<a data-loc2="apps:/">${sf("app.fill")}App data</a>`, `<a data-p="/">${esc(haApp?.name || fs.mode.slice(3))}</a>`]
    : [`<a data-p="/">${sf("iphone")}${esc(dev?.name || "Device")}</a>`])
    .concat(parts.map((p, i) => `<a data-p="/${parts.slice(0, i + 1).map(encodeURIComponent).join("/")}">${esc(p)}</a>`))
    .join(`<span class="dim crumb-sep">›</span>`);
  const jb = fs.mode === "afc2" && fs.path.startsWith("/var/jb");
  el.innerHTML = `<div class="fs-side">
      <div class="fs-sec">Locations</div>
      <a class="fs-loc ${fs.mode === "afc" ? "on" : ""}" data-loc="afc:/" title="Media folder, no jailbreak needed">${sf("folder.fill")}Media</a>
      <a class="fs-loc ${fs.mode === "afc2" && !jb ? "on" : ""}" data-loc="afc2:/" title="Whole file system (AFC2, rootful jailbreak)">${sf("externaldrive")}Root (/)</a>
      <a class="fs-loc ${jb ? "on" : ""}" data-loc="afc2:/var/jb" title="Rootless jailbreak: /var/jb">${sf("lock.open.fill")}Rootless</a>
      <div class="fs-sec">Shortcuts</div>
      <a class="fs-loc" data-loc="afc:/DCIM">${sf("photo")}Photos (DCIM)</a>
      <a class="fs-loc" data-loc="afc:/Downloads">${sf("arrow.down.circle")}Downloads</a>
      <a class="fs-loc" data-loc="afc:/iTunes_Control/Music">${sf("music.note")}Music</a>
      <a class="fs-loc" data-loc="afc2:/var/mobile/Documents">${sf("doc")}Documents</a>
      <a class="fs-loc ${fs.mode === "apps" || fs.mode.startsWith("ha:") ? "on" : ""}" data-loc="apps:/" title="Each app's own folder (like iTunes File Sharing, no jailbreak needed)">${sf("app.fill")}App data</a>
      <a class="fs-loc" data-loc="afc2:/var/mobile/Applications">${sf("app.fill")}Apps (iOS ≤7)</a>
      <a class="fs-loc" data-loc="afc2:/Applications">${sf("app.badge")}System apps</a>
    </div>
    <div class="fs-main">
    <div class="fs-bar">
      <div class="seg"><button class="seg-btn" data-f="back" title="Back" ${fs.hist.length ? "" : "disabled"}>${sf("chevron.left")}</button><button class="seg-btn" data-f="fwd" title="Forward" ${fs.fwd.length ? "" : "disabled"}>${sf("chevron.right")}</button></div>
      <div class="crumbs">${crumbs}</div>
      <div class="seg"><button class="seg-btn ${fs.view === "list" ? "active" : ""}" data-v="list" title="List">${sf("list.bullet")}</button><button class="seg-btn ${fs.view === "icons" ? "active" : ""}" data-v="icons" title="Icons">${sf("square.grid.2x2")}</button></div>
      <button class="btn small icon-only" data-f="put" title="Upload from Mac (or drag files here)">${sf("square.and.arrow.up")}</button>
      <button class="btn small icon-only" data-f="get" title="Download selected to Mac">${sf("arrow.down.circle")}</button>
      <button class="btn small icon-only" data-f="mkdir" title="New folder">${sf("folder.badge.plus")}</button>
      <button class="btn small icon-only danger" data-f="rm" title="Delete selected">${sf("trash")}</button>
      <button class="btn small icon-only" data-f="re" title="Refresh">${sf("arrow.clockwise")}</button></div>
    <div class="fs-list" tabindex="0"><div class="empty">Reading…</div></div>
    <div class="fs-status dim"></div></div>`;
  const cwd = fs.path;
  const op = (o, a, b) => invoke("afc_op", { id, mode: fs.mode, op: o, a, b: b ?? null });
  $$("[data-loc]", el).forEach((a) => (a.onclick = () => { const [m, p] = a.dataset.loc.split(":"); fsGo(p, m); }));
  $$(".crumbs a[data-p]", el).forEach((a) => (a.onclick = () => fsGo(decodeURIComponent(a.dataset.p))));
  $$(".crumbs a[data-loc2]", el).forEach((a) => (a.onclick = () => fsGo("/", "apps")));
  $$("[data-v]", el).forEach((b) => (b.onclick = () => ((fs.view = b.dataset.v), renderFs())));
  $("[data-f=back]", el).onclick = () => { const h = fs.hist.pop(); if (!h) return; fs.fwd.push({ mode: fs.mode, path: fs.path }); Object.assign(fs, h); fs.sel.clear(); renderFs(); };
  $("[data-f=fwd]", el).onclick = () => { const h = fs.fwd.pop(); if (!h) return; fs.hist.push({ mode: fs.mode, path: fs.path }); Object.assign(fs, h); fs.sel.clear(); renderFs(); };
  $("[data-f=re]", el).onclick = renderFs;

  let items;
  // App icons/names: virtual "apps" list, ≤iOS 7 GUID folders, system .app folders.
  const deco = {};
  try {
    if (fs.mode === "apps") {
      if (!S.deviceApps?.length) S.deviceApps = await invoke("device_apps", { id });
      items = S.deviceApps.map((a) => ((deco[a.bundleId] = { label: a.name, bid: a.bundleId }), { name: a.bundleId, dir: true, size: 0, mtime: 0 }));
    } else {
      items = await invoke("afc_ls", { id, mode: fs.mode, path: cwd });
      const p = cwd.replace(/^\/private/, "");
      if (fs.mode === "afc2" && p === "/var/mobile/Applications") {
        if (!S.deviceApps?.length) S.deviceApps = await invoke("device_apps", { id }).catch(() => []);
        for (const a of S.deviceApps) {
          const guid = (a.container || "").split("/").pop();
          if (guid) deco[guid] = { label: a.name, bid: a.bundleId };
        }
      } else if (fs.mode === "afc2" && p === "/Applications") {
        S.sysApps ||= {};
        S.sysApps[id] ||= await invoke("device_system_apps", { id }).catch(() => []);
        for (const a of S.sysApps[id]) {
          const dir = (a.path || "").split("/").pop();
          if (dir) deco[dir] = { label: a.name, bid: a.bundleId };
        }
      }
    }
  } catch (e) {
    $(".fs-list", el).innerHTML = `<div class="empty sel">${esc(e)}${fs.mode === "afc2" ? `<p class="sub wrap">Root access needs the <b>Apple File Conduit "2"</b> package (rootful jailbreak) on the device.</p>` : ""}</div>`;
    return;
  }
  const byName = (a, b) => a.name.localeCompare(b.name, undefined, { numeric: true, sensitivity: "base" });
  const key = { name: byName, date: (a, b) => (a.mtime || 0) - (b.mtime || 0), size: (a, b) => (a.size || 0) - (b.size || 0), kind: (a, b) => fsKind(a)[1].localeCompare(fsKind(b)[1]) }[fs.sort];
  const isD = (f) => (f.dir || f.link ? 1 : 0);
  items.sort((a, b) => isD(b) - isD(a) || (fs.asc ? 1 : -1) * key(a, b) || byName(a, b));
  const isDir = (f) => !!(f.dir || f.link);
  const arrow = (k) => (fs.sort === k ? (fs.asc ? " ▲" : " ▼") : "");
  const list = $(".fs-list", el);
  const row = (f) => {
    const [ic, kind] = fsKind(f);
    const q = esc(f.name.toLowerCase());
    return fs.view === "icons"
      ? `<div class="fs-tile ${fs.sel.has(f.name) ? "sel" : ""}" data-n="${esc(f.name)}" data-dir="${isDir(f) ? 1 : 0}" data-q="${q}" title="${esc(f.name)}">${deco[f.name] ? `<span class="icon lg" data-fsicon="${esc(deco[f.name].bid)}"></span><span class="nm">${esc(deco[f.name].label)}</span>` : `<span class="fs-big k-${ic.replace(/\./g, "-")}">${sf(ic)}</span><span class="nm">${esc(f.name)}</span>`}</div>`
      : `<tr class="${fs.sel.has(f.name) ? "sel" : ""}" data-n="${esc(f.name)}" data-dir="${isDir(f) ? 1 : 0}" data-q="${q}">
        <td class="namecol">${deco[f.name] ? `<span class="icon sm" data-fsicon="${esc(deco[f.name].bid)}"></span><span class="nm">${esc(deco[f.name].label)}</span> <span class="dim">${esc(f.name)}</span>` : `<span class="fs-ico k-${ic.replace(/\./g, "-")}">${sf(ic)}</span><span class="nm">${esc(f.name)}</span>`}${f.link && f.target ? ` <span class="dim">→ ${esc(f.target)}</span>` : ""}</td>
        <td>${f.mtime ? new Date(f.mtime * 1000).toLocaleString(LOC) : "—"}</td><td>${isDir(f) ? "—" : fmtSize(f.size)}</td><td class="dim">${esc(kind)}</td></tr>`;
  };
  list.innerHTML = !items.length
    ? `<div class="empty">Empty folder · drag files here to upload</div>`
    : fs.view === "icons"
      ? `<div class="fs-grid">${items.map(row).join("")}</div>`
      : `<table class="ftable"><thead><tr><th data-s="name">Name${arrow("name")}</th><th data-s="date">Modified${arrow("date")}</th><th data-s="size">Size${arrow("size")}</th><th data-s="kind">Kind${arrow("kind")}</th></tr></thead><tbody>${items.map(row).join("")}</tbody></table>`;
  // Icons for decorated rows (3 at a time).
  (async () => {
    const cells = $$("[data-fsicon]", list);
    let i = 0;
    const w = async () => {
      while (i < cells.length) {
        const c = cells[i++];
        const bid = c.dataset.fsicon;
        let p = await invoke("device_icon", { id, bundleId: bid }).catch(() => null);
        if (!p) p = S.archive.find((e) => e.bundleId === bid && e.iconPath)?.iconPath;
        if (p) c.innerHTML = `<img src="${fileSrc(p)}?v=1">`;
      }
    };
    await Promise.all([w(), w(), w()]);
  })();
  $$("th[data-s]", list).forEach((th) => (th.onclick = () => ((fs.asc = fs.sort === th.dataset.s ? !fs.asc : true), (fs.sort = th.dataset.s), renderFs())));
  const status = () => {
    const sel = items.filter((f) => fs.sel.has(f.name));
    const total = items.filter((f) => !isDir(f)).reduce((s, f) => s + (f.size || 0), 0);
    $(".fs-status", el).textContent = sel.length
      ? `${sel.length} / ${items.length} items selected · ${fmtSize(sel.filter((f) => !isDir(f)).reduce((s, f) => s + (f.size || 0), 0))}`
      : `${items.length} items · ${fmtSize(total)}`;
  };
  status();
  const rows = () => $$("[data-n]", list);
  const paintSel = () => (rows().forEach((r) => r.classList.toggle("sel", fs.sel.has(r.dataset.n))), status());
  let anchor = null;
  const selected = () => items.filter((f) => fs.sel.has(f.name));
  const open = (f) => (fs.mode === "apps" ? fsGo("/", "ha:" + f.name) : isDir(f) ? fsGo(fsJoin(cwd, f.name)) : download([f]));
  const download = async (fl) => {
    fl = fl.filter((f) => !isDir(f));
    if (!fl.length) return toast("Select files to download (not folders)");
    let dest;
    if (fl.length === 1) dest = await T.dialog.save({ defaultPath: fl[0].name });
    else dest = await T.dialog.open({ directory: true, title: "Destination folder" });
    if (!dest) return;
    for (const f of fl) {
      const to = fl.length === 1 ? dest : dest + "/" + f.name;
      await op("get", fsJoin(cwd, f.name), to).then(() => toast("Downloaded: " + f.name)).catch(fail);
    }
  };
  const remove = async (fl) => {
    if (!fl.length) return;
    if (!(await ask(`<h3>${fl.length} items — delete?</h3><p class="mono wrap">${fl.map((f) => esc(fsJoin(cwd, f.name))).join("<br>")}</p><p class="sub">It will be permanently deleted from the device; this can't be undone.</p>`, "Delete"))) return;
    for (const f of fl) await op("rm", fsJoin(cwd, f.name)).catch(fail);
    fs.sel.clear();
    renderFs();
  };
  const rename = async (f) => {
    const r = await ask(`<h3>Rename</h3><label>New name<input name="n" value="${esc(f.name)}"></label>`, "Save");
    if (r?.n && r.n !== f.name) await op("mv", fsJoin(cwd, f.name), fsJoin(cwd, r.n)).then(renderFs).catch(fail);
  };
  const upload = async (files) => {
    for (const f of files) await op("put", f, fsJoin(cwd, f.split("/").pop())).then(() => toast("Uploaded: " + f.split("/").pop())).catch(fail);
    renderFs();
  };
  S.fsUpload = upload; // drag & drop from Finder
  $("[data-f=put]", el).onclick = async () => upload([await T.dialog.open({ multiple: true })].flat().filter(Boolean));
  $("[data-f=get]", el).onclick = () => download(selected());
  $("[data-f=rm]", el).onclick = () => remove(selected());
  $("[data-f=mkdir]", el).onclick = async () => {
    const r = await ask(`<h3>New folder</h3><label>Name<input name="n" value="New folder"></label>`, "Create");
    if (r?.n) await op("mkdir", fsJoin(cwd, r.n)).catch(fail), renderFs();
  };
  rows().forEach((r) => {
    const f = items.find((x) => x.name === r.dataset.n);
    r.onclick = (ev) => {
      if (ev.metaKey) fs.sel.has(f.name) ? fs.sel.delete(f.name) : fs.sel.add(f.name);
      else if (ev.shiftKey && anchor) {
        const names = rows().map((x) => x.dataset.n);
        const [i, j] = [names.indexOf(anchor), names.indexOf(f.name)].sort((a, b) => a - b);
        names.slice(i, j + 1).forEach((n) => fs.sel.add(n));
      } else fs.sel = new Set([f.name]);
      if (!ev.shiftKey) anchor = f.name;
      paintSel();
    };
    r.ondblclick = () => open(f);
    r.oncontextmenu = (ev) => {
      ev.preventDefault();
      if (!fs.sel.has(f.name)) (fs.sel = new Set([f.name])), paintSel();
      const sel = selected();
      fsMenu(ev.clientX, ev.clientY, [
        sel.length === 1 && isDir(f) && ["Open", "folder", () => open(f)],
        ["Download to Mac…", "arrow.down.circle", () => download(sel)],
        sel.length === 1 && ["Rename…", "pencil", () => rename(f)],
        ["Copy path", "doc.on.doc", () => navigator.clipboard.writeText(sel.map((x) => fsJoin(cwd, x.name)).join("\n")).then(() => toast("Copied"))],
        "-",
        ["Delete…", "trash", () => remove(sel), true],
      ]);
    };
  });
  list.oncontextmenu = (ev) => {
    if (ev.target.closest("[data-n]")) return;
    ev.preventDefault();
    fsMenu(ev.clientX, ev.clientY, [
      ["New folder…", "folder.badge.plus", () => $("[data-f=mkdir]", el).click()],
      ["Upload from Mac…", "square.and.arrow.up", () => $("[data-f=put]", el).click()],
      ["Copy path", "doc.on.doc", () => navigator.clipboard.writeText(cwd)],
      ["Refresh", "arrow.clockwise", renderFs],
    ]);
  };
  list.onclick = (ev) => !ev.target.closest("[data-n]") && (fs.sel.clear(), paintSel());
  list.onkeydown = (ev) => {
    const sel = selected();
    if (ev.key === "Backspace" && ev.metaKey) return ev.preventDefault(), remove(sel);
    if (ev.key === "Enter" && sel.length === 1) return ev.preventDefault(), rename(sel[0]);
    if (ev.key === "ArrowDown" && ev.metaKey && sel.length === 1) return ev.preventDefault(), open(sel[0]);
    if (ev.key === "ArrowUp" && ev.metaKey) return ev.preventDefault(), fsGo("/" + parts.slice(0, -1).join("/"));
    if (ev.key === "a" && ev.metaKey) return ev.preventDefault(), (fs.sel = new Set(items.map((f) => f.name))), paintSel();
    if (ev.key === "ArrowDown" || ev.key === "ArrowUp") {
      ev.preventDefault();
      const names = rows().map((x) => x.dataset.n);
      const cur = names.indexOf([...fs.sel].pop());
      const n = names[Math.max(0, Math.min(names.length - 1, cur + (ev.key === "ArrowDown" ? 1 : -1)))];
      if (n) (fs.sel = new Set([n])), (anchor = n), paintSel(), rows()[names.indexOf(n)].scrollIntoView({ block: "nearest" });
    }
  };
  filterDevice();
}

function fsMenu(x, y, items) {
  $("#ctx")?.remove();
  const m = document.createElement("div");
  m.id = "ctx";
  m.className = "ctx";
  m.innerHTML = items
    .filter(Boolean)
    .map((it, i) => (it === "-" ? `<hr>` : `<button data-i="${i}" class="${it[3] ? "danger" : ""}">${sf(it[1])}${esc(it[0])}</button>`))
    .join("");
  document.body.appendChild(m);
  const list = items.filter(Boolean);
  $$("button", m).forEach((b) => (b.onclick = () => (m.remove(), list[+b.dataset.i][2]())));
  const r = m.getBoundingClientRect();
  m.style.left = Math.min(x, innerWidth - r.width - 6) + "px";
  m.style.top = Math.min(y, innerHeight - r.height - 6) + "px";
  setTimeout(() => document.addEventListener("click", () => m.remove(), { once: true }));
}

// Files dragged in from Finder → upload into the open folder.
listen("tauri://drag-drop", (p) => {
  const paths = p?.paths || [];
  if (!paths.length || $("#dev-fs").classList.contains("hidden") || !$("#view-devices").classList.contains("active")) return;
  S.fsUpload?.(paths);
});
listen("tauri://drag-enter", () => $("#dev-fs")?.classList.add("drop"));
listen("tauri://drag-leave", () => $("#dev-fs")?.classList.remove("drop"));
listen("tauri://drag-drop", () => $("#dev-fs")?.classList.remove("drop"));

$$("[data-devtab]").forEach(
  (b) =>
    (b.onclick = () => {
      const files = b.dataset.devtab === "files";
      $$("[data-devtab]").forEach((x) => x.classList.toggle("active", x === b));
      $("#dev-fs").classList.toggle("hidden", !files);
      $("#dev-apps").classList.toggle("hidden", files);
      if (files) renderFs();
    })
);

const GUIDES_EN = {
  clutch: {
    title: "Decrypted IPA with Clutch",
    intro: "Clutch runs the app on the device itself, strips FairPlay and writes an IPA there; iPacked pulls it over SSH and archives it. Clutch 2.0.4 works best on iOS 8–11; use bagbak on newer iOS.",
    steps: [
      ["Install OpenSSH", "Cydia → Search → <b>OpenSSH</b> → Install. On rootless jailbreaks (Dopamine, palera1n) with Sileo/Zebra the package is also <b>OpenSSH</b>."],
      ["Change the root password", "The default is <span class=mono>alpine</span>. Open NewTerm/MobileTerminal on the device:<pre>su root   # password: alpine\npasswd    # new password</pre>On rootless, log in as <span class=mono>mobile</span> and run <span class=mono>sudo passwd root</span>."],
      ["Find the device IP", "Settings → Wi-Fi → (i) next to the network → <b>IP Address</b>. Mac and device must be on the same network."],
      ["Enter SSH details in iPacked", "Devices → select the device → <b>SSH</b>. IP: &lt;your_ip&gt;, port 22, user root, your password. Leave the IP empty to use USB (iproxy; needs <span class=mono>brew install libimobiledevice</span>)."],
      ["Install Clutch", "Download Clutch yourself (iOS 8–11: Clutch 2.0.4 from github.com/KJCracks/Clutch/releases, iOS 5–7: Clutch 1.4.x). Then <b>Install Clutch…</b> in Devices and pick the file or a .deb that contains it. iPacked places it as <span class=mono>/bin/clutch</span> (lower-case, mode 755; <span class=mono>/var/jb/usr/bin/clutch</span> on rootless). By hand:<pre>scp -O Clutch-2.0.4 root@&lt;your_ip&gt;:/bin/clutch\nssh root@&lt;your_ip&gt; chmod 755 /bin/clutch</pre>"],
      ["Check", "The toolbar shows a green <b>Clutch installed</b>. From a terminal: <pre>ssh root@&lt;your_ip&gt; clutch -i</pre>"],
      ["Dump", "App list → <b>Export</b> → <b>Clutch</b>, or Archive → <b>Decrypt on device</b>. iPacked runs <span class=mono>clutch -d &lt;bundle id&gt;</span>, copies the IPA with scp, deletes the device copy and archives it as decrypted."],
      ["Troubleshooting", "<b>SSH failed:</b> check IP, password and Wi-Fi; a locked device may sleep Wi-Fi.<br><b>Segmentation fault / FAILED:</b> open and close the app once, retry; on iOS 12+ use bagbak.<br><b>Permission denied:</b> press refresh next to Clutch — iPacked renames it to clutch and fixes the mode."],
    ],
  },
  bagbak: {
    title: "Decrypted IPA with bagbak + Frida",
    intro: "bagbak launches the app via Frida and pulls the decrypted code from memory to the Mac, extensions included. Works on modern iOS (15–17) and rootless jailbreaks, over USB.",
    steps: [
      ["Add the Frida repo", "Cydia/Sileo/Zebra → Sources → Add: <pre>https://build.frida.re</pre>"],
      ["Install frida-server", "Install the <b>Frida</b> package from that repo (same package for rootless). frida-server then runs in the background."],
      ["Install tools on the Mac", "<pre>brew install node libimobiledevice\nnpm install -g bagbak\npip3 install frida-tools   # optional, for testing</pre>Keep the Mac frida close to the device version."],
      ["Test the connection", "Connect over USB, unlock, Trust this computer, then:<pre>frida-ps -Uai</pre>"],
      ["Dump", "App row → <b>Export</b> → <b>bagbak + Frida</b>. The app opens briefly on the device. Terminal:<pre>bagbak -o ~/Desktop/app.ipa com.example.app</pre>"],
      ["Troubleshooting", "<b>unable to connect to remote frida-server:</b> reinstall Frida or reboot and re-jailbreak.<br><b>Version mismatch:</b> <span class=mono>npm i -g frida bagbak</span> and update Frida on the device.<br><b>App quits at once:</b> jailbreak detection — disable tweaks for it with Choicy/Shadow."],
    ],
  },
  deb: {
    title: "Installing a .deb on the device",
    intro: "Three ways to install a .deb that isn't in a repo:",
    steps: [
      ["iPacked", "Devices → <b>Install .deb package…</b> (in the dump methods card). Uploads over SSH and runs <span class=mono>dpkg -i</span> + <span class=mono>uicache</span>."],
      ["SSH + dpkg", "<pre>scp -O package.deb root@&lt;your_ip&gt;:/tmp/\nssh root@&lt;your_ip&gt; dpkg -i /tmp/package.deb\nssh root@&lt;your_ip&gt; uicache\nssh root@&lt;your_ip&gt; killall SpringBoard</pre>Missing dependencies: <span class=mono>apt-get -f install</span>."],
      ["Filza", "Move the file to the device (AirDrop, Safari) and tap it in Filza → <b>Install</b>."],
    ],
  },
};
Object.assign(GUIDES_EN, {
  archive: {
    title: "IPA backup (ideviceinstaller)",
    intro: "iOS 3–6 without a jailbreak: the device hands back the installed app as an IPA. It stays FairPlay encrypted (runs only on devices authorized for the buying Apple ID).",
    steps: [
      ["Connect over USB", "Plug the device in, unlock it and tap <b>Trust</b>."],
      ["Install the tools on this Mac", "<pre>brew install ideviceinstaller libimobiledevice</pre>"],
      ["Back up", "Devices → Apps → pick an app → <b>Export</b> → <b>ideviceinstaller archive</b>. iPacked adds the IPA to the archive."],
    ],
  },
  dumpdecrypted: {
    title: "dumpdecrypted",
    intro: "Old manual method (iOS 4–9): a dylib injected into the app writes its decrypted binary. You rebuild the IPA yourself.",
    steps: [
      ["Prepare", "Jailbreak + OpenSSH. Build <span class=mono>dumpdecrypted.dylib</span> for the device's iOS (github.com/stefanesser/dumpdecrypted) and copy it to <span class=mono>/var/root</span> with Devices → Files."],
      ["Find the app", "Over SSH: <pre>find /var/mobile/Applications -name '*.app'</pre>"],
      ["Dump", "<pre>cd /var/root\nDYLD_INSERT_LIBRARIES=dumpdecrypted.dylib /var/mobile/Applications/&lt;UUID&gt;/&lt;App&gt;.app/&lt;App&gt;</pre>It writes <span class=mono>&lt;App&gt;.decrypted</span>."],
      ["Rebuild the IPA", "Copy the original .app into <span class=mono>Payload/</span>, replace its binary with the .decrypted file (same name), zip <span class=mono>Payload</span> as .ipa and import it in IPA Archive."],
    ],
  },
  flexdecrypt: {
    title: "flexdecrypt",
    intro: "iOS 11–14, jailbroken. Decrypts a Mach-O on the device; you repack the IPA.",
    steps: [
      ["Install", "Download the .deb from github.com/JohnCoates/flexdecrypt/releases → Set Device → <b>Install flexdecrypt .deb…</b>."],
      ["Decrypt", "Over SSH: <pre>flexdecrypt /var/containers/Bundle/Application/&lt;UUID&gt;/&lt;App&gt;.app/&lt;App&gt;</pre>The decrypted binary goes to /tmp."],
      ["Repack", "Replace the binary inside a copy of the .app, put it in <span class=mono>Payload/</span>, zip as .ipa and import it."],
    ],
  },
  bfdecrypt: {
    title: "bfdecrypt",
    intro: "iOS 11–13, jailbroken. A tweak that writes a decrypted IPA when the chosen app launches.",
    steps: [
      ["Install", "Download the .deb from github.com/BishopFox/bfdecrypt → Set Device → <b>Install bfdecrypt .deb…</b>."],
      ["Pick the app", "On the device: Settings → bfdecrypt → enable the app."],
      ["Launch it", "Open the app once; bfdecrypt writes the IPA into the app's Documents folder."],
      ["Copy it out", "Devices → Files → App data → the app → Documents → save the IPA, then import it in IPA Archive."],
    ],
  },
  crackerxi: {
    title: "CrackerXI+",
    intro: "iOS 11–16, jailbroken. On-device app that dumps installed apps to IPA.",
    steps: [
      ["Add the repo", "In Cydia/Sileo/Zebra add <span class=mono>https://cydia.iphonecake.com</span> and install <b>CrackerXI+</b>."],
      ["Dump", "Open CrackerXI+, tap the app → <b>YES, Full IPA</b>."],
      ["Copy it out", "Devices → Files → <span class=mono>/var/mobile/Documents/CrackerXI</span> → save the IPA, then import it in IPA Archive."],
    ],
  },
  fridadump: {
    title: "frida-ios-dump",
    intro: "iOS 8–16, jailbroken, over USB. Unmaintained and doesn't work with Frida 17+; prefer bagbak where possible.",
    steps: [
      ["Device", "Install <b>Frida 16</b> from <span class=mono>https://build.frida.re</span> (frida-server)."],
      ["This Mac", "<pre>pip3 install 'frida-tools<14' 'frida<17'\ngit clone https://github.com/AloneMonkey/frida-ios-dump</pre>"],
      ["Dump", "<pre>iproxy 2222 22 &amp;\ncd frida-ios-dump\npython3 dump.py &lt;bundle id&gt;</pre>Import the IPA it writes in IPA Archive."],
    ],
  },
  trolldecrypt: {
    title: "TrollDecrypt",
    intro: "No jailbreak, needs TrollStore (iOS 14.0–16.6.1 and 17.0).",
    steps: [
      ["Install TrollStore", "Follow the TrollStore guide for your iOS version (ios.cfw.guide/installing-trollstore)."],
      ["Install TrollDecrypt", "Get the TrollDecrypt .tipa (github.com/donato-fiore/TrollDecrypt) and install it with TrollStore."],
      ["Dump", "Open TrollDecrypt, tap the app; it saves the decrypted IPA in its Documents folder."],
      ["Copy it out", "Share it to this Mac (AirDrop) or take it from Devices → Files → App data → TrollDecrypt → Documents, then import it in IPA Archive."],
    ],
  },
});

const DUMP_NOTES_EN = {
  archive: "No jailbreak. Backs up the IPA as-is (FairPlay encrypted).",
  clutch: "SSH + OpenSSH required. You provide the Clutch file; iPacked installs it as /bin/clutch.",
  flexdecrypt: ".deb from github.com/JohnCoates/flexdecrypt; run by hand over SSH: flexdecrypt <binary>.",
  bfdecrypt: ".deb from github.com/BishopFox/bfdecrypt; pick the app in Settings, the IPA lands in its Documents (grab it in Files).",
  crackerxi: "On-device UI. Writes IPAs to /var/mobile/Documents/CrackerXI (grab them in Files).",
  bagbak: "frida-server on the device, bagbak on this Mac (npm i -g bagbak). Over USB.",
  trolldecrypt: "No jailbreak, needs TrollStore (iOS 14.0–16.6.1, 17.0). On-device UI.",
};
