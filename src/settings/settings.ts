/**
 * Settings window.
 *
 * Every control writes straight through to `settings.json` and the change is applied
 * to the running UI immediately — there is no Apply button and nothing to lose by
 * closing the window. Rust owns the merge (`cmd_patch_settings` takes a partial
 * object), so this file never sends back a whole settings struct and therefore can
 * never clobber something changed elsewhere, such as the pin order after a drag.
 */

import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import "../shared/theme.css";
import { followTint } from "../shared/tint";
import "./settings.css";

type WidgetId = "clock" | "calendar" | "weather" | "media" | "stats";
const WIDGET_IDS: WidgetId[] = ["clock", "calendar", "weather", "media", "stats"];

interface WidgetState {
  on: boolean;
  x: number;
  y: number;
}

interface Settings {
  widgetsEnabled: boolean;
  widgets: Partial<Record<WidgetId, WidgetState>>;
  weatherCity: string;
  weatherLat: number;
  weatherLon: number;
  dockEnabled: boolean;
  topbarEnabled: boolean;
  topbarAutoHide: boolean;
  topbarAppPill: boolean;
  island: boolean;
  switcherEnabled: boolean;
  dockAlwaysVisible: boolean;
  dockEdge: string;
  dockIconSize: number;
  dockMagnification: boolean;
  iconShape: string;
  theme: string;
  animationSpeed: number;
}

interface Config {
  settings: Settings;
  startAtLogin: boolean;
  wallpaperChanged: boolean;
  settingsPath: string;
  version: string;
}

const $ = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;

let config: Config;

/** Send one changed key and let Rust merge, validate and apply it. */
async function patch(key: keyof Settings, value: unknown): Promise<void> {
  (config.settings as unknown as Record<string, unknown>)[key] = value;
  // Appearance choices apply to this window as well, so the pick is visible as it
  // is made rather than only on the dock behind it.
  applyOwnAppearance();
  try {
    await invoke("cmd_patch_settings", { patch: { [key]: value } });
  } catch (e) {
    await invoke("cmd_log", { msg: `settings: saving ${key} failed: ${e}` });
  }
}

/** Which theme this window has actually ended up in, "auto" resolved. */
function isDark(): boolean {
  const t = config.settings.theme;
  if (t === "dark") return true;
  if (t === "light") return false;
  return window.matchMedia("(prefers-color-scheme: dark)").matches;
}

function applyOwnAppearance(): void {
  const root = document.documentElement;
  const theme = config.settings.theme;
  if (theme === "light" || theme === "dark") root.dataset.theme = theme;
  else delete root.dataset.theme;
  root.dataset.iconShape = config.settings.iconShape;
  root.style.setProperty("--anim-scale", String(config.settings.animationSpeed));
  void applyGlass();
}

/**
 * Ask Windows to blur the desktop behind this window.
 *
 * Only marks the page as glass if the call actually succeeded, so a machine where
 * the composition attribute is missing keeps the painted panel and stays readable.
 */
async function applyGlass(): Promise<void> {
  try {
    const ok = await invoke<boolean>("cmd_glass", { dark: isDark() });
    if (ok) document.documentElement.dataset.acrylic = "on";
    else delete document.documentElement.dataset.acrylic;
  } catch {
    /* painted fallback is fine */
  }
}

/* ------------------------------------------------------------- controls */

/** A segmented control: one button per value, the chosen one marked. */
function segment(id: string, key: keyof Settings): void {
  const group = $<HTMLDivElement>(id);
  const buttons = [...group.querySelectorAll("button")] as HTMLButtonElement[];

  const paint = (value: unknown) => {
    for (const b of buttons) {
      const on = b.dataset.value === String(value);
      b.setAttribute("aria-checked", on ? "true" : "false");
      b.setAttribute("role", "radio");
      // Only the selected button is in the tab order, which is how a radio group
      // is meant to behave; arrow keys move within it.
      b.tabIndex = on ? 0 : -1;
    }
  };

  for (const [i, b] of buttons.entries()) {
    b.addEventListener("click", () => {
      paint(b.dataset.value);
      void patch(key, b.dataset.value);
    });
    b.addEventListener("keydown", (e) => {
      const step = e.key === "ArrowRight" ? 1 : e.key === "ArrowLeft" ? -1 : 0;
      if (!step) return;
      e.preventDefault();
      const next = buttons[(i + step + buttons.length) % buttons.length];
      next.focus();
      next.click();
    });
  }

  paint(config.settings[key]);
}

function toggle(id: string, key: keyof Settings): void {
  const el = $<HTMLInputElement>(id);
  el.checked = Boolean(config.settings[key]);
  el.addEventListener("change", () => void patch(key, el.checked));
}

function slider(
  id: string,
  key: keyof Settings,
  format: (n: number) => string,
): void {
  const el = $<HTMLInputElement>(id);
  const out = $<HTMLOutputElement>(`${id}Out`);
  el.value = String(config.settings[key]);
  out.textContent = format(Number(el.value));

  // `input` fires on every pixel of drag, so show the value live but only write on
  // release — otherwise a single drag would be dozens of file writes.
  el.addEventListener("input", () => (out.textContent = format(Number(el.value))));
  el.addEventListener("change", () => void patch(key, Number(el.value)));
}

/* ------------------------------------------------------------- sections */

/**
 * The per-widget switches.
 *
 * They patch the whole `widgets` map rather than one key, because that is the shape
 * Rust stores. The widget layer writes its computed layout back on first run, so the
 * positions in that map are always real ones - this never has to invent coordinates.
 */
function wireWidgets(): void {
  toggle("widgetsEnabled", "widgetsEnabled");

  for (const id of WIDGET_IDS) {
    const box = $<HTMLInputElement>(`wg-${id}`);
    box.checked = config.settings.widgets?.[id]?.on ?? true;
    box.addEventListener("change", () => {
      const map = { ...(config.settings.widgets || {}) };
      map[id] = { x: 0, y: 0, ...(map[id] || {}), on: box.checked };
      config.settings.widgets = map;
      void patch("widgets", map);
    });
  }

  const city = $<HTMLInputElement>("weatherCity");
  const note = $<HTMLParagraphElement>("weatherNote");
  city.value = config.settings.weatherCity;

  city.addEventListener("keydown", async (e) => {
    if (e.key !== "Enter") return;
    const query = city.value.trim();
    if (!query) return;
    note.textContent = `Looking up ${query}…`;
    try {
      // Open-Meteo's geocoding endpoint: no account, no key, CORS open.
      const r = await fetch(
        `https://geocoding-api.open-meteo.com/v1/search?name=${encodeURIComponent(query)}&count=1`,
      );
      const hit = (await r.json())?.results?.[0];
      if (!hit) {
        note.textContent = `No place called "${query}". Try adding the country.`;
        return;
      }
      const name = [hit.name, hit.admin1, hit.country].filter(Boolean).join(", ");
      city.value = hit.name;
      await invoke("cmd_patch_settings", {
        patch: { weatherCity: hit.name, weatherLat: hit.latitude, weatherLon: hit.longitude },
      });
      config.settings.weatherCity = hit.name;
      note.textContent = `Weather now shows ${name}.`;
    } catch (err) {
      note.textContent = `Could not look that up (${err}). Check your connection.`;
    }
  });
}

function wireWallpaper(): void {
  const restore = $<HTMLButtonElement>("paperRestore");
  restore.disabled = !config.wallpaperChanged;

  for (const b of document.querySelectorAll<HTMLButtonElement>(".paper")) {
    b.addEventListener("click", async () => {
      const ok = await invoke<boolean>("cmd_set_wallpaper", { which: b.dataset.paper });
      if (ok) {
        config.wallpaperChanged = true;
        restore.disabled = false;
      }
    });
  }

  restore.addEventListener("click", async () => {
    const ok = await invoke<boolean>("cmd_set_wallpaper", { which: "restore" });
    if (ok) {
      config.wallpaperChanged = false;
      restore.disabled = true;
    }
  });
}

function wireStartup(): void {
  const el = $<HTMLInputElement>("startAtLogin");
  el.checked = config.startAtLogin;
  el.addEventListener("change", async () => {
    try {
      // Reflect what is actually on disk afterwards: if writing the shortcut failed
      // the switch must snap back rather than claim a setting that is not there.
      el.checked = await invoke<boolean>("cmd_set_start_at_login", {
        enabled: el.checked,
      });
    } catch (e) {
      el.checked = config.startAtLogin;
      $<HTMLParagraphElement>("startupNote").textContent = `Could not change it: ${e}`;
    }
    config.startAtLogin = el.checked;
  });
}

function wireLeaving(): void {
  $<HTMLButtonElement>("restart").addEventListener("click", () => {
    void invoke("cmd_app_restart");
  });
  $<HTMLButtonElement>("normal").addEventListener("click", () => {
    void invoke("cmd_return_to_normal");
  });
}

/* -------------------------------------------------------------- startup */

async function main(): Promise<void> {
  // Glass tint first, so nothing is drawn in the wrong colour.
  await followTint(() => void applyGlass());
  const win = getCurrentWindow();
  $<HTMLButtonElement>("close").addEventListener("click", () => void win.close());
  document.addEventListener("keydown", (e) => {
    if (e.key === "Escape") void win.close();
  });

  config = await invoke<Config>("cmd_get_config");

  segment("theme", "theme");
  segment("iconShape", "iconShape");
  segment("dockEdge", "dockEdge");

  toggle("dockEnabled", "dockEnabled");
  toggle("dockAlwaysVisible", "dockAlwaysVisible");
  toggle("dockMagnification", "dockMagnification");
  toggle("topbarEnabled", "topbarEnabled");
  toggle("topbarAutoHide", "topbarAutoHide");
  toggle("topbarAppPill", "topbarAppPill");
  toggle("island", "island");
  toggle("switcherEnabled", "switcherEnabled");

  slider("dockIconSize", "dockIconSize", (n) => `${n} px`);
  slider("animationSpeed", "animationSpeed", (n) => (n === 0 ? "off" : `${n}×`));

  wireWidgets();
  wireWallpaper();
  wireStartup();
  wireLeaving();

  $<HTMLParagraphElement>("sub").textContent = `macui ${config.version}`;
  $<HTMLParagraphElement>("path").textContent = config.settingsPath;

  // Windows' own "Animation effects" switch overrides ours, per the core rule that
  // Windows' behaviour wins. Say so rather than letting the slider look broken.
  try {
    const prefs = await invoke<{ animations: boolean }>("cmd_system_prefs");
    $<HTMLParagraphElement>("animNote").textContent = prefs.animations
      ? "Scales every animation in the UI. 0 turns them off."
      : "Windows' own “Animation effects” setting is off, so animations stay off here too.";
  } catch {
    /* the note is decoration; a failure here must not stop the window opening */
  }

  applyOwnAppearance();

  await win.setFocus();
}

void main();
