/**
 * Launcher — the Start menu's replacement.
 *
 * Opened by the Win key or the dock's Start button, closed by either again, by
 * Escape, or by clicking away. Rust owns the Win key (win/hotkeys.rs explains why
 * only its *release* is ever touched); this owns everything on screen.
 *
 * Three things it shows, in one grid: installed apps, the contents of a folder from
 * the sidebar, and search results. Typing searches apps instantly — they are already
 * in memory — and files in the user's folders through a bounded walk in Rust.
 */

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import "../shared/theme.css";
import { followTint } from "../shared/tint";
import "./launcher.css";

interface App {
  name: string;
  parsingName: string;
  icon: string | null;
}

interface FileEntry {
  name: string;
  path: string;
  isDir: boolean;
}

/** One row in the grid, whatever it came from. */
interface Row {
  name: string;
  /** What to hand the shell to open it. */
  target: string;
  /** A real path, when there is one, for "Open in File Explorer". */
  path?: string;
  isDir?: boolean;
  icon?: string | null;
}

const $ = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;

const FOLDER_SVG = `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linejoin="round"><path d="M3 7.5A1.5 1.5 0 0 1 4.5 6h4L11 8.5h8.5A1.5 1.5 0 0 1 21 10v7.5a1.5 1.5 0 0 1-1.5 1.5h-15A1.5 1.5 0 0 1 3 17.5z"/></svg>`;
const FILE_SVG = `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linejoin="round"><path d="M6 3.5h7L19 9v11.5H6z"/><path d="M13 3.5V9h6"/></svg>`;

const gridEl = $<HTMLDivElement>("grid");
const searchEl = $<HTMLInputElement>("search");

let apps: App[] = [];
let rows: Row[] = [];
let selected = 0;
/** Null means "the app grid"; otherwise the folder being browsed. */
let folder: string | null = null;
let folderLabel = "";

/* ------------------------------------------------------------------ render */

function iconFor(row: Row): string {
  return row.isDir ? FOLDER_SVG : FILE_SVG;
}

function paintIcon(plate: HTMLElement, row: Row): void {
  if (!row.icon) {
    plate.innerHTML = iconFor(row);
    return;
  }
  const img = document.createElement("img");
  img.src = row.icon;
  img.alt = "";
  plate.replaceChildren(img);
}

function render(title: string): void {
  $("section-title").textContent = title;
  $<HTMLElement>("empty").hidden = rows.length > 0;
  gridEl.textContent = "";

  rows.forEach((row, i) => {
    const el = document.createElement("div");
    el.className = "item";
    el.setAttribute("role", "option");
    el.dataset.i = String(i);

    const plate = document.createElement("div");
    plate.className = "plate";
    paintIcon(plate, row);

    const label = document.createElement("span");
    label.textContent = row.name;

    el.append(plate, label);
    gridEl.appendChild(el);
  });

  paintSelection();
}

function paintSelection(): void {
  const items = [...gridEl.children] as HTMLElement[];
  items.forEach((el, i) => el.classList.toggle("on", i === selected));
  items[selected]?.scrollIntoView({ block: "nearest" });
}

/* ----------------------------------------------------------------- sources */

function showApps(): void {
  folder = null;
  rows = apps.map((a) => ({ name: a.name, target: a.parsingName, icon: a.icon }));
  selected = 0;
  render("All apps");
  markSide(null);
}

async function showFolder(path: string, label: string): Promise<void> {
  folder = path;
  folderLabel = label;
  const entries = await invoke<FileEntry[]>("cmd_launcher_list_dir", { path });
  rows = entries.map((e) => ({ name: e.name, target: e.path, path: e.path, isDir: e.isDir }));
  selected = 0;
  render(label);
  markSide(path);
}

/** Apps first, then files — the order Windows Start search uses. */
async function showSearch(query: string): Promise<void> {
  const q = query.toLowerCase();
  const hits = apps.filter((a) => a.name.toLowerCase().includes(q));
  // Names that *start* with the query are what you meant far more often.
  hits.sort((a, b) => {
    const as = a.name.toLowerCase().startsWith(q) ? 0 : 1;
    const bs = b.name.toLowerCase().startsWith(q) ? 0 : 1;
    return as - bs || a.name.localeCompare(b.name);
  });

  rows = hits.map((a) => ({ name: a.name, target: a.parsingName, icon: a.icon }));
  selected = 0;
  render(`Results for "${query}"`);
  markSide(null);

  // Files take a moment; fold them in when they arrive, as long as the query has
  // not moved on in the meantime.
  const files = await invoke<FileEntry[]>("cmd_launcher_search", { query });
  if (searchEl.value.trim() !== query) return;
  rows = rows.concat(
    files.map((f) => ({ name: f.name, target: f.path, path: f.path, isDir: f.isDir })),
  );
  render(`Results for "${query}"`);
}

/* ---------------------------------------------------------------- icons */

const GLYPH = (d: string) =>
  `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">${d}</svg>`;

const SEARCH_ICON = GLYPH(`<circle cx="11" cy="11" r="6.5"/><path d="m16 16 4.5 4.5"/>`);
const APPS_ICON = GLYPH(`<rect x="3.5" y="3.5" width="7" height="7" rx="1.8"/><rect x="13.5" y="3.5" width="7" height="7" rx="1.8"/><rect x="3.5" y="13.5" width="7" height="7" rx="1.8"/><rect x="13.5" y="13.5" width="7" height="7" rx="1.8"/>`);
const FOLDER_ICON = GLYPH(`<path d="M3 7.5a2 2 0 0 1 2-2h4l2 2.2h8a2 2 0 0 1 2 2v7.8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/>`);
const POWER_ICON = GLYPH(`<path d="M12 3.5v8"/><path d="M6.8 6.6a7.5 7.5 0 1 0 10.4 0"/>`);

/* -------------------------------------------------------------- sidebar */

function markSide(path: string | null): void {
  for (const b of document.querySelectorAll<HTMLButtonElement>("#side button")) {
    b.classList.toggle("on", (b.dataset.path ?? null) === path);
  }
}

async function buildSidebar(): Promise<void> {
  const side = $("side");
  side.textContent = "";

  const header = (text: string) => {
    const h = document.createElement("div");
    h.className = "side-head";
    h.textContent = text;
    side.appendChild(h);
  };
  const entry = (label: string, path: string | null) => {
    const b = document.createElement("button");
    b.type = "button";
    b.innerHTML = `<span class="side-icon">${path ? FOLDER_ICON : APPS_ICON}</span><span>${label}</span>`;
    if (path) b.dataset.path = path;
    b.addEventListener("click", () => (path ? void showFolder(path, label) : showApps()));
    side.appendChild(b);
  };

  header("Library");
  entry("Apps", null);

  header("Folders");
  for (const [name, path] of await invoke<[string, string][]>("cmd_launcher_folders")) {
    entry(name, path);
  }
}

/* --------------------------------------------------------------- footer */

/**
 * The account row and the power menu.
 *
 * The four power actions used to sit in the footer as loose text buttons, which put
 * "Shut down" one stray click from "Sleep". They are now behind the power glyph, the
 * way Windows' own Start menu has them - the same four actions, one deliberate step
 * further away.
 */
function wireFooter(): void {
  $("search-icon").innerHTML = SEARCH_ICON;
  $("power-btn").innerHTML = POWER_ICON;

  void invoke<string>("cmd_user_name")
    .then((name) => {
      if (!name) return;
      $("who-name").textContent = name;
      $("avatar").textContent = name.trim().charAt(0).toUpperCase();
    })
    .catch(() => {
      /* the row just stays empty */
    });

  const btn = $<HTMLButtonElement>("power-btn");
  const menu = $<HTMLElement>("power-menu");
  const setOpen = (open: boolean) => {
    menu.hidden = !open;
    btn.setAttribute("aria-expanded", String(open));
  };

  btn.addEventListener("click", (e) => {
    e.stopPropagation();
    setOpen(menu.hidden);
  });
  // Anywhere else in the launcher closes it, the way a menu should.
  document.addEventListener("click", () => setOpen(false));
  document.addEventListener("keydown", (e) => {
    if (e.key === "Escape" && !menu.hidden) {
      e.stopPropagation();
      setOpen(false);
    }
  });

  for (const b of menu.querySelectorAll<HTMLButtonElement>("[data-power]")) {
    b.addEventListener("click", () =>
      void invoke("cmd_launcher_power", { action: b.dataset.power }),
    );
  }
}

/* -------------------------------------------------------------- actions */

function openRow(row: Row | undefined): void {
  if (!row) return;
  // A folder in the results opens inside the launcher; everything else is handed
  // to the shell, exactly as double-clicking it in Explorer would be.
  if (row.isDir && row.path) {
    searchEl.value = "";
    void showFolder(row.path, row.name);
    return;
  }
  void invoke("cmd_launcher_open", { target: row.target });
}

/* -------------------------------------------------------------- startup */

function applyPrefs(iconShape: string, theme: string): void {
  const root = document.documentElement;
  root.dataset.iconShape = iconShape || "circle";
  if (theme === "light" || theme === "dark") root.dataset.theme = theme;
  else delete root.dataset.theme;
}

async function applyGlass(): Promise<void> {
  const forced = document.documentElement.dataset.theme;
  const dark = forced
    ? forced === "dark"
    : window.matchMedia("(prefers-color-scheme: dark)").matches;
  try {
    if (await invoke<boolean>("cmd_glass", { dark })) {
      document.documentElement.dataset.acrylic = "on";
    }
  } catch {
    /* painted fallback */
  }
}

async function main(): Promise<void> {
  // Glass tint first, so nothing is drawn in the wrong colour.
  await followTint(() => void applyGlass());
  try {
    const p = await invoke<{ iconShape: string; theme: string }>("cmd_system_prefs");
    applyPrefs(p.iconShape, p.theme);
  } catch {
    /* CSS defaults are fine */
  }
  await applyGlass();
  await buildSidebar();

  // Deliberately NOT fetched here. This page exists from startup so that the Win
  // key opens instantly, but the app list carries an icon per app - a couple of
  // hundred of them - and holding that decoded in a page nobody has opened yet is
  // idle memory spent on nothing. It is fetched the first time the launcher opens.
  showApps();

  searchEl.addEventListener("input", () => {
    const q = searchEl.value.trim();
    if (!q) {
      if (folder === null) showApps();
      else void showFolder(folder, folderLabel);
      return;
    }
    void showSearch(q);
  });

  gridEl.addEventListener("click", (e) => {
    const el = (e.target as HTMLElement).closest(".item") as HTMLElement | null;
    if (!el) return;
    selected = Number(el.dataset.i);
    paintSelection();
    openRow(rows[selected]);
  });

  gridEl.addEventListener("contextmenu", (e) => {
    e.preventDefault();
    const el = (e.target as HTMLElement).closest(".item") as HTMLElement | null;
    const row = el ? rows[Number(el.dataset.i)] : undefined;
    if (row?.path) void invoke("cmd_launcher_reveal", { path: row.path });
  });

  wireFooter();

  // Arrow keys move through the grid; Enter opens the selection. Escape is also
  // handled by the keyboard hook, so it works even if this window has lost focus.
  document.addEventListener("keydown", (e) => {
    const cols = Math.max(1, Math.floor(gridEl.clientWidth / 108));
    const step =
      e.key === "ArrowRight"
        ? 1
        : e.key === "ArrowLeft"
          ? -1
          : e.key === "ArrowDown"
            ? cols
            : e.key === "ArrowUp"
              ? -cols
              : 0;

    if (step) {
      e.preventDefault();
      selected = Math.min(Math.max(0, selected + step), Math.max(0, rows.length - 1));
      paintSelection();
    } else if (e.key === "Enter") {
      e.preventDefault();
      openRow(rows[selected]);
    } else if (e.key === "Escape") {
      void invoke("cmd_launcher_close");
    }
  });

  // Win while the launcher has focus closes it, as Start does. That is handled in
  // Rust, not here: the guard in win/hotkeys.rs watches the key itself, so it works
  // whichever window has focus and whether or not this page ever sees the key
  // (D-059). Handling it here as well would close the launcher twice - and the
  // second "toggle" would open it again.

  await listen("launcher:open", async () => {
    // Fresh every time, the way Start is: empty search, apps listed, caret ready.
    searchEl.value = "";
    // Re-read the list only while it is still empty. This page is created at
    // startup, in parallel with the background enumeration of installed apps, so
    // the first read can land before that finishes and come back empty - which is
    // exactly what it did. Once it is populated, the icons travelling with it make
    // the payload big enough to be worth not re-sending.
    if (apps.length === 0) apps = await invoke<App[]>("cmd_launcher_apps");
    showApps();
    window.setTimeout(() => searchEl.focus(), 60);
  });

  await getCurrentWindow().hide();
}

void main();
