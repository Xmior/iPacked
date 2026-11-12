// Language: the UI is written in English. Other languages are dictionaries
// "<English phrase>" → "<translation>" loaded from
// ~/Library/Application Support/iPacked/lang/<code>.json (Settings → Language).
// Turkish is built in (lang-tr.js); a user's lang/tr.json overrides its entries.
// Phrases in text nodes, titles and placeholders are replaced on the fly
// (longest first, whole words), so new DOM is covered too.
const LANG_KEY = "ipacked.lang2";
const DICT_KEY = "ipacked.langdict2";
const MISS_KEY = "ipacked.langmissing";
const ls = {
  get: (k, d) => {
    try {
      const v = localStorage.getItem(k);
      return v == null ? d : v;
    } catch {
      return d;
    }
  },
  set: (k, v) => {
    try {
      localStorage.setItem(k, v);
    } catch {}
  },
};
window.LANG = ls.get(LANG_KEY, "en");

const EN = {};

const BUILTIN = { en: { _name: "English" } };
if (window.IPACKED_LANG_TR) BUILTIN.tr = window.IPACKED_LANG_TR;
// Proper names that contain translatable words ("Archive" in "Archive.org") stay as they are.
const PROTECT = ["Archive.org", "App Store", "Mac App Store", "Apple ID", "Apple TV", "Apple Watch", "Apple Vision", "Legacy iOS Kit", "iTunes File Sharing", "Legacy-iOS-Kit"];
// Files saved by this version carry _v: 2. Older tr.json files were Turkish → English
// tables (the old source language) and are ignored.
const LANG_FILE_V = 2;
const clean = (d) => Object.fromEntries(Object.entries(d || {}).filter(([k, v]) => !k.startsWith("_") && typeof v === "string" && v && v !== k));

// Start synchronously from the last loaded dictionary (files are read async).
let DICT = {};
try {
  DICT = JSON.parse(ls.get(DICT_KEY, "{}"));
} catch {}
if (window.LANG === "en") DICT = {};
else if (!Object.keys(DICT).length) DICT = clean(BUILTIN[window.LANG]);

const escRe = (s) => s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
let DICT_RE = null;
function buildRe() {
  const keys = Object.keys(DICT).length ? [...new Set([...Object.keys(DICT), ...PROTECT])].sort((a, b) => b.length - a.length) : [];
  // Whole words only ("Ad" must not hit "Adım").
  DICT_RE = keys.length ? new RegExp("(?<![\\p{L}\\p{N}])(?:" + keys.map(escRe).join("|") + ")(?![\\p{L}\\p{N}])", "gu") : null;
}
buildRe();

// Turkish text still on screen in another language → offered in the editor.
const MISSING = new Set(JSON.parse(ls.get(MISS_KEY, "[]")));
let missSave = 0;
const TR_CH = /[çğıöşüÇĞİÖŞÜ]/;
function noteMissing(src, out) {
  return; // English source: untranslated phrases are listed from the built-in table instead.
  const t = src.trim();
  if (t.length < 2 || t.length > 300 || MISSING.has(t)) return;
  MISSING.add(t);
  clearTimeout(missSave);
  missSave = setTimeout(() => ls.set(MISS_KEY, JSON.stringify([...MISSING].slice(-2000))), 1000);
}
const tr2en = (s) => {
  if (!DICT_RE || !s || !/[^\x00-\x7F]|[A-Za-z]/.test(s)) return s;
  const out = s.replace(DICT_RE, (m) => DICT[m] ?? m);
  noteMissing(s, out);
  return out;
};

function translateNode(root) {
  if (!DICT_RE) return;
  const walk = document.createTreeWalker(root, NodeFilter.SHOW_TEXT);
  let n;
  while ((n = walk.nextNode())) {
    if (n.parentElement && /^(SCRIPT|STYLE|PRE|TEXTAREA)$/.test(n.parentElement.tagName)) continue;
    if (n.parentElement?.closest(".no-tr")) continue;
    const t = tr2en(n.nodeValue);
    if (t !== n.nodeValue) n.nodeValue = t;
  }
  const els = root.querySelectorAll ? root.querySelectorAll("[title],[placeholder]") : [];
  for (const el of [root, ...els]) {
    if (!el.getAttribute || el.closest?.(".no-tr")) continue;
    for (const a of ["title", "placeholder"]) {
      const v = el.getAttribute(a);
      if (v) {
        const t = tr2en(v);
        if (t !== v) el.setAttribute(a, t);
      }
    }
  }
}

let observing = false;
function startTranslating() {
  if (!DICT_RE || observing) return;
  observing = true;
  if (window.LANG !== "tr") document.documentElement.lang = window.LANG;
  translateNode(document.body);
  new MutationObserver((ms) => {
    for (const m of ms) {
      if (m.type === "characterData") {
        if (m.target.parentElement?.closest(".no-tr")) continue;
        const t = tr2en(m.target.nodeValue);
        if (t !== m.target.nodeValue) m.target.nodeValue = t;
      } else m.addedNodes.forEach((x) => (x.nodeType === 3 ? !x.parentElement?.closest(".no-tr") && (x.nodeValue = tr2en(x.nodeValue)) : x.nodeType === 1 && translateNode(x)));
    }
  }).observe(document.body, { childList: true, subtree: true, characterData: true });
}
startTranslating();

function setLang(l) {
  ls.set(LANG_KEY, l);
  ls.set(DICT_KEY, "{}");
  location.reload();
}

// ---- language files ----
const LANG_FILES = { data: {} };
const langInvoke = (c, a) => window.__TAURI__.core.invoke(c, a);
// Every phrase the built-in translations know (English source text).
const allKeys = () => [...new Set(Object.values(BUILTIN).flatMap((d) => Object.keys(d).filter((k) => !k.startsWith("_"))))];

async function loadLangFiles() {
  let files = {};
  try {
    files = (await langInvoke("lang_all")) || {};
  } catch {
    return;
  }
  // Keep the built-in files complete (new phrases get added; user edits stay).
  // English is the source; the old built-in Turkish files are not used any more.
  delete files.en;
  delete files._template;
  for (const c of Object.keys(files)) if (BUILTIN[c] && files[c]?._v !== LANG_FILE_V) delete files[c];
  langInvoke("lang_write", { code: "_template", data: { _name: "New language name", "English phrase": "Translation" } }).catch(() => {});
  if (!files[window.LANG] && !BUILTIN[window.LANG]) window.LANG = "en";
  LANG_FILES.data = files;
  fillLangSelect();
  const next = window.LANG === "en" ? {} : { ...clean(BUILTIN[window.LANG]), ...clean(files[window.LANG]) };
  if (JSON.stringify(next) !== JSON.stringify(DICT)) {
    const first = !DICT_RE;
    ls.set(DICT_KEY, JSON.stringify(next));
    DICT = next;
    buildRe();
    if (first) startTranslating();
    else if (!sessionStorage.getItem("ipacked.langreload")) {
      sessionStorage.setItem("ipacked.langreload", "1");
      location.reload();
    }
  }
}

function langName(code) {
  return LANG_FILES.data[code]?._name || BUILTIN[code]?._name || code;
}

function fillLangSelect() {
  const sel = document.getElementById("lang-select");
  if (!sel) return;
  const codes = [...new Set([...Object.keys(BUILTIN), ...Object.keys(LANG_FILES.data)])];
  sel.innerHTML = codes.map((c) => `<option value="${c}">${langName(c).replace(/</g, "&lt;")} (${c})</option>`).join("");
  sel.value = window.LANG;
  sel.onchange = () => setLang(sel.value);
}
fillLangSelect();
if (window.__TAURI__) loadLangFiles();

// ---- editor (Settings → Language → Edit texts) ----
async function langEditor(code = window.LANG) {
  const ed = document.createElement("div");
  ed.className = "lang-ed no-tr";
  const data = { ...(LANG_FILES.data[code] || { _name: langName(code) }) };
  const base = BUILTIN[code] || {};
  const h = (s) => String(s).replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/"/g, "&quot;");
  ed.innerHTML = `<div class="lang-ed-box">
    <header><b>Edit texts</b>
      <select id="le-code">${[...new Set([...Object.keys(BUILTIN), ...Object.keys(LANG_FILES.data)])].map((c) => `<option value="${c}" ${c === code ? "selected" : ""}>${h(langName(c))} (${c})</option>`).join("")}<option value="+">+ New language…</option></select>
      <input id="le-name" placeholder="Language name" value="${h(data._name || "")}">
      <input id="le-q" placeholder="Search…" type="search">
      <label><input type="checkbox" id="le-empty"> Only empty</label>
      <span class="sp"></span>
      <button class="btn" id="le-folder">Open folder</button>
      <button class="btn" id="le-close">Close</button>
      <button class="btn primary" id="le-save">Save</button>
    </header>
    <p class="sub">Left: English source text. Right: translation${code in BUILTIN && code !== "en" ? " (empty = built-in translation)" : " (empty = stays English)"}. File: lang/${h(code)}.json</p>
    <div class="lang-ed-list"></div></div>`;
  document.body.append(ed);
  const list = ed.querySelector(".lang-ed-list");
  const keys = [...new Set([...allKeys(), ...Object.keys(data).filter((k) => !k.startsWith("_"))])];
  const render = () => {
    const q = ed.querySelector("#le-q").value.toLowerCase();
    const onlyEmpty = ed.querySelector("#le-empty").checked;
    const rows = keys.filter((k) => (!q || k.toLowerCase().includes(q) || String(data[k] || "").toLowerCase().includes(q)) && (!onlyEmpty || !data[k]));
    list.innerHTML = rows
      .slice(0, 600)
      .map((k) => `<div class="le-row"><div class="le-src">${h(k)}</div><textarea rows="1" data-k="${h(k)}" placeholder="${h(base[k] || "")}">${h(data[k] || base[k] || "")}</textarea></div>`)
      .join("") + (rows.length > 600 ? `<div class="sub">… ${rows.length - 600} more rows (narrow with search)</div>` : "");
  };
  list.oninput = (e) => {
    const k = e.target.dataset?.k;
    if (k != null) data[k] = e.target.value;
  };
  ed.querySelector("#le-q").oninput = render;
  ed.querySelector("#le-empty").onchange = render;
  ed.querySelector("#le-close").onclick = () => ed.remove();
  ed.querySelector("#le-folder").onclick = () => langInvoke("lang_reveal");
  ed.querySelector("#le-code").onchange = async (e) => {
    let c = e.target.value;
    if (c === "+") {
      // WKWebView has no prompt(): ask inline.
      e.target.value = code;
      const box = document.createElement("div");
      box.className = "le-new";
      box.innerHTML = `<input id="le-nc" placeholder="Code (e.g. de, fr, az)" maxlength="16"><input id="le-nn" placeholder="Language name (e.g. Deutsch)"><button class="btn primary" id="le-nok">Create</button><span class="sub" id="le-nerr"></span>`;
      ed.querySelector("header").after(box);
      box.querySelector("#le-nc").focus();
      box.querySelector("#le-nok").onclick = async () => {
        const nc = box.querySelector("#le-nc").value.trim().toLowerCase();
        if (!/^[a-z0-9_-]{1,16}$/.test(nc)) return (box.querySelector("#le-nerr").textContent = "Code: letters/digits, at most 16 characters");
        const name = box.querySelector("#le-nn").value.trim() || nc;
        LANG_FILES.data[nc] = { _name: name, _v: LANG_FILE_V, ...Object.fromEntries(allKeys().map((k) => [k, ""])), ...(LANG_FILES.data[nc] || {}) };
        await langInvoke("lang_write", { code: nc, data: LANG_FILES.data[nc] });
        fillLangSelect();
        ed.remove();
        langEditor(nc);
      };
      return;
    }
    ed.remove();
    langEditor(c);
  };
  ed.querySelector("#le-save").onclick = async () => {
    data._name = ed.querySelector("#le-name").value.trim() || code;
    // Keep only real edits: text equal to the built-in translation is left to the built-in.
    const out = { _name: data._name, _v: LANG_FILE_V, ...Object.fromEntries(keys.map((k) => [k, data[k] && data[k] !== base[k] ? data[k] : ""])) };
    try {
      await langInvoke("lang_write", { code, data: out });
    } catch (x) {
      return (ed.querySelector(".sub").textContent = "Could not save: " + x);
    }
    LANG_FILES.data[code] = out;
    if (code === window.LANG) setLang(code);
    else ed.remove(), fillLangSelect();
  };
  render();
}
{
  const b = document.getElementById("lang-edit");
  if (b) b.onclick = () => langEditor();
  const f = document.getElementById("lang-folder");
  if (f) f.onclick = () => langInvoke("lang_reveal");
}
