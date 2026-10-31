/**
 * Top bar — the visual replacement for the taskbar's system area.
 *
 * Every control here opens *the real Windows panel* rather than a reimplementation:
 * clicking Wi-Fi, Bluetooth, volume or battery sends Win+A (Quick Settings), clicking
 * the clock sends Win+N (calendar and notifications), and the chevron opens the real
 * tray overflow. That is the only way to satisfy the brief's rule that behaviour must
 * be identical to stock Windows — a hand-drawn Wi-Fi panel would be a different panel.
 */

import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { listen } from "@tauri-apps/api/event";
import {
  addToShelf,
  initIsland,
  isOpen as islandOpen,
  paint as paintIsland,
  setDragging,
  setIslandEnabled,
  setOnStateChange,
} from "./island";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import "../shared/theme.css";
import "./topbar.css";

const REFRESH_MS = 2000;

interface Status {
  battery: { percent: number | null; charging: boolean; present: boolean; saver: boolean };
  volume: { level: number; muted: boolean; available: boolean };
  network: { connected: boolean; wifi: boolean; signal: number | null; ssid: string | null };
  focused: { name: string; icon: string | null };
  bluetooth: { present: boolean; on: boolean };
}

const $ = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;

const appPill = $<HTMLButtonElement>("app-pill");
const appName = $<HTMLSpanElement>("app-name");
const appIcon = $<HTMLImageElement>("app-icon");
const clockPill = $<HTMLButtonElement>("clock-pill");
const clockEl = $<HTMLSpanElement>("clock");
const statusPill = $<HTMLDivElement>("status-pill");
const netBtn = $<HTMLButtonElement>("net-btn");
const btBtn = $<HTMLButtonElement>("bt-btn");
const volBtn = $<HTMLButtonElement>("vol-btn");
const battBtn = $<HTMLButtonElement>("batt-btn");
const battFill = $<HTMLElement>("batt-fill");
const battPct = $<HTMLSpanElement>("batt-pct");
const chevron = $<HTMLButtonElement>("tray-chevron");

/* ------------------------------------------------------------------ icons */

const ICON = {
  wifi: `<svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5"
    stroke-linecap="round"><path d="M1.8 5.6a9.5 9.5 0 0 1 12.4 0"/>
    <path d="M4.2 8.3a6 6 0 0 1 7.6 0"/><path d="M6.5 10.9a2.5 2.5 0 0 1 3 0"/>
    <circle cx="8" cy="13.2" r=".9" fill="currentColor" stroke="none"/></svg>`,
  wifiOff: `<svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5"
    stroke-linecap="round"><path d="M1.8 5.6a9.5 9.5 0 0 1 12.4 0" opacity=".45"/>
    <path d="M4.2 8.3a6 6 0 0 1 7.6 0" opacity=".45"/>
    <circle cx="8" cy="13.2" r=".9" fill="currentColor" stroke="none" opacity=".45"/>
    <path d="M2.5 2.5l11 11"/></svg>`,
  ethernet: `<svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5"
    stroke-linejoin="round"><rect x="2" y="6" width="12" height="7" rx="1.5"/>
    <path d="M5 6V3.5h6V6M5.5 9.5v2M8 9.5v2M10.5 9.5v2"/></svg>`,
  bt: `<svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5"
    stroke-linejoin="round" stroke-linecap="round">
    <path d="M5 4.5 11 11l-3 2.6V2.4L11 5 5 11.5"/></svg>`,
  volHigh: `<svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5"
    stroke-linejoin="round" stroke-linecap="round">
    <path d="M3 6.2h2.3L8.6 3.4v9.2L5.3 9.8H3z" fill="currentColor"/>
    <path d="M10.8 6a3 3 0 0 1 0 4"/><path d="M12.6 4.2a5.6 5.6 0 0 1 0 7.6"/></svg>`,
  volLow: `<svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5"
    stroke-linejoin="round" stroke-linecap="round">
    <path d="M3 6.2h2.3L8.6 3.4v9.2L5.3 9.8H3z" fill="currentColor"/>
    <path d="M10.8 6a3 3 0 0 1 0 4"/></svg>`,
  volMute: `<svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5"
    stroke-linejoin="round" stroke-linecap="round">
    <path d="M3 6.2h2.3L8.6 3.4v9.2L5.3 9.8H3z" fill="currentColor"/>
    <path d="M11 6.2l3.2 3.2M14.2 6.2 11 9.4"/></svg>`,
};

/* ----------------------------------------------------------------- clock */

/**
 * Follow the user's Windows date and time format.
 *
 * `Intl` with an undefined locale resolves to the browser's locale, which WebView2
 * takes from Windows' regional settings — so 12/24-hour and day/month order match what
 * the taskbar shows without us parsing the registry.
 */
const timeFmt = new Intl.DateTimeFormat(undefined, { hour: "numeric", minute: "2-digit" });
const dateFmt = new Intl.DateTimeFormat(undefined, {
  weekday: "short",
  day: "numeric",
  month: "short",
});

function tickClock(): void {
  const now = new Date();
  clockEl.textContent = `${dateFmt.format(now)}  ${timeFmt.format(now)}`;
}

/* ---------------------------------------------------------------- status */

function renderStatus(s: Status): void {
  // --- focused app
  const name = s.focused.name || "Desktop";
  appName.textContent = name;
  if (s.focused.icon) {
    appIcon.src = s.focused.icon;
    appIcon.hidden = false;
  } else {
    appIcon.hidden = true;
  }

  // --- network
  if (!s.network.connected) {
    netBtn.innerHTML = ICON.wifiOff;
    netBtn.classList.add("off");
    netBtn.ariaLabel = "Not connected";
  } else if (s.network.wifi) {
    netBtn.innerHTML = ICON.wifi;
    netBtn.classList.remove("off");
    netBtn.ariaLabel = s.network.ssid ? `Wi-Fi — ${s.network.ssid}` : "Wi-Fi — connected";
  } else {
    netBtn.innerHTML = ICON.ethernet;
    netBtn.classList.remove("off");
    netBtn.ariaLabel = "Wired network — connected";
  }

  // --- bluetooth
  btBtn.innerHTML = ICON.bt;
  btBtn.hidden = !s.bluetooth.present;
  btBtn.classList.toggle("off", !s.bluetooth.on);
  btBtn.ariaLabel = s.bluetooth.on ? "Bluetooth — on" : "Bluetooth — off";

  // --- volume
  if (!s.volume.available) {
    volBtn.innerHTML = ICON.volMute;
    volBtn.classList.add("off");
    volBtn.ariaLabel = "No playback device";
  } else {
    volBtn.classList.remove("off");
    volBtn.innerHTML = s.volume.muted
      ? ICON.volMute
      : s.volume.level > 50
        ? ICON.volHigh
        : ICON.volLow;
    volBtn.ariaLabel = s.volume.muted
      ? "Muted — scroll to change"
      : `Volume ${s.volume.level}% — scroll to change`;
  }

  // --- battery
  if (!s.battery.present || s.battery.percent === null) {
    battBtn.hidden = true;
  } else {
    battBtn.hidden = false;
    const pct = s.battery.percent;
    battFill.style.width = `${Math.max(4, pct)}%`;
    battPct.textContent = `${pct}%`;
    battBtn.classList.toggle("low", pct <= 20 && !s.battery.charging);
    battBtn.classList.toggle("charging", s.battery.charging);
    battBtn.ariaLabel = s.battery.charging
      ? `Battery ${pct}% — charging`
      : `Battery ${pct}%${s.battery.saver ? " — battery saver on" : ""}`;
  }
}

/* -------------------------------------------- Win32 hit-test region sync */

/**
 * Tell Rust exactly which rectangles should be clickable.
 *
 * Without this the window would be a full-width always-on-top sheet that eats every
 * click along the top of the screen — a behaviour change, and the thing the brief
 * most forbids.
 */
let lastRegions = "";

/**
 * How far outside a pill the region reaches, in CSS px.
 *
 * Must match --shadow-reach in theme.css: it is exactly how far the shadow can
 * extend, so the region contains the whole fade and clips nothing visible.
 */
const PAD = 18;

/** Mirrors the Rust sampler. Starts concealed, like the markup. */
let revealed = false;

async function syncRegions(): Promise<void> {
  await new Promise(requestAnimationFrame);
  // Nothing is clickable while the bar is away, so the top strip of the screen
  // behaves exactly like bare screen.
  // The island is measured as a whole, so when it expands the clickable area
  // follows it down over the body; collapsed, its box is just the pill.
  const parts: Array<[HTMLElement, number | null]> = [
    [appPill, null],
    // Collapsed, the island IS its pill. Measuring the container instead takes in
    // `.island-body`, which keeps its full 330px width while its height is zero -
    // so the region came out three times wider than the pill and the window's
    // acrylic showed through the empty part as a pale slab across the middle of
    // the bar. That was the white marking. See D-034.
    [islandOpen() ? islandEl : clockPill, islandOpen() ? 18 : null],
    [statusPill, null],
  ];

  const rects = !revealed
    ? []
    : parts
        .filter(([el]) => !el.hidden && el.offsetWidth > 0)
        .map(([el, radius]) => {
          // While the island is open its box is measured MID-TRANSITION, because the
          // region has to be sent the moment the state changes. Measuring then gives
          // the collapsed size and the region clips the body away as it grows. So an
          // open island reserves its full footprint instead of its current one.
          // `islandEl` is only in this list while the island is open, so reaching it
          // here means the footprint is the right measure.
          const r = el === islandEl ? islandFootprint() : el.getBoundingClientRect();
          // Inflated by the shadow's reach, with a matching corner radius: a region
          // tight to the pill clips the shadow into four hard corners, which is
          // what made these look like sharp slabs rather than floating islands.
          return {
            x: Math.floor(r.left) - PAD,
            y: Math.floor(r.top) - PAD,
            w: Math.ceil(r.width) + PAD * 2,
            h: Math.ceil(r.height) + PAD * 2,
            r: (radius ?? Math.round(r.height / 2)) + PAD,
          };
        });

  const sig = JSON.stringify(rects);
  if (sig === lastRegions) return;
  lastRegions = sig;
  await invoke("cmd_set_topbar_regions", { rects });
}

/* ----------------------------------------------------------- interaction */

/**
 * Open one of our own panels, centred under the pill that was clicked.
 *
 * Windows' own flyouts open down by the taskbar, nowhere near the bar you pressed.
 * These appear where the click was. Everything they do not duplicate - choosing a
 * network, pairing a device - is one button away on the real Windows page.
 */
function openPanel(kind: string, anchor: HTMLElement, width: number, height: number): void {
  const r = anchor.getBoundingClientRect();
  void invoke("cmd_open_panel", {
    kind,
    anchorX: r.left + r.width / 2,
    width,
    height,
  });
}

for (const el of [netBtn, btBtn, volBtn, battBtn]) {
  el.addEventListener("click", () => openPanel("quick", statusPill, 348, 536));
}

// The clock moved onto the status pill, so it is the clock itself that opens the
// calendar now; the island behind it opens nothing on a click, because hovering it
// already shows everything it has.
clockEl.addEventListener("click", (e) => {
  e.stopPropagation();
  openPanel("calendar", clockEl, 272, 348);
});

chevron.addEventListener("click", () => void invoke("cmd_open_tray_overflow"));

// Scrolling the volume icon changes the volume, like the Windows taskbar.
let pendingVolume: number | null = null;
volBtn.addEventListener(
  "wheel",
  (e) => {
    e.preventDefault();
    const base = pendingVolume ?? currentVolume;
    const step = e.deltaY < 0 ? 2 : -2;
    pendingVolume = Math.max(0, Math.min(100, base + step));
    currentVolume = pendingVolume;
    void invoke("cmd_set_volume", { level: pendingVolume }).then(() => {
      pendingVolume = null;
    });
    // Reflect it immediately rather than waiting for the next poll.
    volBtn.innerHTML = currentVolume > 50 ? ICON.volHigh : ICON.volLow;
    volBtn.ariaLabel = `Volume ${currentVolume}% — scroll to change`;
  },
  { passive: false },
);

let currentVolume = 50;

// Clicking the focused-app pill brings that app forward, which is a no-op when it is
// already focused — harmless, and it makes the pill feel live rather than decorative.
appPill.addEventListener("click", () => void invoke("cmd_focus_foreground"));

/**
 * Right-clicking any pill opens the settings window.
 *
 * The top bar is only 56px tall and its window clips anything drawn outside it, so a
 * popup menu here would need the same window-growing dance the dock does for one
 * entry. Opening settings directly is the whole of that menu anyway, and it gives a
 * second reachable route to Quit while the Windows taskbar - and with it the tray
 * icon - is hidden.
 */
document.addEventListener("contextmenu", (e) => {
  e.preventDefault();
  void invoke("cmd_open_settings");
});

/* --------------------------------------------------------------- startup */

async function applyPrefs(): Promise<void> {
  try {
    const p = await invoke<{
      animations: boolean;
      iconShape: string;
      theme: string;
      animationSpeed: number;
      topbarAppPill: boolean;
      island: boolean;
    }>("cmd_system_prefs");
    const root = document.documentElement;
    root.dataset.animations = p.animations ? "on" : "off";
    root.dataset.iconShape = p.iconShape || "circle";
    if (p.theme === "light" || p.theme === "dark") root.dataset.theme = p.theme;
    else delete root.dataset.theme;
    root.style.setProperty("--anim-scale", p.animations ? String(p.animationSpeed) : "0");

    appPill.hidden = !p.topbarAppPill;
    setIslandEnabled(p.island);
    lastRegions = "";
  } catch {
    /* defaults in CSS are fine */
  }
}

const barEl = document.getElementById("bar") as HTMLDivElement;
const islandEl = document.getElementById("island") as HTMLDivElement;

/** The island's widest, tallest shape. Must cover `.island-body` in topbar.css. */
const ISLAND_W = 330 + 26;
const ISLAND_H = 268;

function islandFootprint(): DOMRect {
  const pill = clockPill.getBoundingClientRect();
  const cx = pill.left + pill.width / 2;
  return new DOMRect(cx - ISLAND_W / 2, pill.top, ISLAND_W, ISLAND_H);
}

function setRevealed(next: boolean): void {
  if (next === revealed) return;
  revealed = next;
  barEl.classList.toggle("hidden", !next);
  void syncRegions();
}

async function refresh(): Promise<void> {
  try {
    const s = await invoke<Status>("cmd_status");
    if (pendingVolume === null) currentVolume = s.volume.level;
    renderStatus(s);
    paintIsland(s as never);
    await syncRegions();
  } catch (err) {
    console.error("top bar refresh failed", err);
  }
}

/**
 * Files dragged near the bar.
 *
 * Tauri reports the native drop target's events for this window; the island opens
 * while something is over it so there is somewhere to aim, and the paths are parked
 * on the shelf when it lands.
 */
async function wireDrops(): Promise<void> {
  try {
    await getCurrentWebview().onDragDropEvent((e) => {
      if (e.payload.type === "enter" || e.payload.type === "over") {
        setDragging(true);
      } else if (e.payload.type === "drop") {
        setDragging(false);
        void addToShelf(e.payload.paths ?? []);
      } else {
        setDragging(false);
      }
    });
  } catch (err) {
    await invoke("cmd_log", { msg: `drag-drop listener failed: ${err}` });
  }
}

async function main(): Promise<void> {
  initIsland(true);
  void wireDrops();
  // The region has to be re-sent the moment the island grows or shrinks, or the
  // body would be drawn outside the clickable area and swallow nothing.
  setOnStateChange(() => {
    lastRegions = "";
    void syncRegions();
    // Hold the bar down while the island is open, or it slides away the moment the
    // pointer leaves the 56px bar strip on its way to the shelf.
    void invoke("cmd_topbar_pin", { on: islandOpen() });
  });
  await applyPrefs();
  tickClock();
  await refresh();

  // Tick on the minute boundary so the displayed time is never a second stale.
  const scheduleClock = () => {
    const now = new Date();
    const ms = (60 - now.getSeconds()) * 1000 - now.getMilliseconds();
    window.setTimeout(() => {
      tickClock();
      void syncRegions();
      scheduleClock();
    }, ms + 50);
  };
  scheduleClock();

  window.setInterval(() => void refresh(), REFRESH_MS);

  try {
    await listen("topbar:reveal", () => setRevealed(true));
    await listen("topbar:conceal", () => setRevealed(false));
  } catch (e) {
    await invoke("cmd_log", { msg: `topbar listen() FAILED, auto-hide disabled: ${e}` });
  }
  // Sync to the sampler's real state: anything emitted before this page attached
  // its listeners is gone, which once left the dock stuck concealed (D-011).
  try {
    setRevealed(await invoke<boolean>("cmd_topbar_revealed"));
  } catch {
    /* keep the concealed default */
  }

  await getCurrentWindow().show();
  // Transitions on only after the concealed start state has been painted, so the
  // bar does not animate a slide nobody asked for.
  requestAnimationFrame(() => barEl.classList.remove("no-anim"));
}

void main();
