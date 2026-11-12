const T = window.__TAURI__;
const LOC = "en-US";
const invoke = (cmd, args) => T.core.invoke(cmd, args);
const listen = (ev, cb) => T.event.listen(ev, (e) => cb(e.payload));
const fileSrc = (p) => T.core.convertFileSrc(p);
const $ = (s, el = document) => el.querySelector(s);
const $$ = (s, el = document) => [...el.querySelectorAll(s)];
const esc = (s) => String(s ?? "").replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c]));
const fmtDate = (d) => (d ? new Date(d).toLocaleDateString(LOC, { year: "numeric", month: "short", day: "numeric" }) : "—");
const fmtSize = (b) => (b > 1e9 ? (b / 1e9).toFixed(2) + " GB" : (b / 1e6).toFixed(1) + " MB");

const sf = (n) => `<i class="sf" style="--sf:url('sf/${n}.png')"></i>`;
function decorateSf(root = document) {
  $$("[data-sf]", root).forEach((el) => {
    if (!el.querySelector(":scope > .sf")) el.insertAdjacentHTML("afterbegin", sf(el.dataset.sf));
  });
}

function toast(msg, err = false) {
  const t = document.createElement("div");
  t.className = "toast" + (err ? " err" : "");
  t.textContent = msg;
  t.title = "You can select and copy · × to close";
  const x = document.createElement("button");
  x.className = "toast-x";
  x.textContent = "×";
  x.onclick = () => t.remove();
  t.append(x);
  $("#toast").append(t);
  // Keep it while hovered/selected so text can be copied.
  const hide = () => (t.matches(":hover") || String(getSelection()).length ? setTimeout(hide, 2000) : t.remove());
  setTimeout(hide, err ? 12000 : 5000);
}
const fail = (e) => toast(String(e).replace("FAIRPLAY_AUTH: ", "") + (/FAIRPLAY_AUTH/.test(String(e)) ? " Authorize the device for this Apple ID with iTunes, then try again." : ""), true);

// ---------- state ----------
const S = {
  store: null,      // ipatool account
  owned: [],        // purchased apps
  searchRes: [],
  src: "owned",
  selected: null,   // selected app {id, bundleID, name}
  archive: [],
  lookups: {},      // appId -> lookup
  devices: [],
  queue: Promise.resolve(),
};

// ---------- navigation ----------
function go(view) {
  $$(".nav").forEach((b) => b.classList.toggle("active", b.dataset.view === view));
  $$(".view").forEach((v) => v.classList.toggle("active", v.id === "view-" + view));
  if (view === "archive") renderArchive();
  if (view === "blobs") loadBlobs();
  if (view === "devices") {
    renderTools();
    if (!S.devices.length) loadDevices();
  }
}
$$(".nav").forEach((b) => (b.onclick = () => go(b.dataset.view)));
document.addEventListener("click", (e) => {
  const g = e.target.closest("[data-goto]");
  if (g) go(g.dataset.goto);
});

// ---------- icons ----------
// Lookups are batched (one request per ~150 apps); a failed batch is retried
// on the next call instead of staying empty for the session.
const LQ = { ids: new Map(), timer: 0 };
function flushLookups() {
  const batch = LQ.ids;
  LQ.ids = new Map();
  LQ.timer = 0;
  invoke("app_lookup_many", { appIds: [...batch.keys()] })
    .then((m) => batch.forEach((res, id) => (m[id] ? res(m[id]) : (delete S.lookups[id], res({})))))
    .catch(() => batch.forEach((res, id) => (delete S.lookups[id], res({}))));
}
async function lookup(appId) {
  if (!appId) return {};
  if (!S.lookups[appId]) {
    S.lookups[appId] = new Promise((res) => LQ.ids.set(Number(appId), res));
    if (!LQ.timer) LQ.timer = setTimeout(flushLookups, 60);
  }
  return S.lookups[appId];
}
/** Icon HTML: archived icon if we have one, otherwise current icon darkened with "?" */
function iconHtml({ iconPath, appId, size = "", platform = "" }) {
  // tvOS / watchOS / visionOS icons are already shaped (rendered by macOS).
  const shaped = ["tvos", "watchos", "visionos"].includes(platform) ? ` shaped plat-${platform}` : "";
  if (iconPath) return `<div class="icon ${size}${shaped}"><img src="${fileSrc(iconPath)}"></div>`;
  const id = "ic" + Math.random().toString(36).slice(2);
  lookup(appId).then((l) => {
    const el = document.getElementById(id);
    if (el && l?.artwork) el.innerHTML = `<img src="${esc(l.artwork)}">`;
  });
  return `<div class="icon unknown ${size}" id="${id}"></div>`;
}
function currentIconHtml(appId, size = "") {
  // Library list: plain current icon (no "?")
  const archived = S.archive.find((e) => e.appId === appId && e.iconPath);
  const id = "ci" + Math.random().toString(36).slice(2);
  lookup(appId).then((l) => {
    const el = document.getElementById(id);
    if (!el) return;
    if (l?.artwork) el.innerHTML = `<img src="${esc(l.artwork)}">`;
    else if (archived) el.innerHTML = `<img src="${fileSrc(archived.iconPath)}">`;
  });
  return `<div class="icon ${size}" id="${id}"></div>`;
}

// ---------- library ----------
async function loadStore() {
  S.store = await invoke("store_info").catch(() => null);
  $("#lib-login").classList.toggle("hidden", !!S.store);
  renderStoreState();
  $("#foot").textContent = S.store ? S.store.email : "App Store: not connected";
  if (S.store) loadOwned();
}

// ---------- paging: long lists render one page at a time ----------
const PAGE = 100;
S.pages = {};
/// Page slice of `list` for `key`; resets to page 1 when `sig` (filter/sort) changes.
function pageOf(key, list, sig = "") {
  const st = (S.pages[key] ||= { p: 0, sig });
  if (st.sig !== sig) (st.p = 0), (st.sig = sig);
  const pages = Math.max(1, Math.ceil(list.length / PAGE));
  st.p = Math.min(st.p, pages - 1);
  return { items: list.slice(st.p * PAGE, st.p * PAGE + PAGE), page: st.p, pages, total: list.length };
}
function pagerHtml(key, pg) {
  if (pg.pages <= 1) return "";
  return `<div class="pager" data-pager="${key}"><button class="btn small" data-pg="-1" ${pg.page ? "" : "disabled"}>${sf("chevron.left")}Prev</button>
    <span class="sub">Page ${pg.page + 1} of ${pg.pages} · ${pg.total} items</span>
    <button class="btn small" data-pg="1" ${pg.page < pg.pages - 1 ? "" : "disabled"}>Next${sf("chevron.right")}</button></div>`;
}
function bindPager(root, key, rerender) {
  if (!root) return;
  $$(`[data-pager="${key}"] [data-pg]`, root).forEach(
    (b) =>
      (b.onclick = () => {
        S.pages[key].p += +b.dataset.pg;
        rerender();
        root.scrollTop = 0;
      })
  );
}

const spinner = (t) => `<div class="empty"><span class="spin"></span> ${t}</div>`;

async function loadOwned() {
  // Show the last list at once; the full refresh (1000+ apps = many pages) runs behind it.
  const cached = await invoke("store_purchases_cached").catch(() => []);
  if (cached.length) await showOwned(cached);
  else $("#lib-list").innerHTML = spinner("Loading purchases…");
  const tag = $("#lib-sync");
  tag?.classList.remove("hidden");
  const un = await listen("purchases-progress", (e) => {
    const { loaded, total } = e;
    if (tag) tag.innerHTML = `<span class="spin"></span> ${loaded} / ${total}`;
    if (!cached.length && !S.owned?.length) $("#lib-list").innerHTML = spinner(`Loading purchases… ${loaded} / ${total}`);
  });
  try {
    await showOwned(await invoke("store_purchases"));
  } catch (e) {
    if (!cached.length) $("#lib-list").innerHTML = `<div class="empty">${esc(e)}</div>`;
    else toast("Could not refresh purchases: " + e, true);
  } finally {
    un();
    tag?.classList.add("hidden");
  }
}

async function showOwned(cloud) {
  const seen = new Set(cloud.map((a) => a.id));
  const extra = [];
  const add = (a, origin) => {
    if (!a.id || seen.has(a.id)) return;
    seen.add(a.id);
    extra.push({ ...a, origin });
  };
  ((await invoke("manual_apps").catch(() => [])) || []).forEach((a) => add(a, "Added manually"));
  (S.deviceApps || []).forEach((d) => d.appleId && add({ id: Number(d.appleId), bundleID: d.bundleId, name: d.name, version: d.version }, "From device"));
  S.owned = cloud.concat(extra);
  S.owned.sort((a, b) => (a.name || "").localeCompare(b.name || "", "en"));
  if (S.src === "owned") renderLibList();
}

function renderLibList() {
  const q = $("#lib-filter").value.toLowerCase();
  // iPhone & iPad view hides Mac / TV / Vision-only purchases ("All" shows everything).
  const iosOnly = (a) => S.view !== "" || S.src === "artist" || !a.platforms?.length || a.platforms.some((p) => p === "iphone" || p === "ipad" || p === "unknown");
  const items = (S.src === "owned" ? S.owned : S.src === "artist" ? S.artistRes : S.searchRes).filter(
    (a) => iosOnly(a) && (!q || S.src !== "owned" || (a.name + " " + a.bundleID).toLowerCase().includes(q))
  );
  const head =
    S.src === "artist"
      ? `<div class="list-head">${sf("person.crop.circle")} <b>${esc(S.artistName)}</b> · ${new Set(items.map((a) => a.id)).size} apps
      <button class="btn small" id="artist-back">${sf("chevron.left")}Back</button></div>`
      : "";
  if (!items.length) {
    const msg =
      S.src === "search"
        ? S.searched
          ? `No apps found for “${esc(S.searched)}”${S.country ? ` in the ${esc(countryName(S.country))} store` : ""}.`
          : "Type and press Enter to search."
        : S.src === "artist"
          ? "No apps found for this developer."
          : q
            ? `No apps match “${esc(q)}”.`
            : "No apps.";
    $("#lib-list").innerHTML = head + `<div class="empty">${msg}</div>`;
    const bk = $("#artist-back");
    if (bk) bk.onclick = () => ((S.src = S.prevSrc || "owned"), renderLibList());
    return;
  }
  const PLAT = [["ios", "iPhone & iPad"], ["macos", "Mac"], ["tvos", "Apple TV"], ["watchos", "Apple Watch"], ["visionos", "Apple Vision"]];
  const row = (a) => {
      const n = S.archive.filter((e) => e.appId === a.id).length;
      return `<div class="row ${S.selected?.id === a.id ? "active" : ""}" data-id="${a.id}">
        ${currentIconHtml(a.id)}
        <div class="meta"><div class="name">${esc(a.name)}</div>
        <div class="sub">${esc(a.bundleID)} · v${esc(a.version)}${a.price != null && S.src !== "owned" ? ` · ${a.price > 0 ? esc(a.price) : "Free"}` : ""}${n ? ` · <b>${n} archived</b>` : ""}</div>
        ${a.origin ? `<span class="pill warn" title="Not in Apple's purchase list (may be delisted)">${esc(a.origin)} · not in list</span>` : ""}</div></div>`;
  };
  $("#lib-list").innerHTML =
    head +
    (S.src === "artist"
      ? PLAT.map(([k, label]) => {
          const g = items.filter((a) => (a.platform || "ios") === k);
          return g.length ? `<div class="plat-head">${esc(label)} <span class="sub">${g.length}</span></div>` + g.map(row).join("") : "";
        }).join("")
      : (() => {
          const pg = pageOf("lib", items, S.src + "|" + q + "|" + S.view);
          return pg.items.map(row).join("") + pagerHtml("lib", pg);
        })());
  bindPager($("#lib-list"), "lib", renderLibList);
  $$("#lib-list .row").forEach(
    (r) =>
      (r.onclick = async () => {
        const a = items.find((x) => String(x.id) === r.dataset.id);
        // Developer page lists every platform: switch the store to the app's platform.
        const want = { ios: "", macos: "macos", tvos: "appletv", watchos: "watchos", visionos: "visionos" }[a.platform || ""];
        if (S.src === "artist" && want != null && want !== (S.platform || "")) {
          S.platform = want;
          $("#lib-platform").value = want;
          await invoke("store_set_platform", { platform: want });
        }
        selectApp(a);
      })
  );
  $$("#lib-list .row").forEach(
    (r) => (r.oncontextmenu = (ev) => libMenu(ev, items.find((x) => String(x.id) === r.dataset.id)))
  );
  const back = $("#artist-back");
  if (back) back.onclick = () => ((S.src = S.prevSrc || "owned"), renderLibList());
}

async function showArtist(artistId, artistName) {
  S.prevSrc = S.src === "artist" ? S.prevSrc : S.src;
  S.src = "artist";
  S.artistName = artistName;
  S.artistRes = [];
  $("#lib-list").innerHTML = `<div class="empty">Loading apps by ${esc(artistName)}…</div>`;
  try {
    S.artistRes = await invoke("artist_apps", { artistId, country: S.country || null });
  } catch (e) {
    fail(e);
  }
  renderLibList();
}

$("#lib-platform").onchange = async (e) => {
  S.view = e.target.value;
  S.platform = S.view === "all" ? "" : S.view;
  await invoke("store_set_platform", { platform: S.platform });
  S.selected = null;
  S.searchRes = [];
  $("#lib-detail").innerHTML = `<div class="empty">Select an app to see its versions.</div>`;
  if (S.src === "search") renderLibList();
  else {
    S.src = "owned";
    $$("[data-src]").forEach((x) => x.classList.toggle("active", x.dataset.src === "owned"));
    loadOwned();
  }
};

$$("[data-src]").forEach(
  (b) =>
    (b.onclick = () => {
      $$("[data-src]").forEach((x) => x.classList.toggle("active", x === b));
      S.src = b.dataset.src;
      $("#lib-filter").placeholder = S.src === "search" ? "Search App Store (Enter)…" : "Filter…";
      renderLibList();
    })
);
$("#lib-filter").oninput = () => S.src === "owned" && renderLibList();
$("#lib-filter").onkeydown = async (e) => {
  if (e.key !== "Enter" || S.src !== "search") return;
  $("#lib-list").innerHTML = `<div class="empty">Searching…</div>`;
  S.searched = e.target.value.trim();
  if (S.view === "all" && S.platform) {
    S.platform = "";
    await invoke("store_set_platform", { platform: "" });
  }
  S.searchRes = await invoke("store_search", { term: e.target.value, country: null, platforms: S.view === "all" ? ["", "macos"] : null }).catch((er) => (fail(er), []));
  renderLibList();
};
$("#lib-refresh").onclick = () => (S.src === "owned" ? loadOwned() : null);
$("#lib-add").onclick = async () => {
  const r = await ask(
    `<h3>${sf("plus.circle")} Add app</h3><p class="sub">For apps missing from Apple's purchase list (removed from the store), enter the App ID or App Store link.</p><input name="q" placeholder="e.g. 473262001, id473262001 or https://apps.apple.com/app/id473262001">`,
    "Add",
    () => {
      const inp = $("[name=q]");
      inp.addEventListener("input", () => {
        const v = cleanAppInput(inp.value);
        if (v !== inp.value) inp.value = v;
      });
    }
  );
  if (!r?.q) return;
  try {
    const a = await invoke("manual_app_add", { input: r.q });
    toast(`${a.name} added`);
    await loadOwned();
    selectApp(S.owned.find((x) => x.id === a.id));
  } catch (e) {
    fail(e);
  }
};

async function selectApp(app, refresh = false, load = false) {
  S.selected = app;
  S.vboard = null;
  $("#vers-board") && ($("#vers-board").innerHTML = "");
  // "All" store: use the app's own platform for versions and downloads.
  if (S.view === "all") {
    const p = app.platforms || [];
    const auto = p.includes("iphone") || p.includes("ipad") || !p.length ? "" : p.includes("macos") ? "macos" : p.includes("appletv") ? "appletv" : p.includes("visionos") ? "visionos" : p.includes("watchos") ? "watchos" : "";
    const plat = S.forcePlat?.id === app.id ? S.forcePlat.plat : auto;
    if (plat !== (S.platform || "")) {
      S.platform = plat;
      await invoke("store_set_platform", { platform: plat });
    }
    if (S.selected !== app) return;
  }
  $$("#lib-list .row").forEach((r) => r.classList.toggle("active", r.dataset.id === String(app.id)));
  const d = $("#lib-detail");
  d.innerHTML = `<div class="app-head">${currentIconHtml(app.id, "lg")}
    <div><h2>${esc(app.name)}</h2><div class="sub" id="app-artist"></div><div class="sub mono"><span class="copyable" data-copy="${esc(app.bundleID)}">${esc(app.bundleID)}</span> · ID <span class="copyable" data-copy="${app.id}">${app.id}</span></div></div>
    <div class="actions">
      <button class="btn" id="v-store">${sf("app.badge")}App Store</button>
      <button class="btn" id="v-buy">${sf("cart")}Get license</button></div></div>
    <div id="app-plats" class="app-plats"></div>
    <div id="app-info"></div>
    <div class="sec-row"><h3 class="sec">${sf("clock")} Version history</h3><span class="grow"></span><span id="vers-tools"></span></div>
    <div id="vers-board"></div>
    <div id="vers"></div>`;
  bindCopy(d);
  $("#v-buy").onclick = () => buyApp(app);
  lookup(app.id).then((l) => {
    if (S.selected !== app) return;
    $("#v-store").onclick = () => (l.trackViewUrl ? invoke("open_url", { url: l.trackViewUrl }) : fail("App Store page not found (the app may have been removed)"));
    $("#v-buy").innerHTML = `${sf("cart")}${l.found && l.price > 0 ? `Buy · ${esc(l.formattedPrice)}` : "Get (free license)"}`;
    if (l.found && l.artistId) {
      $("#app-artist").innerHTML = `<a href="#" id="artist-link">${esc(l.artistName)}</a>`;
      $("#artist-link").onclick = (e) => (e.preventDefault(), showArtist(l.artistId, l.artistName));
    }
    renderAppInfo(l);
    renderAppPlatforms(app, l);
  });

  // Version history is opt-in (Settings → auto): old apps have hundreds of versions.
  if (!load && !refresh && !S.prefs?.autoVersionHistory) {
    $("#vers").innerHTML = `<div class="empty"><p>Version history not loaded yet.</p>
      <button class="btn primary" id="v-load">${sf("clock")}Load version history</button>
      <p class="sub">Old apps can have hundreds of versions; each name and date is fetched one by one. You can turn on automatic loading in Settings.</p></div>`;
    $("#v-load").onclick = () => selectApp(app, false, true);
    return;
  }
  $("#vers-tools").innerHTML = `<button class="btn small" id="v-refresh">${sf("arrow.clockwise")}Refresh versions</button>`;
  $("#v-refresh").onclick = () => selectApp(app, true, true);
  $("#vers").innerHTML = `<div class="empty"><span class="spin"></span> Fetching version list…</div>`;

  let ids;
  try {
    ids = await invoke("app_versions", { appId: app.id, refresh });
  } catch (e) {
    if (S.selected !== app) return;
    const lic = /license/i.test(String(e));
    $("#vers").innerHTML = `<div class="empty">${lic ? "Your account has no license for this app. Get a license first to see and download its version history." : esc(e)}
      ${lic ? `<p><button class="btn primary" id="lic-get">${sf("cart")}Get license</button></p>` : ""}</div>`;
    if (lic) $("#lic-get").onclick = () => buyApp(app);
    return;
  }
  if (S.selected !== app) return;
  ids = [...ids].reverse(); // newest first
  $("#vers").innerHTML = `<table><thead><tr><th></th><th>Version</th><th>Release date</th><th>External ID</th><th>Status</th><th></th></tr></thead>
    <tbody>${ids.map((v) => `<tr data-v="${esc(v)}"></tr>`).join("")}</tbody></table>`;
  ids.forEach((v) => renderVersionRow(app, v, null));

  // Progress board: names/dates arrive one by one.
  S.vboard = { app, total: ids.length, done: ids.filter((v) => metaCache[app.id + ":" + v]).length, t0: Date.now() };
  paintBoard();
  invoke("versions_metadata", { appId: app.id, versionIds: ids })
    .catch((e) => S.selected === app && toast("Could not read version names: " + e, true))
    .finally(() => S.vboard && S.vboard.app === app && ((S.vboard.done = S.vboard.total), paintBoard()));
}

function paintBoard() {
  const b = S.vboard, el = $("#vers-board");
  if (!b || !el || S.selected !== b.app) return;
  if (b.done >= b.total) return (el.innerHTML = "");
  const pct = (b.done / b.total) * 100;
  const secs = (Date.now() - b.t0) / 1000;
  const rate = b.done / Math.max(1, secs);
  const eta = rate > 0 && secs > 3 ? fmtEta((b.total - b.done) / rate) : "";
  el.innerHTML = `<div class="board"><span class="spin"></span><div class="grow">
    <div><b>Fetching version details</b> <span class="sub">${b.done} / ${b.total}${eta ? " · ~" + eta + " left" : ""}</span></div>
    <div class="bar"><div style="width:${pct}%"></div></div>
    <div class="sub">The list is ready; names and dates fill in as they arrive. You can start downloading meanwhile.</div></div></div>`;
}

/** Click-to-copy for any element with data-copy. */
function bindCopy(root) {
  $$("[data-copy]", root).forEach((c) => {
    c.title = c.title || "Click to copy";
    c.onclick = async (ev) => {
      ev.stopPropagation();
      await navigator.clipboard.writeText(c.dataset.copy);
      toast("Copied: " + c.dataset.copy);
    };
  });
}

async function buyApp(app) {
  const l = await lookup(app.id);
  if (l.found && l.price > 0) {
    // ipatool can only obtain free licenses; paid apps go through the App Store.
    toast(`Paid app (${l.formattedPrice}): opening the App Store to buy it`);
    return invoke("open_url", { url: l.trackViewUrl });
  }
  const ok = await ask(`<h3>${sf("cart")} Get license</h3><p>Add the free license for <b>${esc(app.name)}</b> to your account? (Same as "Get" in the App Store.)</p>`, "Get license");
  if (!ok) return;
  try {
    await invoke("store_purchase", { appId: app.id });
    toast(`${app.name} license obtained`);
    selectApp(app, true);
  } catch (e) {
    fail("Couldn't get license: " + e);
  }
}

function renderAppInfo(l) {
  const el = $("#app-info");
  if (!el) return;
  if (!l.found) {
    el.innerHTML = `<div class="notice">${sf("exclamationmark.triangle.fill")}<div>This app is no longer listed on the App Store; no store info.</div></div>`;
    return;
  }
  const shots = (l.screenshotUrls?.length ? l.screenshotUrls : l.ipadScreenshotUrls || []).slice(0, 8);
  const stars = l.averageUserRating ? `${l.averageUserRating.toFixed(1)} ★ (${(l.userRatingCount || 0).toLocaleString(LOC)})` : "—";
  el.innerHTML = `
    <div class="info-grid">
      <div><span>Category</span><b>${esc((l.genres || []).join(", ") || "—")}</b></div>
      <div><span>Size</span><b>${l.fileSizeBytes ? fmtSize(Number(l.fileSizeBytes)) : "—"}</b></div>
      <div><span>Age</span><b>${esc(l.contentAdvisoryRating ?? "—")}</b></div>
      <div><span>Rating</span><b>${stars}</b></div>
      <div><span>Compatibility</span><b>${l.minimumOsVersion ? (S.platform === "macos" ? "macOS " : "iOS ") + esc(l.minimumOsVersion) + "+" : "—"}</b></div>
      <div><span>Price</span><b>${esc(l.formattedPrice ?? "—")}</b></div>
      <div><span>First released</span><b>${fmtDate(l.releaseDate)}</b></div>
      <div><span>Languages</span><b title="${esc((l.languageCodesISO2A || []).join(", "))}">${(l.languageCodesISO2A || []).length || "—"}</b></div>
      <div><span>Store</span><b>${esc(countryName(l.country || "us"))}</b></div>
      <div><span>Developer site</span><b>${l.sellerUrl ? `<a href="#" data-open="${esc(l.sellerUrl)}">Open</a>` : "—"}</b></div>
    </div>
    ${devicesHtml(l)}
    ${l.country && l.country !== "us" && S.store?.country && l.country !== S.store.country ? `<div class="notice">${sf("exclamationmark.triangle.fill")}<div>This app was found only in the <b>${esc(countryName(l.country))}</b> App Store. Licenses are given only to Apple IDs from that country.</div></div>` : ""}
    <details class="box" open><summary>${sf("sparkles")} What's New · v${esc(l.currentVersion)} · ${fmtDate(l.currentVersionReleaseDate)}</summary>
      <p class="pre">${esc(l.releaseNotes || "No release notes.")}</p></details>
    <details class="box"><summary>${sf("text.alignleft")} Description</summary><p class="pre">${esc(l.description || "")}</p></details>
    ${shots.length ? `<div class="shots">${shots.map((u) => `<img src="${esc(u)}" loading="lazy">`).join("")}</div>` : ""}`;
}

const archivedFor = (appId, v) => S.archive.find((e) => e.appId === appId && e.externalVersionId === v);
const metaCache = {};
const dlState = {};

function renderVersionRow(app, v, meta) {
  if (meta) metaCache[app.id + ":" + v] = meta;
  if (S.selected !== app) return;
  meta = meta || metaCache[app.id + ":" + v];
  const tr = $(`#vers tr[data-v="${CSS.escape(v)}"]`);
  if (!tr) return;
  const arc = archivedFor(app.id, v);
  const dAny = S.dls.find((x) => x.key === dlKey(app, v) && !["done", "cancelled"].includes(x.state));
  // A failed download doesn't block the buttons: it can simply be tried again.
  const d = dAny && dAny.state !== "error" ? dAny : null;
  const status = arc
    ? `<span class="pill ok">${sf("checkmark.seal.fill")}Archived</span>`
    : d
      ? `<span class="pill warn">${sf("hourglass")}${esc(dlLabel(d))}</span>`
      : dAny
        ? `<span class="pill bad" title="${esc(dAny.error || "")}">${sf("exclamationmark.triangle.fill")}Failed</span>`
        : `<span class="sub">—</span>`;
  tr.innerHTML = `<td>${iconHtml({ iconPath: arc?.iconPath, appId: app.id })}</td>
    <td><b title="${esc(meta?.error ?? "")}">${esc(meta?.displayVersion ?? arc?.version ?? "…")}</b></td>
    <td>${fmtDate(arc ? relDate(arc) : meta?.releaseDate)}</td>
    <td class="mono copyable" title="Click to copy">${esc(v)}</td><td>${status}</td>
    <td class="menu">${
      arc
        ? `<button type="button" class="btn small icon-only" data-a="show" title="Show in archive">${sf("eye")}</button>`
        : `<button type="button" class="btn small icon-only" data-a="dl" title="Download" ${d ? "disabled" : ""}>${sf("arrow.down.circle")}</button>
           <button type="button" class="btn small dldump" data-a="dldump" title="Download, then decrypt it on the connected device; both the original and the dump are archived" ${d ? "disabled" : ""}>${sf("key.fill")}Download + Decrypt</button>`
    }</td>`;
  $(".copyable", tr).onclick = async () => (await navigator.clipboard.writeText(v), toast(`Version ID copied: ${v}`));
  $$("button", tr).forEach(
    (b) =>
      (b.onclick = () =>
        arc ? (go("archive"), ($("#arc-filter").value = arc.sha256), renderArchive()) : enqueueDownload(app, v, b.dataset.a === "dldump"))
  );
}

// ---------- downloads (iTunes-style list: progress, time left, pause/cancel) ----------
S.dls = [];
function dlLabel(d) {
  if (d.state === "queued") return "Queued";
  if (d.state === "paused") return "Paused";
  if (d.state === "analyzing") return "Analyzing…";
  if (d.state === "error") return "Error";
  if (d.state === "done") return "Done";
  if (d.total && d.bytes >= d.total) return "Decrypting…";
  if (d.total) return `%${Math.floor((d.bytes / d.total) * 100)}`;
  return d.bytes ? fmtSize(d.bytes) : "Starting…";
}
const fmtEta = (s) => (!isFinite(s) || s <= 0 ? "" : s < 60 ? `${Math.ceil(s)} s` : s < 3600 ? `${Math.ceil(s / 60)} min` : `${(s / 3600).toFixed(1)} h`);

const dlKey = (app, v, plat = S.platform || "") => `${app.id}:${v}:${plat}`;
function enqueueDownload(app, v, dump = false) {
  const plat = S.platform || "";
  const key = dlKey(app, v, plat);
  const ex = S.dls.find((x) => x.key === key);
  if (ex && ["queued", "running", "analyzing"].includes(ex.state)) return;
  if (ex) S.dls.splice(S.dls.indexOf(ex), 1);
  const meta = metaCache[app.id + ":" + v];
  S.dls.push({ key, plat, app, v, meta, dump, name: app.name, version: meta?.displayVersion ?? v, state: "queued", bytes: 0, total: 0, speed: 0 });
  dlRefresh(app, v);
  pumpDownloads();
}

function pumpDownloads() {
  const running = S.dls.filter((x) => x.state === "running" || x.state === "analyzing").length;
  const next = S.dls.find((x) => x.state === "queued");
  if (running >= 2 || !next) return;
  runDownload(next);
  pumpDownloads();
}

async function runDownload(d) {
  d.state = "running";
  d.started = Date.now();
  dlRefresh(d.app, d.v);
  const run = (purchase) =>
    invoke("download_version", {
      appId: d.app.id,
      versionId: d.v,
      displayVersion: d.meta?.displayVersion ?? null,
      releaseDate: d.meta?.releaseDate ?? null,
      purchase,
      bundleId: d.app.bundleId ?? null,
      name: d.app.name ?? null,
      platform: d.plat ?? null,
    });
  try {
    let entry;
    try {
      entry = await run(false);
    } catch (e) {
      if (!/license/i.test(String(e))) throw e;
      entry = await run(true); // free app not yet "purchased" on this account
    }
    d.state = "done";
    S.archive = await invoke("archive_list");
    toast(`${entry.name} ${entry.version} archived`);
    if (d.dump) {
      if (entry.encrypted === true) enqueueDumps([entry], null);
      else toast(`${entry.name}: the IPA is already decrypted, no dump needed`);
    }
  } catch (e) {
    const m = String(e);
    if (m === "PAUSED") d.state = "paused";
    else if (m === "CANCELLED") d.state = "cancelled";
    else (d.state = "error"), (d.error = m), fail(`${d.name} ${d.version}: ${m}`);
  }
  dlRefresh(d.app, d.v);
  pumpDownloads();
}

function dlRefresh(app, v) {
  if (S.selected && app && S.selected.id === app.id) renderVersionRow(S.selected, v);
  const active = S.dls.filter((x) => ["queued", "running", "analyzing", "paused"].includes(x.state)).length;
  const badge = $("#dl-badge");
  if (badge) (badge.textContent = active || ""), badge.classList.toggle("hidden", !active);
  const dn = (S.dumps || []).filter((x) => ["queued", "running"].includes(x.state)).length;
  $$(".dump-badge").forEach((b) => ((b.textContent = dn || ""), b.classList.toggle("hidden", !dn)));
  renderDownloads();
  renderDumps();
}

function renderDownloads() {
  const el = $("#dl-panel");
  if (!el || el.classList.contains("hidden")) return;
  const rows = S.dls.filter((x) => x.state !== "cancelled");
  el.innerHTML = `<div class="dl-head"><b>${sf("arrow.down.circle")} Downloads</b>
      <button class="btn small" id="dl-clear">Clear finished</button><button class="btn small icon-only" id="dl-close">${sf("xmark.circle.fill")}</button></div>
    ${
      rows.length
        ? rows
            .map((d, i) => {
              const pct = d.total ? (d.bytes / d.total) * 100 : 0;
              const eta = d.state === "running" && d.speed ? fmtEta((d.total - d.bytes) / d.speed) : "";
              return `<div class="dl-row" data-i="${S.dls.indexOf(d)}">
                ${iconHtml({ appId: d.app.id })}
                <div class="grow"><div class="name">${esc(d.name)} <span class="sub">${esc(d.version)}</span></div>
                  <div class="bar"><div style="width:${d.state === "done" ? 100 : pct}%"></div></div>
                  <div class="sub">${
                    d.state === "running" && d.total && d.bytes < d.total
                      ? `${fmtSize(d.bytes)} / ${fmtSize(d.total)} · ${fmtSize(d.speed)}/s${eta ? " · " + eta + " left" : ""}`
                      : d.state === "error"
                        ? `<span class="sel">${esc(d.error)}</span>`
                        : esc(dlLabel(d))
                  }</div></div>
                <div class="acts">
                  ${d.state === "running" ? `<button class="btn small icon-only" data-d="pause" title="Pause">${sf("pause.circle")}</button>` : ""}
                  ${["paused", "error"].includes(d.state) ? `<button class="btn small icon-only" data-d="resume" title="Resume">${sf("arrow.clockwise")}</button>` : ""}
                  ${["queued", "running", "paused"].includes(d.state) ? `<button class="btn small icon-only danger" data-d="cancel" title="Cancel">${sf("xmark.circle.fill")}</button>` : ""}
                </div></div>`;
            })
            .join("")
        : `<div class="empty">No downloads.</div>`
    }`;
  $("#dl-close").onclick = () => el.classList.add("hidden");
  $("#dl-clear").onclick = () => ((S.dls = S.dls.filter((x) => !["done", "error", "cancelled"].includes(x.state))), dlRefresh());
  $$("[data-d]", el).forEach(
    (b) =>
      (b.onclick = async () => {
        const d = S.dls[+b.closest(".dl-row").dataset.i];
        const act = b.dataset.d;
        if (act === "pause") await invoke("download_stop", { versionId: d.v, keep: true });
        else if (act === "resume") (d.state = "queued"), pumpDownloads();
        else if (act === "cancel") {
          if (d.state === "running") await invoke("download_stop", { versionId: d.v, keep: false });
          else d.state = "cancelled";
        }
        dlRefresh(d.app, d.v);
      })
  );
}
function renderDumps() {
  const el = $("#dump-panel");
  if (!el || el.classList.contains("hidden")) return;
  const dumps = S.dumps || [];
  el.innerHTML = `<div class="dl-head"><b>${sf("key.fill")} Dump queue</b>
      <button class="btn small" id="dp-clear">Clear finished</button><button class="btn small icon-only" id="dp-close">${sf("xmark.circle.fill")}</button></div>
    ${dumps.length ? dumps.map((j, i) => `<div class="dl-row" data-j="${i}">
        ${iconHtml({ platform: j.e.platform, iconPath: j.e.iconPath, appId: j.e.appId })}
        <div class="grow"><div class="name">${esc(j.e.name)} <span class="sub">${esc(j.e.version)}${j.dev ? " → " + esc(j.dev.name) : ""}</span></div>
          <div class="bar"><div class="${j.state === "running" ? "indet" : ""}" style="width:${j.state === "done" ? 100 : j.state === "running" ? 100 : 0}%"></div></div>
          <div class="sub">${j.state === "error" ? `<span class="sel">${esc(j.error)}</span>` : esc(j.step)}</div></div>
        <div class="acts">${j.state === "queued" ? `<button class="btn small icon-only danger" data-j="cancel" title="Remove from queue">${sf("xmark.circle.fill")}</button>` : ""}
          ${j.state === "error" && j.dev ? `<button class="btn small icon-only" data-j="retry" title="Retry">${sf("arrow.clockwise")}</button>` : ""}</div></div>`).join("") : `<div class="empty">Dump queue is empty. Add items with "Select to dump" in the Archive.</div>`}`;
  $$("[data-j]", el).forEach((b) => {
    if (!b.matches("button")) return;
    b.onclick = () => {
      const j = S.dumps[+b.closest(".dl-row").dataset.j];
      if (b.dataset.j === "cancel") S.dumps.splice(S.dumps.indexOf(j), 1);
      else (j.state = "queued"), (j.step = "Queued"), pumpDumps();
      renderDumps();
    };
  });

  $("#dp-close").onclick = () => el.classList.add("hidden");
  $("#dp-clear").onclick = () => ((S.dumps = S.dumps.filter((x) => !["done", "error"].includes(x.state))), dlRefresh());
}
const toggleDumps = () => ($("#dump-panel").classList.toggle("hidden"), renderDumps());
$("#arc-dumps").onclick = toggleDumps;
$("#dev-dumps").onclick = toggleDumps;
$("#lib-downloads").onclick = () => ($("#dl-panel").classList.toggle("hidden"), renderDownloads());

listen("version-meta", (m) => {
  if (!S.selected || m.appId !== S.selected.id) return;
  if (m.error) {
    metaCache[m.appId + ":" + m.versionId] = { displayVersion: "?", error: m.error };
  }
  renderVersionRow(S.selected, m.versionId, m.error ? null : m);
  if (S.vboard && S.vboard.app === S.selected) S.vboard.done++, paintBoard();
});

listen("download-progress", (p) => {
  const d = S.dls.find((x) => x.v === p.versionId && ["running", "analyzing"].includes(x.state));
  if (!d) return;
  if (p.stage === "analyzing") d.state = "analyzing";
  if (p.bytes != null) {
    const now = Date.now();
    if (d.lastT && now > d.lastT) {
      const inst = ((p.bytes - d.lastB) * 1000) / (now - d.lastT);
      d.speed = d.speed ? d.speed * 0.8 + inst * 0.2 : inst;
    }
    d.lastT = now;
    d.lastB = p.bytes;
    d.bytes = p.bytes;
    if (p.total > 1) d.total = p.total;
  }
  dlRefresh(d.app, d.v);
});

// ---------- archive ----------
S.view = "all";
const PLAT_LABEL = { tvos: ["Apple TV", "appletv"], watchos: ["Apple Watch", "clock"], visionos: ["Apple Vision", "eye"], imessage: ["iMessage app", "text.alignleft"], stickers: ["iMessage stickers", "star.fill"] };
/// What device the package is for (iPhone/iPad apps show nothing unless they carry a Watch app).
const platPill = (e) => {
  const p = PLAT_LABEL[e.platform];
  const own = p ? `<span class="pill plat">${sf(p[1])}${p[0]}</span>` : "";
  const watch = e.hasWatch ? `<span class="pill plat" title="The iPhone app carries an Apple Watch app">${sf("clock")}+ Watch app</span>` : "";
  return own + watch;
};
/// "iOS 9.0+", "tvOS 9.0+", … for the package's platform.
const osLabel = (e) => (isMac(e) ? "macOS" : `${{ tvos: "tvOS", watchos: "watchOS", visionos: "visionOS" }[e.platform] || "iOS"} ${esc(e.minIos ?? "?")}+`);
const encPill = (e) => platPill(e) + encPillOnly(e);
const encPillOnly = (e) =>
  e.encrypted === true
    ? `<span class="pill warn tip" data-tip="${esc(e.appleId ? "Apple ID: " + e.appleId : "Apple ID unknown")}">${sf("lock.fill")}Encrypted</span>`
    : e.encrypted === false
      ? `<span class="pill ok">${sf("checkmark.shield.fill")}Decrypted</span>`
      : "";
/// Release date of this *version*. Store metadata often carries the app's first
/// release date; the binary's build date is closer when it is clearly later.
const relDate = (e) => {
  const r = e.releaseDate, b = e.buildDate;
  if (!b) return r;
  if (!r) return b;
  return Date.parse(b) - Date.parse(r) > 3 * 864e5 ? b : r;
};
const fileName = (e) => String(e.filePath || "").split("/").pop();
const isMac = (e) => /\.pkg$/i.test(e.filePath || "") || (e.deviceFamily || []).includes("Mac");
const verCmp = (a, b) =>
  (relDate(b) || "").localeCompare(relDate(a) || "") ||
  String(b.version).localeCompare(String(a.version), undefined, { numeric: true });
S.arcOpen = new Set();

function versionKv(e) {
  return `<dl class="kv">
    <dt>Release date</dt><dd>${fmtDate(relDate(e))}${e.buildDate && relDate(e) === e.buildDate ? ` <span class="sub">(build date)</span>` : ""}</dd>
    <dt>File</dt><dd class="mono">${esc(fileName(e))}</dd>
    <dt>Kind</dt><dd>${isMac(e) ? "Mac app (.pkg)" : e.dumpedFrom ? "Dump (decrypted on device)" : e.encrypted === true ? "Original (FairPlay)" : e.encrypted === false ? "Decrypted" : "—"}</dd>
    <dt>Minimum ${isMac(e) ? "macOS" : { tvos: "tvOS", watchos: "watchOS", visionos: "visionOS" }[e.platform] || "iOS"}</dt><dd>${esc(e.minIos ?? "—")}</dd>
    <dt>Platform</dt><dd>${isMac(e) ? "Mac" : PLAT_LABEL[e.platform]?.[0] || "iPhone / iPad"}${e.hasWatch ? " + Apple Watch app" : ""}</dd>
    ${isMac(e) && e.appName ? `<dt>App name</dt><dd>${esc(e.appName)}</dd>` : ""}
    <dt>Architectures</dt><dd>${esc(e.architectures.join(", ") || "—")}</dd>
    <dt>${isMac(e) ? "Platform" : "Device"}</dt><dd>${esc(e.deviceFamily.join(", ") || "—")}</dd>
    <dt>Bundle ID</dt><dd class="mono copyable" data-copy="${esc(e.bundleId)}">${esc(e.bundleId)}</dd>
    <dt>External version ID</dt><dd class="mono ${e.externalVersionId ? "copyable" : ""}" data-copy="${esc(e.externalVersionId ?? "")}">${esc(e.externalVersionId ?? "—")}</dd>
    <dt>MD5</dt><dd class="mono copyable" data-copy="${e.md5}">${e.md5}</dd>
    <dt>SHA-1</dt><dd class="mono copyable" data-copy="${e.sha1}">${e.sha1}</dd>
    <dt>SHA-256</dt><dd class="mono copyable" data-copy="${e.sha256}">${e.sha256}</dd>
    <dt>Apple ID</dt><dd class="sel">${esc(e.appleId ?? "—")}</dd>
    <dt>Source</dt><dd>${esc(e.source)}</dd>
    <dt>Archived at</dt><dd>${new Date(e.archivedAt).toLocaleString(LOC)}</dd>
  </dl>`;
}

function versionActions(e) {
  return `<div class="actions">
    <button type="button" class="btn small" data-a="reveal">${sf("folder")}Show in Finder</button>
    ${isMac(e) ? "" : `<button type="button" class="btn small" data-a="install">${sf("iphone")}Install on device</button>`}
    <button type="button" class="btn small" data-a="copy">${sf("doc.on.doc")}Copy info</button>
    <button type="button" class="btn small" data-a="copypath">${sf("doc.on.doc")}Copy path</button>
    <button type="button" class="btn small" data-a="store">${sf("app.badge")}App Store page</button>
    <button type="button" class="btn small" data-a="reicon" title="Extract the icon again from the IPA">${sf("photo.badge.arrow.down")}Refresh icon</button>
    ${e.encrypted === true ? `<button type="button" class="btn small" data-a="devdec">${sf("key.fill")}Decrypt on device</button>` : ""}
    <button type="button" class="btn small danger" data-a="del">${sf("trash")}Delete</button></div>`;
}

function bindActions(root, e) {
  $$("[data-a]", root).forEach(
    (b) =>
      (b.onclick = async (ev) => {
        ev.stopPropagation();
        const a = b.dataset.a;
        if (a === "reveal") invoke("reveal", { path: e.filePath });
        if (a === "install") installDialog(e.filePath);
        if (a === "store") {
          const l = await invoke("find_store_page", { appId: e.appId ?? null, bundleId: e.bundleId }).catch(() => ({}));
          if (l.found && l.trackViewUrl) invoke("open_url", { url: l.trackViewUrl });
          else fail(`App Store page for ${e.name} not found (it may have been removed)`);
        }
        if (a === "macdec") macDecryptFor(e);
        if (a === "reicon") {
          try {
            const n = await invoke("archive_refresh_icon", { id: e.id });
            Object.assign(e, n);
            toast("Icon refreshed from the IPA");
            if (!$("#modal").classList.contains("hidden") && $("#modal-body .vtop")) showVersion(e);
            renderArchive();
          } catch (err) {
            fail("Icon refresh failed: " + err);
          }
        }
        if (a === "devdec") deviceDecryptDialog(e);
        if (a === "copypath") (await navigator.clipboard.writeText(e.filePath), toast("File path copied"));
        if (a === "copy") {
          const txt = `${e.name} ${e.version} (${e.build})\nBundle ID: ${e.bundleId}\nReleased: ${e.releaseDate ?? "-"}\nMin iOS: ${e.minIos}\nArchitectures: ${e.architectures.join(", ")}\nMD5: ${e.md5}\nSHA-1: ${e.sha1}\nSHA-256: ${e.sha256}\nSource: ${e.source}\nArchived: ${e.archivedAt}`;
          await navigator.clipboard.writeText(txt);
          toast("Copied");
        }
        if (a === "del") {
          $("#modal").open && $("#modal").close();
          const r = await ask(`<h3>Delete: ${esc(e.name)} ${esc(e.version)}</h3><p>Remove this entry from the archive?</p>
            <label><input type="checkbox" name="file" style="width:auto"> Also permanently delete the IPA file</label>`, "Delete");
          if (!r) return;
          await invoke("archive_remove", { id: e.id, deleteFile: !!r.file }).catch(fail);
          renderArchive();
        }
      })
  );
}

/// Right-click menu for an archived file (same actions as the details sheet).
function arcMenu(ev, e) {
  ev.preventDefault();
  ev.stopPropagation();
  const act = (a) => () => {
    const b = document.createElement("button");
    b.dataset.a = a;
    const wrap = document.createElement("div");
    wrap.append(b);
    bindActions(wrap, e);
    b.click();
  };
  fsMenu(ev.clientX, ev.clientY, [
    ["Details", "info.circle", () => showVersion(e)],
    ["Show in Finder", "folder", act("reveal")],
    !isMac(e) && ["Install on device", "iphone", act("install")],
    e.encrypted === true && !isMac(e) && ["Decrypt on device", "key.fill", act("devdec")],
    "-",
    ["Copy info", "doc.on.doc", act("copy")],
    ["Copy path", "doc.on.doc", act("copypath")],
    ["Copy bundle ID", "doc.on.doc", () => navigator.clipboard.writeText(e.bundleId).then(() => toast("Copied: " + e.bundleId))],
    ["App Store page", "app.badge", act("store")],
    ["Show in Library", "person.text.rectangle", () => showInLibrary(e)],
    ["Refresh icon from IPA", "photo.badge.arrow.down", act("reicon")],
    "-",
    ["Delete…", "trash", act("del"), true],
  ]);
}

function showVersion(e) {
  const m = $("#modal");
  $("#modal-body").innerHTML = `<div class="top vtop">${iconHtml({ platform: e.platform, iconPath: e.iconPath, appId: e.appId, size: "lg" })}
      <div><h3>${esc(e.name)} ${esc(e.version)}</h3><div class="sub">build ${esc(e.build)} · ${fmtSize(e.fileSize)} ${encPill(e)}</div></div></div>
    ${versionKv(e)}${versionActions(e)}
    <div class="row-btns"><button class="btn" value="cancel">Close</button></div>`;
  bindActions($("#modal-body"), e);
  bindCopy($("#modal-body"));
  m.showModal();
}

/// A decrypted dump of this original that is already in the archive.
const dumpOf = (o) =>
  o.encrypted === true &&
  S.archive.find((d) => d !== o && d.bundleId === o.bundleId && (d.dumpedFrom === o.sha256 || (d.encrypted === false && d.version === o.version && d.build === o.build)));

/// Versions of one app, originals first with their dumps right below them.
function pairVersions(items) {
  const dumps = items.filter((e) => e.dumpedFrom || e.encrypted === false);
  const origs = items.filter((e) => !dumps.includes(e));
  const out = [];
  const used = new Set();
  for (const o of origs.sort(verCmp)) {
    out.push({ e: o, child: false });
    for (const d of dumps)
      if (!used.has(d) && (d.dumpedFrom === o.sha256 || (!d.dumpedFrom && d.version === o.version && d.build === o.build)))
        used.add(d), out.push({ e: d, child: true });
  }
  for (const d of dumps.sort(verCmp)) if (!used.has(d)) out.push({ e: d, child: false });
  return out;
}

const ARC_SORTS = {
  az: (a, b) => a.name.localeCompare(b.name, "en"),
  za: (a, b) => b.name.localeCompare(a.name, "en"),
  bid: (a, b) => a.bundleId.localeCompare(b.bundleId),
  bidza: (a, b) => b.bundleId.localeCompare(a.bundleId),
  date: (a, b) => (relDate(b.latest) || "").localeCompare(relDate(a.latest) || ""),
  dateold: (a, b) => (relDate(a.latest) || "").localeCompare(relDate(b.latest) || ""),
  added: (a, b) => b.added.localeCompare(a.added),
  size: (a, b) => b.size - a.size,
  count: (a, b) => b.items.length - a.items.length || a.name.localeCompare(b.name, "en"),
};

async function renderArchive() {
  S.archive = await invoke("archive_list");
  const q = $("#arc-filter").value.toLowerCase().trim();
  const list = S.archive.filter(
    (e) => !q || [e.name, e.bundleId, e.version, e.build, e.externalVersionId, fileName(e), e.md5, e.sha1, e.sha256].join(" ").toLowerCase().includes(q)
  );
  const el = $("#arc-list");
  renderSelBar();
  if (S.sel) {
    // Like a search: only FairPlay IPAs this device can run and is authorized for.
    for (let k = list.length - 1; k >= 0; k--) if (selBlocker(list[k])) list.splice(k, 1);
  }
  if (!list.length) {
    el.className = "cards";
    el.innerHTML = `<div class="empty">${S.sel ? "No encrypted IPA this device can dump (FairPlay, iOS version and Apple ID must match)." : S.archive.length ? "No matching entries." : "Archive is empty. Download versions from the Library or import an IPA."}</div>`;
    return;
  }
  // Group versions of the same app.
  const groups = Object.values(
    list.reduce((m, e) => ((m[e.bundleId] ||= { bundleId: e.bundleId, items: [] }).items.push(e), m), {})
  ).map((g) => {
    g.items.sort(verCmp);
    g.latest = g.items[0];
    g.name = g.latest.name;
    g.size = g.items.reduce((s, e) => s + e.fileSize, 0);
    g.added = g.items.reduce((m, e) => (e.archivedAt > m ? e.archivedAt : m), "");
    g.mac = g.items.every(isMac);
    return g;
  });
  groups.sort(ARC_SORTS[$("#arc-sort").value] || ARC_SORTS.az);
  if (q) groups.forEach((g) => S.arcOpen.add(g.bundleId)); // searching expands matches

  // iOS apps first, Mac App Store packages in their own section below.
  const ios = groups.filter((g) => !g.mac), mac = groups.filter((g) => g.mac);
  const view = S.sel ? "list" : S.arcView || "grid";
  el.className = "arc-sections";
  el.innerHTML = `<div id="arc-ios"></div>${mac.length ? `<h3 class="sec arc-mac-h">${sf("desktopcomputer")} macOS apps <span class="sub">${mac.length}</span></h3><div id="arc-mac"></div>` : ""}`;
  const draw = (box, gs) => {
    if (!box) return;
    if (!gs.length) return (box.innerHTML = `<div class="empty">No iOS apps.</div>`);
    if (view === "list") renderArchiveList(box, gs);
    else if (view === "icons") renderArchiveIcons(box, gs);
    else renderArchiveGrid(box, gs);
  };
  const pg = pageOf("arc", ios, q + "|" + $("#arc-sort").value + "|" + view + "|" + (S.sel ? 1 : 0));
  draw($("#arc-ios"), pg.items);
  $("#arc-ios").insertAdjacentHTML("beforeend", pagerHtml("arc", pg));
  bindPager(el, "arc", renderArchive);
  draw($("#arc-mac"), mac);
}

function renderArchiveGrid(el, groups) {
  el.className = "cards grid";
  el.innerHTML = groups
    .map((g) => {
      const e = g.latest;
      const open = S.arcOpen.has(g.bundleId);
      return `<div class="card group" data-b="${esc(g.bundleId)}">
        <div class="top">${iconHtml({ platform: e.platform, iconPath: e.iconPath, appId: e.appId, size: "lg" })}
          <div class="grow"><div class="ttl">${esc(g.name)}</div>
          <div class="sub mono sel">${esc(g.bundleId)}</div>
          <div class="sub">Latest: v${esc(e.version)} · ${fmtDate(relDate(e))} · ${g.items.length} files · ${fmtSize(g.size)}</div></div></div>
        <details class="vers" ${open ? "open" : ""}><summary>${sf("archivebox")} Archived versions (${g.items.length})</summary>
          ${pairVersions(g.items)
            .map(
              ({ e: v, child }) => `<div class="vrow ${child ? "dump" : ""}" data-id="${v.id}">
                ${child ? `<span class="tree">↳</span>` : iconHtml({ platform: v.platform, iconPath: v.iconPath, appId: v.appId })}
                <div class="grow"><b>v${esc(v.version)}</b> <span class="sub">(${esc(v.build)})</span>
                  <div class="sub">${child ? "Dump · " : ""}${fmtDate(relDate(v))} · ${isMac(v) ? "macOS" : `iOS ${esc(v.minIos ?? "?")}+`} · ${esc(v.architectures.join(", "))} · ${fmtSize(v.fileSize)}</div></div>
                ${encPill(v)}
                ${isMac(v) ? "" : `<button type="button" class="btn small icon-only" data-a="install" title="Install on device">${sf("iphone")}</button>`}
                <button type="button" class="btn small icon-only" data-a="reveal" title="Show in Finder">${sf("folder")}</button>
              </div>`
            )
            .join("")}
        </details></div>`;
    })
    .join("");
  $$(".card.group", el).forEach((c) => {
    const g = groups.find((x) => x.bundleId === c.dataset.b);
    $("details", c).ontoggle = (ev) => (ev.target.open ? S.arcOpen.add(g.bundleId) : S.arcOpen.delete(g.bundleId));
    $$(".vrow", c).forEach((r) => {
      const v = g.items.find((x) => x.id === r.dataset.id);
      bindActions(r, v);
      r.onclick = () => showVersion(v);
      r.oncontextmenu = (ev) => arcMenu(ev, v);
    });
  });
}

/** Finder-style list: app "folders" with disclosure triangles, versions inside. */
function renderArchiveList(el, groups) {
  el.className = "finder";
  const sel = S.sel;
  const rows = [];
  for (const g of groups) {
    const open = S.arcOpen.has(g.bundleId) || !!sel;
    const e = g.latest;
    const pairs = pairVersions(g.items);
    let grpCls = "";
    if (sel) {
      const ok = g.items.filter((v) => !selBlocker(v));
      grpCls = ok.length ? "" : "blocked";
    }
    rows.push(`<tr class="grp ${open ? "open" : ""} ${grpCls}" data-b="${esc(g.bundleId)}">
      <td class="namecol">${sel ? `<input type="checkbox" class="pick-g" ${grpCls ? "disabled" : ""}>` : ""}<span class="disc">${sf("chevron.right")}</span>${iconHtml({ platform: e.platform, iconPath: e.iconPath, appId: e.appId, size: "sm" })}
        <span class="nm">${esc(g.name)}</span><span class="cnt">${g.items.length}</span></td>
      <td>v${esc(e.version)}</td><td>${fmtDate(relDate(e))}</td><td class="mono dim copyable" data-copy="${esc(g.bundleId)}">${esc(g.bundleId)}</td><td></td><td>${fmtSize(g.size)}</td><td></td></tr>`);
    if (open)
      for (const { e: v, child } of pairs) {
        const why = sel ? selBlocker(v) : null;
        rows.push(`<tr class="ver ${child ? "dump" : ""} ${why ? "blocked" : ""}" data-id="${v.id}" ${why ? `title="${esc(why)}"` : ""}>
          <td class="namecol"><span class="indent"></span>${child ? `<span class="indent"></span><span class="tree">↳</span>` : ""}${
            sel ? `<input type="checkbox" class="pick" ${why ? "disabled" : ""} ${S.sel.picked.has(v.id) ? "checked" : ""}>` : ""
          }${iconHtml({ platform: v.platform, iconPath: v.iconPath, appId: v.appId, size: "sm" })}
            <span class="nm">${esc(fileName(v))}</span></td>
          <td>v${esc(v.version)} <span class="dim">(${esc(v.build)})</span></td><td>${fmtDate(relDate(v))}</td>
          <td>${why ? `<span class="dim">${esc(why)}</span>` : `${osLabel(v)} · ${esc(v.architectures.join(", "))}`}${sel && dumpOf(v) ? ` <span class="pill warn">${sf("exclamationmark.triangle.fill")}Already dumped</span>` : ""}</td><td>${encPill(v)}</td><td>${fmtSize(v.fileSize)}</td>
          <td class="acts">${isMac(v) ? "" : `<button type="button" class="btn small icon-only" data-a="install" title="Install on device">${sf("iphone")}</button>`}
            <button type="button" class="btn small icon-only" data-a="reveal" title="Show in Finder">${sf("folder")}</button></td></tr>`);
      }
  }
  const sortKey = $("#arc-sort").value;
  const th = (label, asc, desc) =>
    `<th class="sortable ${sortKey === asc || sortKey === desc ? "on" : ""}" data-s="${asc}" data-s2="${desc}">${label}${sortKey === asc ? " ▲" : sortKey === desc ? " ▼" : ""}</th>`;
  el.innerHTML = `<table class="ftable"><thead><tr>${th("Name", "az", "za")}<th>Version</th>${th("Release date", "date", "dateold")}${th("Bundle ID / Info", "bid", "bidza")}<th>Status</th>${th("Size", "size", "size")}<th></th></tr></thead><tbody>${rows.join("")}</tbody></table>`;
  bindCopy(el);
  $$("th.sortable", el).forEach(
    (h) =>
      (h.onclick = () => {
        const cur = $("#arc-sort").value;
        $("#arc-sort").value = cur === h.dataset.s && h.dataset.s2 !== h.dataset.s ? h.dataset.s2 : h.dataset.s;
        $("#arc-sort").onchange();
      })
  );
  $$("tr.grp", el).forEach((r) => {
    const b = r.dataset.b;
    const g = groups.find((x) => x.bundleId === b);
    const box = $(".pick-g", r);
    if (box) {
      const ok = g.items.filter((v) => !selBlocker(v));
      box.checked = ok.length > 0 && ok.every((v) => S.sel.picked.has(v.id));
      box.onclick = (ev) => {
        ev.stopPropagation();
        ok.forEach((v) => (box.checked ? S.sel.picked.add(v.id) : S.sel.picked.delete(v.id)));
        renderArchive();
      };
    }
    r.onclick = () => {
      S.arcKbd = { b };
      if (sel) return;
      S.arcOpen.has(b) ? S.arcOpen.delete(b) : S.arcOpen.add(b);
      renderArchive();
    };
    r.oncontextmenu = (ev) => arcMenu(ev, g.latest);
  });
  $$("tr.ver", el).forEach((r) => {
    const v = S.archive.find((x) => x.id === r.dataset.id);
    bindActions(r, v);
    const box = $(".pick", r);
    if (box)
      box.onclick = (ev) => {
        ev.stopPropagation();
        box.checked ? S.sel.picked.add(v.id) : S.sel.picked.delete(v.id);
        renderSelBar();
        renderArchive();
      };
    r.onclick = () => {
      if (sel && box && !box.disabled) return box.click();
      $$("tr.sel, tr.kbd", el).forEach((x) => x.classList.remove("sel", "kbd"));
      r.classList.add("sel", "kbd");
      S.arcKbd = { id: v.id };
    };
    r.ondblclick = () => showVersion(v);
    r.oncontextmenu = (ev) => arcMenu(ev, v);
  });
  // Restore the keyboard cursor after a re-render.
  const k = S.arcKbd && $(S.arcKbd.id ? `tr.ver[data-id="${CSS.escape(S.arcKbd.id)}"]` : `tr.grp[data-b="${CSS.escape(S.arcKbd.b || "")}"]`, el);
  if (k) k.classList.add("kbd");
}

// Finder-like keys in the Archive list: ↑/↓ move, → open, ← close/parent, Enter/Space details.
document.addEventListener("keydown", (ev) => {
  if (!$("#view-archive").classList.contains("active") || $("#modal").open) return;
  if (ev.target.closest?.("input, select, textarea")) return;
  if (!["ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight", "Enter", " "].includes(ev.key)) return;
  if ((S.sel ? "list" : S.arcView) !== "list") return;
  const rows = $$("#arc-list tr.grp, #arc-list tr.ver");
  if (!rows.length) return;
  ev.preventDefault();
  let i = rows.findIndex((r) => r.classList.contains("kbd"));
  const cur = rows[i];
  const setK = (r) => {
    rows.forEach((x) => x.classList.remove("kbd"));
    if (!r) return;
    r.classList.add("kbd");
    r.scrollIntoView({ block: "nearest" });
    S.arcKbd = r.classList.contains("ver") ? { id: r.dataset.id } : { b: r.dataset.b };
  };
  if (ev.key === "ArrowDown") return setK(rows[Math.min(rows.length - 1, i + 1)] || rows[0]);
  if (ev.key === "ArrowUp") return setK(rows[Math.max(0, i - 1)] || rows[0]);
  if (!cur) return setK(rows[0]);
  const grpOf = (r) => { let p = r; while (p && !p.classList.contains("grp")) p = p.previousElementSibling; return p; };
  if (ev.key === "ArrowRight" && cur.classList.contains("grp") && !S.arcOpen.has(cur.dataset.b)) return S.arcOpen.add(cur.dataset.b), renderArchive();
  if (ev.key === "ArrowRight" && cur.classList.contains("grp")) return setK(rows[i + 1]);
  if (ev.key === "ArrowLeft" && cur.classList.contains("ver")) return setK(grpOf(cur));
  if (ev.key === "ArrowLeft" && cur.classList.contains("grp") && S.arcOpen.has(cur.dataset.b)) return S.arcOpen.delete(cur.dataset.b), renderArchive();
  if (ev.key === "Enter" || ev.key === " ") {
    if (cur.classList.contains("ver")) return cur.ondblclick();
    S.arcOpen.has(cur.dataset.b) ? S.arcOpen.delete(cur.dataset.b) : S.arcOpen.add(cur.dataset.b);
    renderArchive();
  }
});

function renderArchiveIcons(el, groups) {
  el.className = "cards icons";
  el.innerHTML = groups
    .map(
      (g) => `<div class="card" data-b="${esc(g.bundleId)}" title="${esc(g.bundleId)}">
        <div class="badgewrap">${iconHtml({ platform: g.latest.platform, iconPath: g.latest.iconPath, appId: g.latest.appId, size: "lg" })}<span class="badge">${g.items.length}</span></div>
        <div class="ttl">${esc(g.name)}</div><div class="sub">v${esc(g.latest.version)}</div></div>`
    )
    .join("");
  $$(".card", el).forEach(
    (c) =>
      (c.onclick = () => {
        S.arcOpen.add(c.dataset.b);
        $$("[data-arcview]").forEach((x) => x.classList.toggle("active", x.dataset.arcview === "list"));
        S.arcView = "list";
        renderArchive();
      })
  );
}
$("#arc-filter").oninput = renderArchive;
try {
  $("#arc-sort").value = localStorage.getItem("arcSort") || "az";
} catch {}
$("#arc-sort").onchange = () => {
  try {
    localStorage.setItem("arcSort", $("#arc-sort").value);
  } catch {}
  renderArchive();
};
try {
  S.arcView = localStorage.getItem("arcView") || "grid";
} catch {
  S.arcView = "grid";
}
$$("[data-arcview]").forEach((b) => {
  b.classList.toggle("active", b.dataset.arcview === S.arcView);
  b.onclick = () => {
    S.arcView = b.dataset.arcview;
    try {
      localStorage.setItem("arcView", S.arcView);
    } catch {}
    $$("[data-arcview]").forEach((x) => x.classList.toggle("active", x === b));
    renderArchive();
  };
});
$("#arc-open").onclick = async () => invoke("open_path", { path: await invoke("archive_root") });
$("#arc-import").onclick = async () => {
  const files = await T.dialog.open({ multiple: true, filters: [{ name: "IPA", extensions: ["ipa"] }] });
  for (const f of [files].flat().filter(Boolean)) {
    toast("Importing: " + f.split("/").pop());
    await invoke("archive_import", { path: f }).then((e) => toast(`${e.name} ${e.version} added`)).catch(fail);
  }
  renderArchive();
};

// ---------- Mac FairPlay decrypt ----------
async function macDecryptDialog() {
  const apps = await invoke("mac_ios_apps");
  if (!apps.length) {
    return ask(`<h3>${sf("key.fill")} FairPlay decryption on this Mac</h3>
      <p>No iPhone/iPad apps from the Mac App Store are installed on this Mac.</p>
      <p class="sub">Install the app from Mac App Store → your account → "iPhone & iPad Apps" with the same Apple ID, then decrypt it here. SIP stays on; only apps with arm64 code (those that run on Apple silicon) can be decrypted.</p>`, "OK");
  }
  const r = await ask(`<h3>${sf("key.fill")} FairPlay decryption on this Mac</h3>
    <label>Installed iOS app<select name="path">${apps.map((a) => `<option value="${esc(a.path)}">${esc(a.name)} ${esc(a.version)} (${esc(a.bundleId)})</option>`).join("")}</select></label>
    <p class="sub">Encrypted binaries are decrypted with this Mac's FairPlay and the decrypted IPA is added to the archive.</p>`, "Decrypt");
  if (r) runMacDecrypt(r.path);
}

async function macDecryptFor(e) {
  const apps = await invoke("mac_ios_apps");
  const inst = apps.find((a) => a.bundleId === e.bundleId);
  if (inst) {
    if (inst.version !== e.version) toast(`Note: version ${inst.version} is installed on this Mac, the archive has ${e.version}. The installed version will be decrypted.`);
    return runMacDecrypt(inst.path);
  }
  const ok = await ask(`<h3>${sf("key.fill")} Decrypt on this Mac: ${esc(e.name)}</h3><p>This app isn't installed on this Mac. Install it from the Mac App Store (same Apple ID) first.</p>`, "Open Mac App Store");
  if (ok && e.appId) invoke("open_url", { url: `macappstore://apps.apple.com/app/id${e.appId}` });
}

async function runMacDecrypt(path) {
  go("devices");
  const log = $("#dev-log");
  log.textContent = "Decrypting FairPlay on this Mac…\n";
  log.classList.remove("hidden");
  try {
    const e = await invoke("mac_decrypt", { path });
    toast(`${e.name} ${e.version} added to archive decrypted`);
    S.archive = await invoke("archive_list");
  } catch (er) {
    fail("Decryption failed: " + er);
  }
}
// ---------- decrypt on a connected device (install → dump) ----------
const verNum = (v) => String(v || "0").split(".").map((n) => parseInt(n) || 0);
const verLt = (a, b) => {
  const x = verNum(a), y = verNum(b);
  for (let i = 0; i < 3; i++) if ((x[i] || 0) !== (y[i] || 0)) return (x[i] || 0) < (y[i] || 0);
  return false;
};
const iosMajor = (d) => verNum(d.iosVersion)[0];

/// Best dump method for an iOS version (see DUMP_METHODS in guide.js).
function dumpMethodFor(d) {
  const m = iosMajor(d);
  return m <= 11 ? "clutch" : "bagbak";
}

/// Why this entry can't be decrypted on this device, or null.
function decryptBlocker(e, d) {
  if (/\.pkg$/i.test(e.filePath) || (e.deviceFamily || []).includes("Mac")) return "macOS app, can't be decrypted on an iOS device";
  const fam = e.deviceFamily || [];
  if (fam.length && !fam.includes("iPhone") && !fam.includes("iPad"))
    return `${fam.includes("TV") ? "tvOS" : fam.includes("Watch") ? "watchOS" : fam.join("/")} app, not possible`;
  if (fam.length && !fam.includes("iPhone") && d.productType.startsWith("iPhone")) return "iPad-only app";
  if (e.encrypted === false) return "This IPA is already decrypted";
  if (e.minIos && verLt(d.iosVersion, e.minIos)) return `iOS version too low (iOS ${d.iosVersion}, required ${e.minIos})`;
  const archs = e.architectures || [];
  const has64 = archs.some((a) => a.startsWith("arm64"));
  const has32 = archs.some((a) => a.startsWith("armv") || a === "arm");
  const dev64 = d.cpu.startsWith("arm64");
  if (archs.length && dev64 && iosMajor(d) >= 11 && !has64) return "32-bit app won't work (iOS 11+ can't run 32-bit)";
  if (archs.length && !dev64 && !has32) return "64-bit app, 32-bit device";
  if (iosMajor(d) < 5) return "iOS too old (no dump tool)";
  return null;
}

// Connected devices with details and the Apple IDs they hold FairPlay keys for
// (approximated by the buyers of App Store apps installed on them).
async function deviceInfos(force = false) {
  if (!S.devices?.length || force) await loadDevices();
  S.devInfo ||= {};
  const out = [];
  for (const x of S.devices || []) {
    let d = S.devInfo[x.id];
    if (!d || force) {
      const det = await invoke("device_details", { id: x.id }).catch(() => null);
      if (!det) continue;
      const apps = await invoke("device_apps", { id: x.id }).catch(() => []);
      d = { ...det, id: x.id, apps, accounts: new Set(apps.map((a) => (a.appleId || "").toLowerCase()).filter(Boolean)) };
      S.devInfo[x.id] = d;
    }
    out.push(d);
  }
  return out;
}

/// decryptBlocker + Apple ID check: the device must already be authorized for the buyer.
function fullBlocker(e, d) {
  const why = decryptBlocker(e, d);
  if (why) return why;
  const id = (e.appleId || "").toLowerCase();
  if (id && d.accounts?.size && !d.accounts.has(id)) return `Apple ID ${e.appleId} isn't authorized on this device`;
  if (d.apps?.some((a) => a.bundleId === e.bundleId) && !S.qInstalled?.has(d.id + ":" + e.bundleId)) return "Already installed on the device (won't install over it)";
  return null;
}

/// Why `e` can't be picked in "select to dump" mode (null = selectable).
function selBlocker(e) {
  const d = S.sel && S.devInfo?.[S.sel.dev];
  if (!d) return "Choose a device";
  return fullBlocker(e, d);
}

// ---------- "select to dump" mode ----------
async function startSelect() {
  const devs = await deviceInfos(!Object.keys(S.devInfo || {}).length);
  if (!devs.length) return fail("No connected device");
  S.sel = { dev: devs[0].id, picked: new Set(), keep: false };
  S.arcView = "list";
  try { localStorage.setItem("arcView", "list"); } catch {}
  $$("[data-arcview]").forEach((x) => x.classList.toggle("active", x.dataset.arcview === "list"));
  renderArchive();
}

function renderSelBar() {
  const bar = $("#arc-selbar");
  if (!bar) return;
  if (!S.sel) return bar.classList.add("hidden");
  bar.classList.remove("hidden");
  const devs = Object.values(S.devInfo || {});
  const picked = [...S.sel.picked].map((id) => S.archive.find((x) => x.id === id)).filter((e) => e && !selBlocker(e));
  bar.innerHTML = `${sf("key.fill")}<b>Select to dump</b>
    <select id="sel-dev" class="select">${devs.map((d) => `<option value="${esc(d.id)}" ${d.id === S.sel.dev ? "selected" : ""}>${esc(d.name)} · iOS ${esc(d.iosVersion)} · ${esc(d.cpu)}</option>`).join("")}</select>
    <span class="sub">Only encrypted IPAs this device can dump are listed.</span><span class="grow"></span>
    <label class="sw"><input type="checkbox" class="switch" id="sel-rm" ${S.prefs?.removeAfterDump !== false ? "checked" : ""}><span>Remove from device after dump</span></label>
    <button class="btn" id="sel-cancel">Cancel</button>
    <button class="btn primary" id="sel-go" ${picked.length ? "" : "disabled"}>${sf("key.fill")}Add to queue (${picked.length})</button>`;
  $("#sel-dev").onchange = (ev) => ((S.sel.dev = ev.target.value), renderArchive());
  $("#sel-rm").onchange = (ev) => savePrefs({ removeAfterDump: ev.target.checked });
  $("#sel-cancel").onclick = () => ((S.sel = null), renderArchive());
  $("#sel-go").onclick = () => {
    const { dev } = S.sel;
    S.sel = null;
    enqueueDumps(picked, dev);
    renderArchive();
  };
}

// ---------- dump queue (shown with downloads) ----------
S.dumps = [];
async function enqueueDumps(entries, devId, keep = S.prefs?.removeAfterDump === false) {
  const devs = await deviceInfos();
  for (const e of entries) {
    if (S.dumps.some((j) => j.e.id === e.id && ["queued", "running"].includes(j.state))) continue;
    const d = devs.find((x) => x.id === devId) || devs.find((x) => !fullBlocker(e, x));
    const job = { kind: "dump", e, dev: d, keep, state: "queued", step: "Queued" };
    if (!d) (job.state = "error"), (job.error = devs.length ? "No suitable device: " + fullBlocker(e, devs[0]) : "No connected device");
    S.dumps.push(job);
  }
  if ($("#view-archive").classList.contains("active") || $("#view-devices").classList.contains("active")) $("#dump-panel").classList.remove("hidden");
  else toast("Added to the dump queue (IPA Archive → Dumps)");
  dlRefresh();
  pumpDumps();
}

function pumpDumps() {
  if (S.dumps.some((j) => j.state === "running")) return;
  const next = S.dumps.find((j) => j.state === "queued");
  if (next) runDump(next);
}

async function runDump(j) {
  const { e, dev: d } = j;
  const set = (step) => ((j.step = step), dlRefresh());
  j.state = "running";
  const method = dumpMethodFor(d);
  let installed = false;
  try {
    const why = fullBlocker(e, d);
    if (why) throw why;
    S.qInstalled ||= new Set();
    if (S.qInstalled.has(d.id + ":" + e.bundleId)) {
      // Another version of this app was left on the device by an earlier job in this queue.
      set("Removing the previously dumped version from the device…");
      await invoke("device_uninstall", { id: d.id, bundleId: e.bundleId }).catch(() => {});
      S.qInstalled.delete(d.id + ":" + e.bundleId);
    }
    set(`1/3 Installing on ${d.name}…`);
    await invoke("device_install", { id: d.id, ipa: e.filePath, resign: false });
    installed = true;
    S.qInstalled.add(d.id + ":" + e.bundleId);
    invoke("devlog_note", { line: `dump queue: installed ${e.bundleId} ${e.version} on ${d.name}` }).catch(() => {});
    let out;
    for (let tries = 0; ; tries++) {
      set(`2/3 ${method} dumping with`);
      try {
        out = await invoke("device_export", { id: d.id, bundleId: e.bundleId, method, origId: e.id });
        break;
      } catch (x) {
        if (!/FAIRPLAY_AUTH/.test(String(x)) || tries >= 3) throw x;
        set("Device not authorized for this Apple ID, waiting…");
        const again = await ask(
          `<h3>${sf("key.fill")} Authorization needed on the device</h3>
           <p>${esc(d.name)} isn't authorized yet for the Apple ID that bought this IPA${e.appleId ? ` (<b>${esc(e.appleId)}</b>)` : ""}, so it couldn't be decrypted.</p>
           <p>Old iOS versions can no longer get this authorization from Apple themselves. Give it from a computer:</p>
           <ol><li>In iTunes (Windows/Mac, e.g. 12.5.8) authorize this Apple ID via <b>Account → Authorizations → Authorize This Computer</b>.</li>
           <li>Connect the phone and install or sync any app bought with this Apple ID through iTunes.</li>
           <li>Then press the button below. This is needed only once per Apple ID.</li></ol>`,
          "Retry"
        );
        if (!again) throw "Cancelled (device not authorized)";
      }
    }
    j.state = "done";
    j.step = `Archived: ${fileName(out)}`;
    toast(`${out.name} ${out.version} added to archive decrypted`);
  } catch (er) {
    j.state = "error";
    j.error = String(er);
    if (/SSH/.test(j.error)) sshMissing(er);
  }
  if (installed && !j.keep) {
    set("3/3 Removing from device…");
    await invoke("device_uninstall", { id: d.id, bundleId: e.bundleId }).catch(() => {});
    S.qInstalled.delete(d.id + ":" + e.bundleId);
    if (j.state === "running") j.state = "done";
  }
  if (j.state === "done" && /^3\/3/.test(j.step)) j.step = "Done";
  if (S.devInfo?.[d?.id]) delete S.devInfo[d.id]; // installed apps changed
  dlRefresh();
  if ($("#view-archive").classList.contains("active")) renderArchive().catch(() => {});
  pumpDumps();
}

async function deviceDecryptDialog(entry) {
  if (entry) return enqueueDumps([entry], null);
  startSelect();
}
$("#arc-macdec").onclick = () => (S.sel ? ((S.sel = null), renderArchive()) : startSelect());


// ---------- SHSH blobs ----------
const hexEcid = (d) => "0x" + BigInt(d).toString(16).toUpperCase();
S.blobSel = null;

async function loadBlobs() {
  S.blobs = await invoke("blobs_state");
  renderBlobDevices();
  if (S.blobSel) renderBlobDetail(S.blobSel);
}

function blobsFor(ecid) {
  return S.blobs.files.filter((f) => f.ecid === ecid);
}

function renderBlobDevices() {
  const { settings, files } = S.blobs;
  const known = new Set(settings.devices.map((d) => d.ecid));
  const unknown = [...new Set(files.filter((f) => f.ecid && !known.has(f.ecid)).map((f) => f.ecid))];
  const orphan = files.filter((f) => !f.ecid).length;
  $("#blob-devs").innerHTML =
    settings.devices
      .sort((a, b) => a.name.localeCompare(b.name, "en"))
      .map((d) => {
        const n = new Set(blobsFor(d.ecid).map((f) => f.build || f.version)).size;
        return `<div class="row ${S.blobSel === d.ecid ? "active" : ""}" data-e="${d.ecid}" data-q="${esc((d.name + " " + d.productType + " " + d.boardConfig + " " + d.ecid).toLowerCase())}">
          ${devIconHtml(d, "sm")}
          <div class="meta"><div class="name">${esc(d.name)}</div><div class="sub">${esc(d.productType)} · ${esc(d.boardConfig)} · ${n} versions</div></div></div>`;
      })
      .join("") +
    (unknown.length
      ? `<div class="list-head">${sf("exclamationmark.triangle.fill")} Unknown ECIDs</div>` +
        unknown
          .map((e) => {
            const f = files.find((x) => x.ecid === e);
            return `<div class="row" data-unknown="${e}"><div class="icon sm devic">${sf("questionmark")}</div>
              <div class="meta"><div class="name">${esc(f.productType || "Unknown device")}</div><div class="sub mono">${hexEcid(e)} · ${blobsFor(e).length} blob</div></div></div>`;
          })
          .join("")
      : "") +
    (orphan ? `<div class="sub" style="padding:8px 14px">${orphan} blob files have no ECID.</div>` : "") ||
    `<div class="empty">No devices yet. Add the connected device or add one manually.</div>`;
  $$("#blob-devs [data-e]").forEach((r) => (r.onclick = () => renderBlobDetail(r.dataset.e)));
  filterBlobDevs();
  $$("#blob-devs [data-unknown]").forEach(
    (r) =>
      (r.onclick = () => {
        const f = files.find((x) => x.ecid === r.dataset.unknown);
        blobDeviceDialog({ ecid: f.ecid, productType: f.productType || "", boardConfig: f.board || "", name: f.productType || "" });
      })
  );
}

const DEV_ICONS = ["iphone", "ipad", "appletv", "homepod", "ipad.and.iphone", "lock.shield", "star.fill", "heart.fill"];
const DEV_COLORS = ["#0a84ff", "#30d158", "#ff9f0a", "#ff453a", "#bf5af2", "#ff375f", "#64d2ff", "#8e8e93"];
function devIconHtml(d, size) {
  const c = d.color;
  return `<div class="icon ${size} devic" ${c ? `style="background:${c}22;color:${c}"` : ""}>${sf(d.icon || devIcon(d.productType))}</div>`;
}
function devIcon(pt = "") {
  return pt.startsWith("iPad") ? "ipad" : pt.startsWith("AppleTV") ? "appletv" : pt.startsWith("AudioAccessory") ? "homepod" : "iphone";
}

async function renderBlobDetail(ecid) {
  S.blobSel = ecid;
  $$("#blob-devs .row").forEach((r) => r.classList.toggle("active", r.dataset.e === ecid));
  const d = S.blobs.settings.devices.find((x) => x.ecid === ecid);
  if (!d) return;
  const mine = blobsFor(ecid);
  const el = $("#blob-detail");
  el.innerHTML = `<div class="app-head">${devIconHtml(d, "lg")}
      <div class="grow"><h2>${esc(d.name)}</h2>
        <div class="sub">${esc(d.productType)} · ${esc(d.boardConfig)}${d.note ? " · " + esc(d.note) : ""}</div>
        <div class="sub mono sel">ECID ${esc(d.ecid)} · ${hexEcid(d.ecid)}${d.generator ? " · generator " + esc(d.generator) : ""}</div></div>
      <div class="actions">
        <button class="btn" data-b="edit">${sf("pencil")}Edit</button>
        <button class="btn" data-b="folder">${sf("folder")}Folder</button>
        <button class="btn primary" data-b="saveall">${sf("square.and.arrow.down")}Save all signed</button></div></div>
    <div id="fw"><div class="empty">Fetching firmware list (ipsw.me)…</div></div>`;
  $("[data-b=edit]", el).onclick = () => blobDeviceDialog(d, true);
  $("[data-b=folder]", el).onclick = async () => invoke("reveal", { path: await invoke("blobs_folder", { ecid }) });

  let fw = { firmwares: [] };
  try {
    fw = await invoke("blobs_firmwares", { productType: d.productType });
  } catch (e) {
    toast("Couldn't fetch the ipsw.me firmware list: " + e, true);
  }
  if (S.blobSel !== ecid) return;
  const byBuild = {};
  mine.forEach((f) => (byBuild[f.build || f.version] ||= []).push(f));
  const listed = new Set(fw.firmwares.map((f) => f.buildid));
  const extra = mine.filter((f) => f.build && !listed.has(f.build));
  const extraBuilds = [...new Set(extra.map((f) => f.build))];
  const info = extraBuilds.length ? await invoke("blobs_build_info", { productType: d.productType, builds: extraBuilds }).catch(() => []) : [];
  if (S.blobSel !== ecid) return;
  fw.firmwares = fw.firmwares.concat(info);
  info.forEach((f) => listed.add(f.buildid));
  extra.splice(0, extra.length, ...extra.filter((f) => !listed.has(f.build)));
  const rows = fw.firmwares
    .map((f) => ({ version: f.version, build: f.buildid, signed: f.signed, date: f.releasedate, ota: f.ota, beta: f.beta, url: f.url, files: byBuild[f.buildid] || [] }))
    .concat([...new Set(extra.map((f) => f.build))].map((b) => ({ version: byBuild[b][0].version, build: b, signed: null, date: null, files: byBuild[b] })));
  const saved = rows.filter((r) => r.files.length).length;
  const signed = rows.filter((r) => r.signed);
  $("#fw").innerHTML = `<div class="info-grid">
      <div><span>Versions with blobs</span><b>${saved} / ${rows.length}</b></div>
      <div><span>Total blob files</span><b>${mine.length}</b></div>
      <div><span>Currently signed</span><b>${signed.map((s) => esc(s.version)).join(", ") || "—"}</b></div>
      <div><span>Model</span><b>${esc(fw.name || d.productType)}</b></div></div>
    <div class="chk-row"><label class="chk"><input type="checkbox" id="fw-only" ${S.fwOnlySaved ? "checked" : ""}> Only show versions with blobs</label>
    <label class="chk"><input type="checkbox" id="fw-signed" ${S.fwOnlySigned ? "checked" : ""}> Signed only</label></div>
    <table><thead><tr><th>Version</th><th>Build</th><th>Released</th><th>Signing</th><th>Blob</th><th></th></tr></thead><tbody>
    ${rows
      .filter((r) => (!S.fwOnlySaved || r.files.length) && (!S.fwOnlySigned || r.signed))
      .map(
        (r) => `<tr data-build="${esc(r.build)}">
        <td><b>${esc(r.version)}</b>${r.beta ? ` <span class="pill warn">Beta</span>` : r.ota ? ` <span class="pill">OTA</span>` : ""}</td><td class="mono">${esc(r.build)}</td><td>${fmtDate(r.date)}</td>
        <td>${r.signed ? `<span class="pill ok">${sf("checkmark.seal.fill")}Signed</span>` : r.signed === false ? `<span class="dim">Unsigned</span>` : `<span class="dim">?</span>`}</td>
        <td>${r.files.length ? `<span class="pill ok" title="${esc(r.files.map((f) => f.source + ": " + f.fileName).join("\n"))}">${sf("checkmark.circle.fill")}${r.files.length} files</span>` : `<span class="dim">—</span>`}</td>
        <td class="acts">${r.signed ? `<button class="btn small" data-save="${esc(r.build)}" data-v="${esc(r.version)}">${sf("square.and.arrow.down")}Save</button>` : r.signed == null && r.url ? `<button class="btn small" data-save="${esc(r.build)}" data-v="${esc(r.version)}" title="Signing status unknown (beta). iPacked asks Apple; it saves only if the build is still signed.">${sf("square.and.arrow.down")}Try save</button>` : ""}
          ${r.files.length ? `<button class="btn small icon-only" data-show="${esc(r.files[0].path)}" title="Show in Finder">${sf("folder")}</button>` : ""}</td></tr>`
      )
      .join("")}</tbody></table>`;
  $("#fw-only").onchange = (e) => ((S.fwOnlySaved = e.target.checked), renderBlobDetail(ecid));
  $("#fw-signed").onchange = (e) => ((S.fwOnlySigned = e.target.checked), renderBlobDetail(ecid));
  const rowOf = (b) => rows.find((r) => r.build === b) || {};
  $$("[data-show]", el).forEach((b) => (b.onclick = () => invoke("reveal", { path: b.dataset.show })));
  const save = async (v, b, btn) => {
    if (btn) (btn.disabled = true), (btn.innerHTML = `${sf("hourglass")}Saving…`);
    try {
      const r = rowOf(b);
      // Betas aren't in tsschecker's own database → pass the OTA/IPSW URL so the
      // BuildManifest gets pulled out of it remotely.
      toast(await invoke("blobs_save", { ecid, version: v, build: b, ota: !!r.ota, beta: !!r.beta, url: r.url || null }));
    } catch (e) {
      fail(e);
    }
  };
  $$("[data-save]", el).forEach((b) => (b.onclick = async () => (await save(b.dataset.v, b.dataset.save, b), loadBlobs())));
  $("[data-b=saveall]", el).onclick = async () => {
    if (!signed.length) return toast("No signed versions for this device right now");
    for (const s of signed) await save(s.version, s.build);
    loadBlobs();
  };
}

async function blobDeviceDialog(d = {}, editing = false) {
  const r = await ask(
    `<h3>${sf("iphone")} ${editing ? "Edit device" : "Add device"}</h3>
     <label>Name<input name="name" value="${esc(d.name || "")}" placeholder="e.g. Mom's iPad"></label>
     <label>ECID (decimal or 0x… hex)<input name="ecid" value="${esc(d.ecid || "")}"></label>
     <label>Model (ProductType)<input name="productType" value="${esc(d.productType || "")}" placeholder="e.g. iPhone10,3"></label>
     <label>Board config<input name="boardConfig" value="${esc(d.boardConfig || "")}" placeholder="e.g. d22ap"></label>
     <label>Generator (optional)<input name="generator" value="${esc(d.generator || "")}" placeholder="0x1111111111111111"></label>
     <label>ApNonce (A12+, optional)<input name="apnonce" value="${esc(d.apnonce || "")}"></label>
     <div class="row-btns" style="justify-content:flex-start;margin-top:6px"><button type="button" class="btn small" id="rd-nonce">${sf("cable.connector")}Read ApNonce + generator from connected device</button><span class="sub" id="rd-msg"></span></div>
     <div class="sub">Like blobsaver: the device reboots into recovery mode once and back (about a minute). Needs <span class="mono">irecovery</span> (brew install libirecovery).</div>
     <label>Note<input name="note" value="${esc(d.note || "")}"></label>
     <label>Icon</label><div class="pick" id="pk-icon">${DEV_ICONS.map((i) => `<button type="button" class="pk ${(d.icon || devIcon(d.productType)) === i ? "on" : ""}" data-v="${i}">${sf(i)}</button>`).join("")}</div>
     <label>Colour</label><div class="pick" id="pk-color">${DEV_COLORS.map((c) => `<button type="button" class="pk sw ${(d.color || "") === c ? "on" : ""}" data-v="${c}" style="background:${c || "var(--accent-soft)"}">${c ? "" : "A"}</button>`).join("")}<input type="color" class="pk-custom" value="${esc(d.color || "#0a84ff")}" title="Custom colour"></div>
     <input type="hidden" name="icon" value="${esc(d.icon || "")}"><input type="hidden" name="color" value="${esc(d.color || "")}">
     ${editing ? `<label class="chk"><input type="checkbox" name="remove"> Remove device from list (blob files are kept)</label>` : ""}`,
    editing ? "Save" : "Add",
    () => {
      const pick = (box, name) =>
        $$(`#${box} .pk`).forEach(
          (b) =>
            (b.onclick = () => {
              $$(`#${box} .pk`).forEach((x) => x.classList.toggle("on", x === b));
              $(`[name=${name}]`).value = b.dataset.v;
            })
        );
      pick("pk-icon", "icon");
      pick("pk-color", "color");
      $("#rd-nonce").onclick = async () => {
        const dev = (S.devices?.length ? S.devices : await invoke("devices_list").catch(() => []))[0];
        if (!dev) return ($("#rd-msg").textContent = "No device connected");
        const btn = $("#rd-nonce");
        btn.disabled = true;
        const off = await T.event.listen("apnonce-progress", (e) => ($("#rd-msg").textContent = e.payload));
        try {
          const r = await invoke("blobs_read_apnonce", { udid: dev.udid });
          $("[name=apnonce]").value = r.apnonce;
          if (r.generator) $("[name=generator]").value = r.generator;
          $("#rd-msg").textContent = "Done.";
        } catch (e) {
          $("#rd-msg").textContent = String(e);
        } finally {
          off();
          btn.disabled = false;
        }
      };
      $(".pk-custom").oninput = (ev) => {
        $$("#pk-color .pk").forEach((x) => x.classList.remove("on"));
        $("[name=color]").value = ev.target.value;
      };
    }
  );
  if (!r) return;
  try {
    if (r.remove) {
      await invoke("blobs_device_remove", { ecid: d.ecid });
      S.blobSel = null;
      $("#blob-detail").innerHTML = "";
    } else {
      const { remove, ...dev } = r;
      const saved = await invoke("blobs_device_upsert", { device: { udid: d.udid || "", ...dev }, oldEcid: editing ? d.ecid : null });
      S.blobSel = saved.ecid;
      const n = await invoke("blobs_collect");
      if (n) toast(`${n} blobs collected from the source folders for this device`);
    }
  } catch (e) {
    fail(e);
  }
  loadBlobs();
}

$("#blob-add-conn").onclick = async () => {
  if (!S.devices.length) await loadDevices().catch(() => {});
  if (!S.devices.length) return fail("No connected device");
  const dev = S.devices.length === 1 ? S.devices[0] : null;
  let udid = dev?.udid;
  if (!udid) {
    const r = await ask(`<h3>Connected device</h3><select name="u">${S.devices.map((d) => `<option value="${d.udid}">${esc(d.name)}</option>`).join("")}</select>`, "Choose");
    if (!r) return;
    udid = r.u;
  }
  try {
    const d = await invoke("blobs_read_connected", { udid });
    const existing = S.blobs.settings.devices.find((x) => x.ecid === d.ecid);
    blobDeviceDialog(existing ? { ...d, ...existing } : d, !!existing);
  } catch (e) {
    fail("Could not read device info: " + e);
  }
};
$("#blob-add").onclick = () => blobDeviceDialog();
function filterBlobDevs() {
  const q = $("#blob-filter").value.toLowerCase().trim();
  $$("#blob-devs [data-q]").forEach((r) => r.classList.toggle("hidden", !!q && !r.dataset.q.includes(q)));
}
$("#blob-filter").oninput = filterBlobDevs;
$("#blob-collect").onclick = async () => {
  try {
    const n = await invoke("blobs_collect");
    toast(n ? `${n} blob files copied to device folders` : "No new blobs to collect");
    loadBlobs();
  } catch (e) {
    fail(e);
  }
};
$("#blob-open").onclick = () => invoke("reveal", { path: S.blobs.root });
$("#blob-sources").onclick = async () => {
  const draw = () =>
    S.blobs.settings.sources.map((s, i) => `<div class="srcrow"><span class="mono">${esc(s)}</span><button class="btn small" type="button" data-rm="${i}">Remove</button></div>`).join("") ||
    `<p class="sub">No source folders yet.</p>`;
  const m = $("#modal");
  $("#modal-body").innerHTML = `<h3>${sf("folder")} Blob source folders</h3>
    <p class="sub">Folders to scan, e.g. Legacy iOS Kit → <span class="mono">Legacy-iOS-Kit/saved/shsh</span>, blobsaver → its save folder. Blobs stay in these folders; "Collect into library" copies those of saved devices into iPacked's device folders.</p>
    <div id="srcs">${draw()}</div>
    <div class="row-btns"><button class="btn" type="button" id="src-add">${sf("plus.circle")}Add folder</button><button class="btn primary" value="ok">OK</button></div>`;
  const bind = () =>
    $$("[data-rm]").forEach(
      (b) =>
        (b.onclick = async () => {
          S.blobs.settings.sources.splice(+b.dataset.rm, 1);
          await invoke("blobs_set_sources", { sources: S.blobs.settings.sources });
          $("#srcs").innerHTML = draw();
          bind();
        })
    );
  bind();
  $("#src-add").onclick = async () => {
    const dir = await T.dialog.open({ directory: true });
    if (!dir) return;
    S.blobs.settings.sources.push(dir);
    await invoke("blobs_set_sources", { sources: S.blobs.settings.sources });
    $("#srcs").innerHTML = draw();
    bind();
  };
  m.onclose = () => loadBlobs();
  m.showModal();
};

// ---------- modal helper ----------
function ask(html, okLabel = "OK", after) {
  const m = $("#modal");
  $("#modal-body").innerHTML = `${html}<div class="row-btns"><button class="btn" value="cancel">Cancel</button><button class="btn primary" value="ok">${esc(okLabel)}</button></div>`;
  m.showModal();
  after?.();
  return new Promise((res) => {
    m.onclose = () => {
      if (m.returnValue !== "ok") return res(null);
      const out = {};
      $$("input,select", m).forEach((i) => (out[i.name] = i.type === "checkbox" ? i.checked : i.value));
      res(out);
    };
  });
}

// ---------- devices ----------
async function loadDevices() {
  $("#dev-apps").innerHTML = `<div class="empty">Looking for devices…</div>`;
  try {
    S.devices = await invoke("devices_list");
  } catch (e) {
    S.devices = [];
    $("#dev-apps").innerHTML = `<div class="empty">${esc(e)}</div>`;
  }
  const sel = $("#dev-select");
  const prevUdid = S.devices.find((d) => d.id === sel.value)?.udid || S.selUdid;
  sel.innerHTML = S.devices.map((d) => `<option value="${d.id}">${esc(d.name)} (${d.connection})</option>`).join("") || `<option value="">No device</option>`;
  const keep = S.devices.find((d) => d.udid === prevUdid);
  if (keep) sel.value = keep.id;
  S.selUdid = S.devices.find((d) => d.id === sel.value)?.udid;
  if (S.devices.length) loadDeviceApps(), renderClutch(), renderDumpCard();
  else if (!$("#dev-apps .empty")?.textContent.includes("usbmuxd")) $("#dev-apps").innerHTML = `<div class="empty">No connected device. Connect the iPhone over USB and tap "Trust This Computer".</div>`;
}
$("#dev-select").onchange = () => (loadDeviceApps(), renderClutch(), renderDumpCard(), Object.assign(S.fs, { mode: "afc", path: "/", hist: [], fwd: [], sel: new Set() }), renderFs());
$("#dev-refresh").onclick = () => (loadDevices(), renderTools());

async function loadDeviceApps() {
  const id = $("#dev-select").value;
  if (!id) return;
  $("#dev-apps").innerHTML = `<div class="empty">Reading apps…</div>`;
  try {
    const apps = await invoke("device_apps", { id });
    S.deviceApps = apps;
    $("#dev-apps").innerHTML = `<table><thead><tr><th>App</th><th>Bundle ID</th><th>Version</th><th>Build</th><th>Min iOS</th><th>Kind</th><th>Archive</th><th></th></tr></thead><tbody>
      ${apps
        .map((a) => {
          const n = S.archive.filter((e) => e.bundleId === a.bundleId).length;
          return `<tr data-q="${esc((a.name + " " + a.bundleId + " " + (a.appleId || "")).toLowerCase())}"><td><div class="appcell"><div class="icon sm" data-devicon="${esc(a.bundleId)}"></div><b>${esc(a.name)}</b></div></td><td class="mono">${esc(a.bundleId)}</td><td>${esc(a.version)}</td><td class="mono">${esc(a.build)}</td>
          <td>${esc(a.minIos || "—")}</td><td>${a.appStore ? `<span class="pill" title="${esc(a.appleId ? "Apple ID: " + a.appleId : "Apple ID unknown")}">${sf("app.badge")}App Store</span>` : `<span class="pill warn">Sideload</span>`}</td>
          <td>${n ? `<span class="pill ok">${sf("checkmark.seal.fill")}${n} versions</span>` : "—"}</td>
          <td><button class="btn small" data-export="${esc(a.bundleId)}" data-name="${esc(a.name)}">${sf("square.and.arrow.up")}Export</button></td></tr>`;
        })
        .join("")}</tbody></table>`;
    $$("[data-export]").forEach((b) => (b.onclick = () => exportDialog(id, b.dataset.export, b.dataset.name)));
    filterDevice();
    loadDeviceIcons(id);
    // Old iOS: purchaser Apple ID comes from each app's iTunesMetadata.plist (AFC2).
    const need = apps.filter((a) => !a.appleId && a.container).map((a) => [a.bundleId, a.container]);
    if (need.length)
      invoke("device_apple_ids", { id, apps: need }).then((m) => {
        if ($("#dev-select").value !== id) return;
        for (const [bid, aid] of Object.entries(m)) {
          const a = apps.find((x) => x.bundleId === bid);
          if (!a) continue;
          a.appleId = aid;
          a.appStore = true;
          const tr = $(`#dev-apps tr[data-q] [data-devicon="${CSS.escape(bid)}"]`)?.closest("tr");
          const cell = tr?.children[5];
          if (cell) cell.innerHTML = `<span class="pill" title="${esc(aid ? "Apple ID: " + aid : "Apple ID unknown")}">${sf("app.badge")}App Store</span>`;
          if (tr) tr.dataset.q += " " + aid.toLowerCase();
        }
      });
  } catch (e) {
    $("#dev-apps").innerHTML = `<div class="empty">${esc(e)}</div>`;
  }
}

function filterDevice() {
  const q = $("#dev-filter").value.toLowerCase().trim();
  $$("#dev-apps tr[data-q], #dev-fs tr[data-q]").forEach((r) => r.classList.toggle("hidden", !!q && !r.dataset.q.includes(q)));
}
$("#dev-filter").oninput = filterDevice;

async function loadDeviceIcons(devId) {
  const cells = $$("[data-devicon]");
  let i = 0;
  const worker = async () => {
    while (i < cells.length) {
      const el = cells[i++];
      const bid = el.dataset.devicon;
      let p = await invoke("device_icon", { id: devId, bundleId: bid }).catch(() => null);
      if (!p) p = S.archive.find((e) => e.bundleId === bid && e.iconPath)?.iconPath;
      if (p) el.innerHTML = `<img src="${fileSrc(p)}?v=1">`;
    }
  };
  await Promise.all([worker(), worker(), worker()]);
}

// ---------- export (archive / bagbak / Clutch) ----------
async function renderTools() {
  S.tools = await invoke("tools_status");
  const t = (k, label) =>
    `<button class="tool ${S.tools[k] ? "ok" : "no"}" data-tool="${k}" title="Check again">${sf(S.tools[k] ? "checkmark.circle.fill" : "xmark.circle.fill")}${label}</button>`;
  $("#dev-tools").innerHTML = t("ideviceinstaller", "ideviceinstaller") + t("iproxy", "iproxy") + t("bagbak", "bagbak") + t("frida", "frida") + t("ssh", "ssh");
  $$("#dev-tools [data-tool]").forEach(
    (b) =>
      (b.onclick = async () => {
        b.classList.add("checking");
        const was = S.tools[b.dataset.tool];
        await renderTools();
        const now = S.tools[b.dataset.tool];
        toast(`${b.dataset.tool}: ${now ? "installed" : was ? "no longer there (removed)" : "not installed"}`);
      })
  );
}

// ---------- Clutch on device (/bin/clutch) + SSH settings ----------
async function renderClutch(force) {
  const id = $("#dev-select").value;
  const el = $("#dev-clutch");
  if (!id) return (el.innerHTML = "");
  S.clutch ||= {};
  if (!force && S.clutch[id]) return paintClutch(id);
  el.innerHTML = `<span class="dim">${sf("hourglass")}Checking Clutch (SSH)…</span>`;
  try {
    S.clutch[id] = await invoke("clutch_status", { id });
  } catch (e) {
    S.clutch[id] = { error: String(e) };
  }
  paintClutch(id);
}
function paintClutch(id) {
  const c = S.clutch[id];
  const el = $("#dev-clutch");
  if (c.error)
    el.innerHTML = `<button class="tool no" data-c="re" title="${esc(c.error)}">${sf("xmark.circle.fill")}clutch (no SSH)</button>
      <button class="btn small primary" data-c="ssh">${sf("network")}Set up SSH first</button><button class="btn small" data-c="re">${sf("arrow.clockwise")}</button>`;
  else if (c.installed)
    el.innerHTML = `<button class="tool ok" data-c="re" title="${esc(c.via)} · check again">${sf("checkmark.circle.fill")}clutch <span class="mono dim">${esc(c.path)}</span></button>`;
  else
    el.innerHTML = `<button class="tool no" data-c="re" title="Check again">${sf("xmark.circle.fill")}clutch</button>
      <button class="btn small primary" data-c="file">${sf("square.and.arrow.down.on.square")}Install Clutch…</button>`;
  const on = (k, f) => $(`[data-c=${k}]`, el) && ($(`[data-c=${k}]`, el).onclick = f);
  on("re", async () => {
    const was = c.installed;
    await renderClutch(true);
    const now = S.clutch[id]?.installed;
    toast(now ? "Clutch installed" : was ? "Clutch is gone (removed)" : "Clutch not installed");
  });
  on("ssh", sshDialog);
  on("file", clutchFileDialog);
}

async function clutchFileDialog() {
  const id = $("#dev-select").value;
  if (!id) return fail("Select a device first");
  const ok = await ask(
    `<h3>${sf("square.and.arrow.down.on.square")} Install Clutch</h3>
     <p>Choose the Clutch file you downloaded: the program itself (<span class="mono">Clutch</span>, <span class="mono">Clutch2</span>, <span class="mono">Clutch-2.0.4</span>…) or a <span class="mono">.deb</span> paketi.</p>
     <p class="sub wrap">iPacked puts the file on the device as <span class="mono">/bin/clutch</span> : the name becomes lower-case <b>clutch</b> with execute permission (755). On rootless jailbreaks the location is <span class="mono">/var/jb/usr/bin/clutch</span> .<br>
     Use Clutch 2.0.4 for iOS 6–11 (github.com/KJCracks/Clutch/releases) and Clutch 1.4.x for iOS 5.</p>`,
    "Choose file…"
  );
  if (!ok) return;
  const f = await T.dialog.open({ multiple: false, title: "Clutch file or .deb package" });
  if (!f) return;
  const el = $("#dev-clutch");
  el.innerHTML = `<span class="dim">${sf("hourglass")}Uploading Clutch → /bin/clutch…</span>`;
  try {
    toast("Clutch installed: " + (await invoke("clutch_install", { id, path: f })));
  } catch (e) {
    if (/SSH/.test(String(e))) sshMissing(e);
    else fail("Couldn't install Clutch: " + e);
  }
  renderClutch(true);
}

/// SSH isn't working → say so plainly and offer the settings.
async function sshMissing(err) {
  const r = await ask(
    `<h3>${sf("network")} Set up SSH first</h3>
     <p>Couldn't connect to the device over SSH. For Clutch and package installs the device needs <b>OpenSSH</b> installed, and its IP + root password entered in iPacked.</p>
     <details class="box"><summary>Details</summary><p class="pre sel">${esc(err)}</p></details>`,
    "Open SSH settings"
  );
  if (r) sshDialog();
}

async function sshDialog() {
  const id = $("#dev-select").value;
  if (!id) return fail("Select a device first");
  const c = await invoke("ssh_get", { id });
  const r = await ask(
    `<h3>${sf("network")} SSH (Clutch)</h3>
     <label>Device IP address<input name="host" value="${esc(c.host)}" placeholder="e.g. 192.168.x.x (empty: USB via iproxy)"></label>
     <div class="sub">Find the IP on the device under Settings → Wi-Fi → (i). Leave empty to use SSH over USB (iproxy).</div>
     <label>Port<input name="port" value="${esc(c.port)}"></label>
     <label>User<input name="user" value="${esc(c.user)}"></label>
     <label>Password<input name="password" type="password" value="${esc(c.password)}" placeholder="alpine"></label>
     <div class="notice">${sf("exclamationmark.triangle.fill")}<div>The default root password is <b>alpine</b>; if you haven't changed it, run on the device <span class="mono">passwd</span> to change it. The password is stored only on this Mac (file with 0600 permissions).</div></div>`,
    "Save"
  );
  if (!r) return;
  await invoke("ssh_set", { id, cfg: { host: r.host.trim(), port: parseInt(r.port) || 22, user: r.user || "root", password: r.password } });
  renderClutch(true);
}
$("#dev-ssh").onclick = sshDialog;
$("#dev-log-x").onclick = () => $("#dev-log").classList.add("hidden");

async function exportDialog(devId, bundleId, name) {
  const r = await ask(
    `<h3>${sf("square.and.arrow.up")} Export: ${esc(name)}</h3><div class="sub mono">${esc(bundleId)}</div>
     <label style="display:block;margin-top:8px">Method<select name="method">
       <option value="archive">Device archive — ideviceinstaller (iOS 6 and below, no jailbreak, original/encrypted)</option>
       <option value="bagbak">bagbak + Frida — decrypted dump (jailbreak + frida-server)</option>
       <option value="clutch">Clutch — decrypted dump over SSH (jailbreak + Clutch + OpenSSH)</option></select></label>
     <div class="notice">${sf("exclamationmark.triangle.fill")}<div><a href="#" id="exp-guide">Guide</a> · Clutch connects to the IP set in Devices → SSH (&lt;your_ip&gt;) or over USB and uses /bin/clutch. bagbak needs frida-server running. See "Guide" for step-by-step help. When done, the IPA is added to the archive automatically.</div></div>`,
    "Export",
    () => $("#exp-guide") && ($("#exp-guide").onclick = (e) => (e.preventDefault(), showGuide($("[name=method]").value === "bagbak" ? "bagbak" : "clutch")))
  );
  if (!r) return;
  const log = $("#dev-log");
  log.textContent = "";
  log.classList.remove("hidden");
  try {
    const e = await invoke("device_export", { id: devId, bundleId, method: r.method });
    toast(`${e.name} ${e.version} added to archive${e.encrypted === false ? " (decrypted)" : ""}`);
    S.archive = await invoke("archive_list");
    loadDeviceApps();
  } catch (e) {
    if (/SSH connection failed|iproxy/.test(String(e))) sshMissing(e);
    else fail("Export failed: " + e);
  }
}
listen("export-log", (p) => {
  const log = $("#dev-log");
  const last = p.line.split("\r").pop();
  log.textContent += last + "\n";
  log.scrollTop = log.scrollHeight;
});

async function installDialog(ipa) {
  if (!S.devices.length) await loadDevices();
  if (!S.devices.length) return fail("No connected device");
  const r = await ask(
    `<h3>Install on device</h3><div class="sub mono">${esc(ipa.split("/").pop())}</div>
     <label>Device<select name="dev">${S.devices.map((d) => `<option value="${d.id}">${esc(d.name)}</option>`).join("")}</select></label>
     <label style="display:block;margin-top:8px">Method<select name="mode">
       <option value="direct">Install with original signature (App Store IPA)</option>
       <option value="resign">Impactor: re-sign with Apple ID and install</option></select></label>`,
    "Install"
  );
  if (!r) return;
  go("devices");
  $("#dev-select").value = r.dev;
  const bar = $("#dev-progress");
  bar.classList.remove("hidden");
  try {
    await invoke("device_install", { id: r.dev, ipa, resign: r.mode === "resign" });
    toast("Installation finished");
    loadDeviceApps();
  } catch (e) {
    fail("Installation failed: " + e);
  }
  setTimeout(() => bar.classList.add("hidden"), 1500);
}
listen("install-progress", (p) => {
  $("#dev-progress .fill").style.width = p.percent + "%";
  $("#dev-progress span").textContent = `${p.stage} · %${p.percent}`;
});
$("#dev-install-file").onclick = async () => {
  const f = await T.dialog.open({ filters: [{ name: "IPA", extensions: ["ipa"] }] });
  if (f) installDialog(f);
};


// ---------- accounts ----------
function renderStoreState() {
  const st = $("#store-state");
  if (S.store) {
    st.innerHTML = `<p>Signed in: <b>${esc(S.store.name ?? "")}</b> <span class="mono">${esc(S.store.email)}</span> <button class="btn small" id="store-out">Sign out</button></p>`;
    $("#store-form").classList.add("hidden");
    $("#store-out").onclick = async () => {
      await invoke("store_logout").catch(fail);
      S.owned = [];
      loadStore();
    };
  } else {
    st.innerHTML = "";
    $("#store-form").classList.remove("hidden");
  }
}
$("#store-form").onsubmit = async (e) => {
  e.preventDefault();
  const f = Object.fromEntries(new FormData(e.target));
  const btn = $("button", e.target);
  btn.disabled = true;
  const ready = await invoke("store_assets_ready");
  const t0 = Date.now();
  const tick = setInterval(() => {
    const s = Math.floor((Date.now() - t0) / 1000);
    btn.textContent = `${ready ? "Connecting" : "Preparing Apple files"}… ${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
  }, 1000);
  if (!ready) $("#store-prep").classList.remove("hidden");
  try {
    const r = await invoke("store_login", { email: f.email, password: f.password, code: f.code || null });
    if (r.status === "need2fa") {
      $("input[name=code]", e.target).classList.remove("hidden");
      $("input[name=code]", e.target).focus();
      toast("Apple asked for verification. If two-factor authentication is on, enter the code sent to your devices and press Sign in again. If no code arrived, the email or password is wrong.");
    } else {
      e.target.reset();
      $("input[name=code]", e.target).classList.add("hidden");
      toast("App Store account connected");
      loadStore();
    }
  } catch (er) {
    fail(er);
  }
  clearInterval(tick);
  $("#store-prep").classList.add("hidden");
  btn.disabled = false;
  btn.textContent = "Sign in";
};

async function renderDevState() {
  const email = await invoke("dev_account");
  $("#dev-state").innerHTML = email ? `<p>Signed in: <span class="mono">${esc(email)}</span> <button class="btn small" id="dev-out">Sign out</button></p>` : "";
  $("#devacc-form").classList.toggle("hidden", !!email);
  if (email) $("#dev-out").onclick = async () => (await invoke("dev_logout").catch(fail), renderDevState());
}
$("#devacc-form").onsubmit = async (e) => {
  e.preventDefault();
  const f = Object.fromEntries(new FormData(e.target));
  const btn = $("button", e.target);
  btn.disabled = true;
  btn.textContent = "Connecting…";
  try {
    await invoke("dev_login", { email: f.email, password: f.password });
    e.target.reset();
    toast("Signing account connected");
  } catch (er) {
    fail(er);
  }
  btn.disabled = false;
  btn.textContent = "Sign in";
  renderDevState();
};
listen("dev-2fa", async (p) => {
  const r = await ask(
    `<h3>Two-factor authentication</h3><p>Enter the code sent ${p.method === "sms" ? "by SMS" : "to your devices"}.${p.canSms ? " Type <b>sms</b> to get it by SMS." : ""}</p><input name="code" autofocus autocomplete="one-time-code">`,
    "Send"
  );
  await invoke("dev_submit_2fa", { code: r?.code ?? "" }).catch(fail);
});

// ---------- boot ----------
(async () => {
  decorateSf();
  S.view = "all";
  invoke("store_set_platform", { platform: "" });
  $("#root-path").textContent = await invoke("archive_root");
  S.archive = await invoke("archive_list");
  loadStore();
  renderDevState();
  // Re-read the archive folder: deleted files drop out, new ones are analysed
  // from the .ipa itself, identical copies are removed (see deleted.log).
  invoke("archive_rescan")
    .then((r) => {
      if (r.duplicates.length) toast(`Identical ${r.duplicates.length} files deleted (deleted.log): ${r.duplicates.join(", ")}`);
      if (r.added || r.missing) toast(`Archive updated: ${r.added} new, ${r.missing} deleted files`);
      return invoke("archive_list");
    })
    .then((a) => ((S.archive = a), $("#view-archive").classList.contains("active") && renderArchive()))
    .catch(() => {});
})();

// ---------- Cmd+F: focus the current screen's search field ----------
document.addEventListener("keydown", (e) => {
  if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "f") {
    const f = $(".view.active .bar .search") || $(".view.active .search");
    if (f) e.preventDefault(), f.focus(), f.select();
  }
  if (e.key === "Escape" && document.activeElement?.classList.contains("search") && document.activeElement.value) {
    document.activeElement.value = "";
    document.activeElement.dispatchEvent(new Event("input"));
  }
});


// ---------- preferences (Settings) ----------
async function loadPrefs() {
  S.prefs = await invoke("prefs_get").catch(() => ({}));
  const tpl = $("#pref-template");
  if (!tpl) return;
  tpl.value = S.prefs.nameTemplate || "";
  const preview = async () => ($("#pref-preview").textContent = await invoke("name_preview", { template: tpl.value, entry: null }).catch(() => "?"));
  preview();
  let t;
  tpl.oninput = () => {
    preview();
    clearTimeout(t);
    t = setTimeout(() => savePrefs({ nameTemplate: tpl.value }), 400);
  };
  $("#pref-tpl-reset").onclick = () => ((tpl.value = "<name>_<version>_<BuildNo>_(<ExternalVersionID>)"), tpl.oninput());
  $("#pref-rename").onclick = async () => {
    const ok = await ask(`<h3>${sf("pencil")} Rename files</h3><p>Rename every IPA/PKG in the archive using this template?</p><p><code>${esc(tpl.value)}</code></p>`, "Rename");
    if (!ok) return;
    await savePrefs({ nameTemplate: tpl.value });
    const n = await invoke("archive_rename_all").catch(fail);
    if (n != null) toast(`${n} files renamed`);
    S.archive = await invoke("archive_list");
  };
  $$("[data-pref]").forEach((c) => {
    c.checked = !!S.prefs[c.dataset.pref];
    c.onchange = () => savePrefs({ [c.dataset.pref]: c.checked });
  });
  $("#ia-url").value = S.prefs.iaUrl || "";
}
$("#credit-link").onclick = (e) => (e.preventDefault(), invoke("open_url", { url: "https://github.com/Xmior" }));

// ---------- Archive.org importer (Settings) ----------
S.ia = { files: [], meta: {}, picked: new Set(), sort: "name", asc: true, q: "", jobs: [] };
$("#ia-load").onclick = async () => {
  const url = $("#ia-url").value.trim();
  if (!url) return;
  savePrefs({ iaUrl: url });
  const box = $("#ia-box");
  box.innerHTML = `<div class="empty"><span class="spin"></span> Fetching list…</div>`;
  try {
    const l = await invoke("ia_list", { url });
    Object.assign(S.ia, { item: l.item, base: l.base, title: l.title, files: l.files, meta: {}, picked: new Set() });
  } catch (e) {
    box.innerHTML = `<div class="empty sel">${esc(e)}</div>`;
    return;
  }
  renderIa();
  // Read name / bundle ID / min iOS / icon of each file, 4 at a time.
  let i = 0;
  const item = S.ia.item;
  const w = async () => {
    while (i < S.ia.files.length && S.ia.item === item) {
      const f = S.ia.files[i++];
      S.ia.meta[f.name] = await invoke("ia_inspect", { base: S.ia.base, item, name: f.name }).catch((e) => ({ error: String(e) }));
      iaTick();
    }
  };
  await Promise.all([w(), w(), w(), w()]);
  renderIa();
};
let iaT;
const iaTick = () => (clearTimeout(iaT), (iaT = setTimeout(renderIa, 300)));

function renderIa() {
  const box = $("#ia-box");
  const ia = S.ia;
  if (!ia.files.length) return (box.innerHTML = `<div class="empty">This item has no IPAs.</div>`);
  const done = Object.keys(ia.meta).length;
  const rows = ia.files
    .map((f) => ({ f, m: ia.meta[f.name] || {} }))
    .filter(({ f, m }) => !ia.q || [f.name, m.name, m.bundleId, m.minIos].join(" ").toLowerCase().includes(ia.q));
  const ver = (v) => verNum(v || "99");
  const cmpV = (a, b) => { const x = ver(a), y = ver(b); for (let k = 0; k < 3; k++) if ((x[k] || 0) !== (y[k] || 0)) return (x[k] || 0) - (y[k] || 0); return 0; };
  const byName = (a, b) => (a.m.name || a.f.name).localeCompare(b.m.name || b.f.name, "en", { numeric: true, sensitivity: "base" });
  const cmp =
    ia.sort === "bundle" ? (a, b) => (a.m.bundleId || "~").localeCompare(b.m.bundleId || "~", "en", { sensitivity: "base" }) || byName(a, b)
    : ia.sort === "ios" ? (a, b) => cmpV(a.m.minIos, b.m.minIos) || byName(a, b)
    : ia.sort === "size" ? (a, b) => (a.f.size || 0) - (b.f.size || 0) || byName(a, b)
    : byName;
  rows.sort(ia.asc ? cmp : (a, b) => cmp(b, a));
  const keepTop = $("#ia-box .ia-list")?.scrollTop || 0;
  const iaPage = pageOf("ia", rows, ia.item + "|" + ia.q + "|" + ia.sort + "|" + ia.asc);
  const sel = [...ia.picked];
  const selSize = ia.files.filter((f) => ia.picked.has(f.name)).reduce((s, f) => s + (f.size || 0), 0);
  box.innerHTML = `<div class="ia-tools">
      <b>${esc(ia.title || ia.item)}</b><span class="sub">${ia.files.length} IPA${done < ia.files.length ? ` · reading info ${done}/${ia.files.length}` : ""}</span><span class="grow"></span>
      <input id="ia-q" class="search" placeholder="Search…" value="${esc(ia.q)}"><span class="sub">Sort by</span>
      <select id="ia-sort" class="select"><option value="name">Name</option><option value="bundle">Bundle ID</option><option value="ios">Min iOS</option><option value="size">Size</option></select><button class="btn small" id="ia-dir" title="Sort direction">${ia.asc ? "↑ Low → High" : "↓ High → Low"}</button>
      <button class="btn small" id="ia-all">Select visible</button><button class="btn small" id="ia-none">Clear selection</button>
      <button class="btn small primary" id="ia-go" ${sel.length ? "" : "disabled"}>${sf("arrow.down.circle")}Download to archive (${sel.length} · ${fmtSize(selSize)})</button></div>
    <div class="ia-list"><table><thead><tr><th></th>${[["name", "Name"], ["bundle", "Bundle ID"], ["ios", "Min iOS"], ["size", "Size"]].map(([k, l]) => `<th class="sortable ${ia.sort === k ? "on" : ""}" data-iasort="${k}">${l}${ia.sort === k ? (ia.asc ? " ▲" : " ▼") : ""}</th>`).join("")}</tr></thead><tbody>${iaPage.items
      .map(({ f, m }) => {
        const have = m.bundleId && S.archive.some((e) => e.bundleId === m.bundleId && e.version === m.version && e.build === m.build);
        return `<tr data-f="${esc(f.name)}" class="${ia.picked.has(f.name) ? "on" : ""}">
        <td><input type="checkbox" ${ia.picked.has(f.name) ? "checked" : ""}></td>
        <td><div class="appcell">${iconHtml({ platform: m.platform, iconPath: m.iconPath })}<div><b>${esc(m.name || f.name)}</b> ${m.version ? `<span class="sub">v${esc(m.version)}</span>` : ""}${have ? ` <span class="pill ok">${sf("checkmark.seal.fill")}Archived</span>` : ""}</div></div></td>
        <td class="sub mono">${esc(m.bundleId || (m.error ? "unreadable: " + m.error : "…"))}</td>
        <td>${m.minIos ? `iOS ${esc(m.minIos)}+` : ""}</td><td class="sub">${fmtSize(f.size || 0)}</td></tr>`;
      })
      .join("")}</tbody></table>${pagerHtml("ia", iaPage)}</div>
    <div class="ia-dl" id="ia-dl"></div>`;
  bindPager($("#ia-box"), "ia", renderIa);
  $("#ia-sort").value = ia.sort;
  $("#ia-box .ia-list").scrollTop = keepTop;
  $$("#ia-box [data-iasort]").forEach((h) => (h.onclick = () => (ia.sort === h.dataset.iasort ? (ia.asc = !ia.asc) : ((ia.sort = h.dataset.iasort), (ia.asc = true)), renderIa())));
  $("#ia-sort").onchange = (e) => ((ia.sort = e.target.value), renderIa());
  $("#ia-dir").onclick = () => ((ia.asc = !ia.asc), renderIa());
  $("#ia-q").oninput = (e) => {
    ia.q = e.target.value.toLowerCase();
    clearTimeout(iaT);
    iaT = setTimeout(() => (renderIa(), $("#ia-q").focus(), $("#ia-q").setSelectionRange(99, 99)), 250);
  };
  $("#ia-all").onclick = () => (rows.forEach(({ f }) => ia.picked.add(f.name)), renderIa());
  $("#ia-none").onclick = () => (ia.picked.clear(), renderIa());
  $$("#ia-box tr[data-f]").forEach((r) => (r.onclick = () => (ia.picked.has(r.dataset.f) ? ia.picked.delete(r.dataset.f) : ia.picked.add(r.dataset.f), renderIa())));
  $("#ia-go").onclick = () => {
    for (const n of sel)
      if (!ia.jobs.some((j) => j.name === n && ["queued", "running"].includes(j.state)))
        ia.jobs.push({ name: n, item: ia.item, base: ia.base, size: ia.files.find((f) => f.name === n)?.size || 0, state: "queued", bytes: 0 });
    ia.picked.clear();
    renderIa();
    pumpIa();
  };
  renderIaJobs();
}

function renderIaJobs() {
  const el = $("#ia-dl");
  if (!el) return;
  const jobs = S.ia.jobs;
  if (!jobs.length) return (el.innerHTML = "");
  const left = jobs.filter((j) => ["queued", "running"].includes(j.state)).length;
  const tot = jobs.reduce((s, j) => s + (j.size || 0), 0);
  const got = jobs.reduce((s, j) => s + (j.state === "done" ? j.size || 0 : j.bytes || 0), 0);
  el.innerHTML = `<div class="dl-head"><b>${sf("arrow.down.circle")} Archive.org downloads</b>
      <span class="sub">%${tot ? Math.floor((got / tot) * 100) : 0} · ${left} files left</span><span class="grow"></span>
      <button class="btn small" id="ia-clear">Clear finished</button></div>
    ${jobs
      .map((j) => {
        const m = S.ia.meta[j.name] || {};
        const pct = j.state === "done" ? 100 : j.size ? (j.bytes / j.size) * 100 : 0;
        return `<div class="dl-row">${iconHtml({ platform: m.platform, iconPath: m.iconPath })}<div class="grow"><div class="name">${esc(m.name || j.name)}</div>
          <div class="bar"><div style="width:${pct}%"></div></div>
          <div class="sub">${j.state === "error" ? `<span class="sel">${esc(j.error)}</span>` : j.state === "done" ? "Added to archive" : j.state === "analyzing" ? "Analyzing…" : j.state === "queued" ? "Queued" : `${fmtSize(j.bytes)} / ${fmtSize(j.size)}`}</div></div></div>`;
      })
      .join("")}`;
  $("#ia-clear").onclick = () => ((S.ia.jobs = jobs.filter((j) => ["queued", "running", "analyzing"].includes(j.state))), renderIaJobs());
}

function pumpIa() {
  const running = S.ia.jobs.filter((j) => j.state === "running" || j.state === "analyzing").length;
  const next = S.ia.jobs.find((j) => j.state === "queued");
  if (running >= 2 || !next) return renderIaJobs();
  next.state = "running";
  invoke("ia_download", { base: next.base, item: next.item, name: next.name })
    .then((e) => ((next.state = "done"), toast(`${e.name} ${e.version} added to archive`)))
    .catch((e) => ((next.state = "error"), (next.error = String(e))))
    .finally(async () => {
      S.archive = await invoke("archive_list").catch(() => S.archive);
      renderIaJobs();
      pumpIa();
    });
  pumpIa();
}
listen("ia-progress", (p) => {
  const j = S.ia.jobs.find((x) => x.name === p.file && ["running", "analyzing"].includes(x.state));
  if (!j) return;
  j.bytes = p.bytes;
  if (p.total) j.size = p.total;
  if (p.stage === "analyzing") j.state = "analyzing";
  renderIaJobs();
});
async function savePrefs(p) {
  S.prefs = await invoke("prefs_set", { prefs: p }).catch((e) => (fail(e), S.prefs));
}
loadPrefs();

// ⌘F focuses the search box of the current view.
document.addEventListener("keydown", (ev) => {
  if ((ev.metaKey || ev.ctrlKey) && ev.key.toLowerCase() === "f") {
    const box = $(".view.active .search");
    if (box) ev.preventDefault(), box.focus(), box.select();
  }
  if (ev.key === "Escape" && S.sel) (S.sel = null), renderArchive();
});


// ---------- sidebar: hide / resize ----------
(() => {
  const app = $(".app"), side = $(".sidebar");
  try {
    const w = +localStorage.getItem("sideW");
    if (w) side.style.width = w + "px";
    app.style.setProperty("--side-w", side.getBoundingClientRect().width + "px");
    if (localStorage.getItem("sideHidden") === "1") app.classList.add("side-hidden");
  } catch {}
  $("#side-toggle").onclick = () => {
    app.classList.toggle("side-hidden");
    try { localStorage.setItem("sideHidden", app.classList.contains("side-hidden") ? "1" : "0"); } catch {}
  };
  $(".side-resize").onmousedown = (ev) => {
    ev.preventDefault();
    const move = (e) => {
      side.style.width = Math.max(150, Math.min(360, e.clientX)) + "px";
      app.style.setProperty("--side-w", side.style.width);
    };
    const up = () => {
      document.removeEventListener("mousemove", move);
      document.removeEventListener("mouseup", up);
      try { localStorage.setItem("sideW", parseInt(side.style.width)); } catch {}
    };
    document.addEventListener("mousemove", move);
    document.addEventListener("mouseup", up);
  };
})();
// Dump panel belongs to Archive / Devices only.
const _go = go;
go = (view) => {
  _go(view);
  if (!["archive", "devices"].includes(view)) $("#dump-panel")?.classList.add("hidden");
  if (view !== "devices") $("#log-panel")?.classList.add("hidden");
  if (view !== "library") $("#dl-panel")?.classList.add("hidden");
};
$$(".nav").forEach((b) => (b.onclick = () => go(b.dataset.view)));


// ---------- device log (Devices → Log) ----------
S.devlog = [];
listen("device-log", (p) => {
  S.devlog.push(`${p.t}  ${p.line}`);
  if (S.devlog.length > 3000) S.devlog.splice(0, 1000);
  const pre = $("#log-panel pre");
  if (pre && !$("#log-panel").classList.contains("hidden")) {
    const atEnd = pre.scrollTop + pre.clientHeight >= pre.scrollHeight - 20;
    pre.textContent += `${p.t}  ${p.line}\n`;
    if (atEnd) pre.scrollTop = pre.scrollHeight;
  }
});
$("#dev-logbtn").onclick = async () => {
  const el = $("#log-panel");
  if (!el.classList.contains("hidden")) return el.classList.add("hidden");
  const lines = await invoke("devlog_tail", { n: 1500 }).catch(() => S.devlog);
  el.innerHTML = `<div class="dl-head"><b>${sf("doc.text")} Device log</b><span class="grow"></span>
      <button class="btn small" id="lg-copy">Copy</button><button class="btn small" id="lg-open">Show file</button>
      <button class="btn small icon-only" id="lg-close">${sf("xmark.circle.fill")}</button></div>
    <pre class="devlog sel">${esc(lines.join("\n"))}\n</pre>`;
  el.classList.remove("hidden");
  const pre = $("pre", el);
  pre.scrollTop = pre.scrollHeight;
  $("#lg-close").onclick = () => el.classList.add("hidden");
  $("#lg-copy").onclick = () => navigator.clipboard.writeText(pre.textContent).then(() => toast("Log copied"));
  $("#lg-open").onclick = async () => invoke("reveal", { path: await invoke("devlog_path") });
};


// ---------- theme: System / Light / Dark ----------
function applyTheme(t) {
  if (t === "light" || t === "dark") document.documentElement.dataset.theme = t;
  else delete document.documentElement.dataset.theme;
  $$("#theme-seg [data-theme]").forEach((b) => b.classList.toggle("active", b.dataset.theme === (t || "system")));
  try {
    T.window?.getCurrentWindow?.().setTheme?.(t === "light" || t === "dark" ? t : null);
  } catch {}
}
{
  let t = "system";
  try { t = localStorage.getItem("ipacked.theme") || "system"; } catch {}
  applyTheme(t);
  $$("#theme-seg [data-theme]").forEach(
    (b) =>
      (b.onclick = () => {
        try { localStorage.setItem("ipacked.theme", b.dataset.theme); } catch {}
        applyTheme(b.dataset.theme);
      })
  );
}

// ---------- password fields: eye toggle + caps lock indicator ----------
function enhancePw(inp) {
  if (inp.dataset.pw) return;
  inp.dataset.pw = "1";
  const w = document.createElement("span");
  w.className = "pw-wrap";
  inp.replaceWith(w);
  w.append(inp);
  w.insertAdjacentHTML("beforeend", `<span class="pw-caps" title="Caps Lock is on">${sf("capslock.fill")}</span><button type="button" class="pw-eye" title="Show password" tabindex="-1">${sf("eye")}</button>`);
  const eye = w.querySelector(".pw-eye");
  eye.onclick = () => {
    const show = inp.type === "password";
    inp.type = show ? "text" : "password";
    eye.innerHTML = sf(show ? "eye.slash" : "eye");
    eye.title = show ? "Hide password" : "Show password";
    inp.focus();
  };
  const caps = (ev) => ev.getModifierState && w.classList.toggle("caps", ev.getModifierState("CapsLock"));
  inp.addEventListener("keydown", caps);
  inp.addEventListener("keyup", caps);
  inp.addEventListener("focus", () => {});
  inp.addEventListener("blur", () => w.classList.remove("caps"));
}
$$('input[type="password"]').forEach(enhancePw);
new MutationObserver(() => $$('input[type="password"]:not([data-pw])').forEach(enhancePw)).observe(document.body, { childList: true, subtree: true });


// ---------- sidebar: right-click a section for its actions ----------
$$(".nav[data-view]").forEach(
  (n) =>
    (n.oncontextmenu = (ev) => {
      ev.preventDefault();
      ev.stopPropagation();
      const view = n.dataset.view;
      const go = (fn) => () => (n.click(), setTimeout(fn, 30));
      const items = $$(`#view-${view} header.bar button[id]`)
        .filter((b) => !b.classList.contains("seg-btn") && b.id !== "side-toggle")
        .map((b) => [(b.textContent.trim() || b.title || b.id).replace(/\s+\d+$/, ""), b.dataset.sf || "chevron.right", go(() => b.click())]);
      if (view === "devices") items.push("-", ["Guide: Clutch", "book", go(() => showGuide("clutch"))], ["Guide: bagbak + Frida", "book", go(() => showGuide("bagbak"))]);
      if (!items.length) items.push(["Open " + n.textContent.trim(), "chevron.right", () => n.click()]);
      fsMenu(ev.clientX, ev.clientY, items);
    })
);


// ---------- hot-plug: usbmuxd watcher in the backend ----------
listen("devices-changed", async (udids) => {
  const before = new Map((S.devices || []).map((d) => [d.udid, d.name]));
  await loadDevices().catch(() => {});
  for (const d of S.devices) if (!before.has(d.udid)) toast(`Connected: ${d.name}`);
  for (const [u, name] of before) if (!udids.includes(u)) toast(`Disconnected: ${name}`);
  if (typeof renderTools === "function") renderTools();
});


// ---------- App Store: country, supported devices, App ID input ----------
const STORE_COUNTRIES = ["au", "at", "be", "br", "ca", "cn", "hr", "cz", "dk", "fi", "fr", "de", "gr", "hk", "hu", "in", "id", "ie", "il", "it", "jp", "kr", "my", "mx", "nl", "nz", "ng", "no", "pk", "ph", "pl", "pt", "ro", "ru", "sa", "sg", "za", "es", "se", "ch", "tw", "th", "tr", "ua", "ae", "gb", "us", "vn"];
function countryName(c) {
  try {
    return new Intl.DisplayNames(["en"], { type: "region" }).of(String(c).toUpperCase()) || c;
  } catch {
    return String(c).toUpperCase();
  }
}
S.country = ""; // always the account's own store

/// "id123", links with ?l=… etc. → the bare id or a clean apps.apple.com link.
function cleanAppInput(v) {
  const t = v.trim();
  const bare = t.match(/^id(\d+)$/i);
  if (bare) return bare[1];
  if (/^https?:\/\//i.test(t)) return t.replace(/[?#].*$/, "");
  return v;
}

function devicesHtml(l) {
  const devs = l.supportedDevices || [];
  if (!devs.length) return "";
  const fam = (p) => (p.startsWith("iPhone") ? "iPhone" : p.startsWith("iPad") ? "iPad" : p.startsWith("iPod") ? "iPod touch" : p.startsWith("AppleTV") ? "Apple TV" : p.startsWith("Watch") ? "Apple Watch" : p.startsWith("Mac") ? "Mac" : p.startsWith("Vision") || p.startsWith("RealityDevice") ? "Apple Vision" : "Other");
  const groups = {};
  devs.forEach((d) => (groups[fam(d)] ||= []).push(d.replace(/-/g, " ")));
  return `<details class="box"><summary>${sf("ipad.and.iphone")} Supported devices · ${Object.keys(groups).join(", ")}</summary>
    ${Object.entries(groups).map(([k, v]) => `<p><b>${esc(k)}</b> <span class="sub">(${v.length})</span><br><span class="sub">${esc(v.join(", "))}</span></p>`).join("")}</details>`;
}
document.addEventListener("click", (e) => {
  const a = e.target.closest?.("[data-open]");
  if (a) (e.preventDefault(), invoke("open_url", { url: a.dataset.open }));
});


// ---------- one app, several platforms ----------
// Universal purchases share one App ID across iPhone/iPad, Mac, Apple TV, Apple
// Watch and Apple Vision; each platform has its own versions and downloads.
// The app page shows which platforms it has and switches the store to one.
const STORE_PLATS = [["", "iPhone & iPad", "iphone"], ["macos", "Mac", "desktopcomputer"], ["appletv", "Apple TV", "appletv"], ["watchos", "Apple Watch", "clock"], ["visionos", "Apple Vision", "eye"]];
function appPlatforms(app, l) {
  const have = new Set();
  const p = app.platforms || [];
  const devs = l?.supportedDevices || [];
  if (p.includes("iphone") || p.includes("ipad") || devs.some((d) => /^(iPhone|iPad|iPod)/.test(d))) have.add("");
  if (p.includes("macos") || l?.kind === "mac-software" || devs.some((d) => /^Mac/.test(d))) have.add("macos");
  if (p.includes("appletv") || devs.some((d) => /^AppleTV/.test(d))) have.add("appletv");
  if (p.includes("watchos") || devs.some((d) => /^Watch/.test(d))) have.add("watchos");
  if (p.includes("visionos") || devs.some((d) => /^(Vision|RealityDevice)/.test(d))) have.add("visionos");
  have.add(S.platform || "");
  return have;
}
async function renderAppPlatforms(app, l) {
  const el = $("#app-plats");
  if (!el) return;
  const have = appPlatforms(app, l);
  const req = await invoke("app_platforms", { appId: app.id, country: l?.country || S.country || null }).catch(() => ({}));
  if (S.selected !== app) return;
  const map = { ios: "", macos: "macos", tvos: "appletv", watchos: "watchos", visionos: "visionos" };
  const ver = {};
  for (const [k, v] of Object.entries(req || {})) if (k in map) (have.add(map[k]), (ver[map[k]] = v));
  if (have.size < 2) return (el.innerHTML = "");
  const osName = { "": "iOS", macos: "macOS", appletv: "tvOS", watchos: "watchOS", visionos: "visionOS" };
  el.innerHTML = `<span class="sub">Platforms:</span> ` + STORE_PLATS.filter(([k]) => have.has(k))
    .map(([k, label, icon]) => `<button class="btn small ${k === (S.platform || "") ? "primary" : ""}" data-plat="${k}" title="Show this app's ${label} versions${ver[k] ? ` (requires ${osName[k]} ${esc(ver[k])})` : ""}">${sf(icon)}${label}</button>`)
    .join("");
  if ((S.platform || "") === "watchos")
    el.insertAdjacentHTML(
      "beforeend",
      `<div class="sub wrap" style="flex-basis:100%">Apple Watch: most Watch apps aren't sold separately. They ship inside the iPhone app (Payload/…app/Watch/), so the download is the iPhone IPA that carries the Watch app; versions without a Watch app can't be downloaded here.</div>`
    );
  $$("[data-plat]", el).forEach(
    (b) =>
      (b.onclick = async () => {
        const k = b.dataset.plat;
        if (k === (S.platform || "")) return;
        S.platform = k;
        S.forcePlat = { id: app.id, plat: k };
        if (S.view !== "all") $("#lib-platform").value = k;
        await invoke("store_set_platform", { platform: k });
        toast(`Store: ${STORE_PLATS.find((x) => x[0] === k)[1]}`);
        selectApp(app);
      })
  );
}

// ---------- About: version + release date ----------
invoke("app_info")
  .then((i) => i && $("#about-ver") && ($("#about-ver").textContent = `Version ${i.version} · released ${fmtDate(i.released)}`))
  .catch(() => {});


// ---------- Library ⇄ Archive ----------
function libMenu(ev, a) {
  if (!a) return;
  ev.preventDefault();
  ev.stopPropagation();
  const arc = S.archive.filter((e) => e.appId === a.id || (a.bundleID && e.bundleId === a.bundleID));
  const manual = a.manual || a.origin === "Added manually";
  fsMenu(ev.clientX, ev.clientY, [
    ["Open", "info.circle", () => selectApp(a)],
    arc.length && ["Show in Archive", "archivebox", () => showInArchive(a.bundleID || arc[0].bundleId)],
    ["Get license", "cart", () => buyApp(a)],
    ["App Store page", "app.badge", async () => {
      const l = await lookup(a.id);
      l?.trackViewUrl ? invoke("open_url", { url: l.trackViewUrl }) : fail("App Store page not found (the app may have been removed)");
    }],
    "-",
    ["Copy App ID", "doc.on.doc", () => navigator.clipboard.writeText(String(a.id)).then(() => toast("Copied: " + a.id))],
    a.bundleID && ["Copy bundle ID", "doc.on.doc", () => navigator.clipboard.writeText(a.bundleID).then(() => toast("Copied: " + a.bundleID))],
    manual && "-",
    manual && ["Remove from list", "trash", async () => {
      await invoke("manual_app_remove", { id: a.id }).catch(fail);
      if (S.selected?.id === a.id) (S.selected = null), ($("#lib-detail").innerHTML = `<div class="empty">Select an app to see its versions.</div>`);
      toast(`${a.name} removed from the list`);
      loadOwned();
    }, true],
  ]);
}

function showInArchive(bundleId) {
  go("archive");
  $("#arc-filter").value = bundleId;
  S.arcOpen.add(bundleId);
  renderArchive();
}

async function showInLibrary(e) {
  go("library");
  const owned = (S.owned || []).find((a) => (e.appId && a.id === e.appId) || a.bundleID === e.bundleId);
  if (owned) return selectApp(owned);
  if (!e.appId) return fail("This archive entry has no App Store ID");
  selectApp({ id: e.appId, bundleID: e.bundleId, name: e.storeName || e.name, version: e.version });
}

// Manually added apps saved before their store info could be found: look them up again.
async function refreshManualNames() {
  const list = (await invoke("manual_apps").catch(() => [])) || [];
  const stale = list.filter((a) => /^App \d+$/.test(a.name || ""));
  let changed = false;
  for (const a of stale) {
    const n = await invoke("manual_app_add", { input: String(a.id) }).catch(() => null);
    if (n && !/^App \d+$/.test(n.name)) changed = true;
  }
  if (changed) loadOwned();
}
setTimeout(refreshManualNames, 4000);


// ---------- Esc: close whatever is open (never leave full screen) ----------
window.addEventListener(
  "keydown",
  (e) => {
    if (e.key !== "Escape") return;
    // macOS would otherwise take Esc to leave full screen.
    e.preventDefault();
    const done = () => e.stopImmediatePropagation();
    const ctx = $("#ctx");
    if (ctx) return ctx.remove(), done();
    const ed = $(".lang-ed");
    if (ed) return ed.remove(), done();
    const dlg = [...document.querySelectorAll("dialog[open]")].pop();
    if (dlg) return dlg.close(), done();
    const panel = ["#dl-panel", "#dump-panel", "#log-panel"].map((x) => $(x)).find((p) => p && !p.classList.contains("hidden"));
    if (panel) return panel.classList.add("hidden"), done();
    const a = document.activeElement;
    if (a?.matches?.("input, textarea") && a.value && a.classList.contains("search")) {
      a.value = "";
      a.dispatchEvent(new Event("input"));
      return done();
    }
    if (a?.matches?.("input, textarea, select")) return a.blur(), done();
    if ($(".view.active")?.id === "view-devices" && $("#dev-dump") && !$("#dev-dump").classList.contains("hidden")) return $("#dev-dump").classList.add("hidden"), done();
    if (S.sel) return (S.sel = null), renderArchive(), done();
    if ($(".view.active")?.id === "view-library" && S.src === "artist") return (S.src = S.prevSrc || "owned"), renderLibList(), done();
    if ($(".view.active")?.id === "view-library" && S.selected) {
      S.selected = null;
      $$("#lib-list .row").forEach((r) => r.classList.remove("active"));
      $("#lib-detail").innerHTML = `<div class="empty">Select an app to see its versions.</div>`;
      return done();
    }
  },
  true
);


// ---------- top bar: device, downloads, dumps, refresh, archive folder ----------
(() => {
  const bar = $(".drag.top");
  if (!bar) return;
  bar.insertAdjacentHTML(
    "beforeend",
    `<div class="topbar">
      <button class="tb" id="tb-dev" title="Connected device (Devices)">${sf("iphone")}<span>No device</span></button>
      <button class="tb" id="tb-dl" title="Downloads">${sf("arrow.down.circle")}<span class="tbn"></span></button>
      <button class="tb" id="tb-dump" title="Dump queue">${sf("key.fill")}<span class="tbn"></span></button>
      <button class="tb" id="tb-refresh" title="Refresh this screen (⌘R)">${sf("arrow.clockwise")}</button>
      <button class="tb" id="tb-folder" title="Open the archive folder">${sf("folder")}</button>
      <button class="tb" id="tb-search" title="Search this screen (⌘F)">${sf("magnifyingglass")}</button>
    </div>`
  );
  const view = () => $(".view.active")?.id?.replace("view-", "");
  $("#tb-dev").onclick = () => go("devices");
  $("#tb-dl").onclick = () => (go("library"), $("#lib-downloads")?.click());
  $("#tb-dump").onclick = () => (view() === "devices" ? $("#dev-dumps") : (go("archive"), $("#arc-dumps")))?.click();
  const refresh = () => {
    const v = view();
    if (v === "library") $("#lib-refresh")?.click();
    else if (v === "devices") $("#dev-refresh")?.click();
    else if (v === "archive") invoke("archive_rescan").catch(() => {}).finally(renderArchive);
    else if (v === "blobs") typeof loadBlobs === "function" && loadBlobs();
  };
  $("#tb-refresh").onclick = refresh;
  $("#tb-folder").onclick = async () => invoke("open_path", { path: await invoke("archive_root") });
  $("#tb-search").onclick = () => {
    const f = $(".view.active .bar .search") || $(".view.active .search");
    f && (f.focus(), f.select());
  };
  const paint = () => {
    const d = S.devices?.[0];
    $("#tb-dev span").textContent = d ? d.name : "No device";
    $("#tb-dev").classList.toggle("on", !!d);
    const dl = (S.dls || []).filter((x) => ["queued", "running", "analyzing", "paused"].includes(x.state)).length;
    const dm = (S.dumps || []).filter((x) => !["done", "error"].includes(x.state)).length;
    $("#tb-dl .tbn").textContent = dl || "";
    $("#tb-dump .tbn").textContent = dm || "";
  };
  setInterval(paint, 1500);
  paint();
  // ⌘1–5: sections · ⌘R: refresh
  document.addEventListener("keydown", (e) => {
    if (!(e.metaKey || e.ctrlKey) || e.altKey) return;
    const views = ["library", "archive", "devices", "blobs", "settings"];
    const n = parseInt(e.key, 10);
    if (n >= 1 && n <= 5) return e.preventDefault(), go(views[n - 1]);
    if (e.key.toLowerCase() === "r") return e.preventDefault(), refresh();
  });
})();
