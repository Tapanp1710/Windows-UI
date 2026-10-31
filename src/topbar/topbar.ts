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
    netBtn.title = "Not connected";
  } else if (s.network.wifi) {
    netBtn.innerHTML = ICON.wifi;
    netBtn.classList.remove("off");
    netBtn.title = s.network.ssid ? `Wi-Fi — ${s.network.ssid}` : "Wi-Fi — connected";
  } else {
    netBtn.innerHTML = ICON.ethernet;
    netBtn.classList.remove("off");
    netBtn.title = "Wired network — connected";
  }

  // --- bluetooth
  btBtn.innerHTML = ICON.bt;
  btBtn.hidden = !s.bluetooth.present;
  btBtn.classList.toggle("off", !s.bluetooth.on);
  btBtn.title = s.bluetooth.on ? "Bluetooth — on" : "Bluetooth — off";

  // --- volume
  if (!s.volume.available) {
    volBtn.innerHTML = ICON.volMute;
    volBtn.classList.add("off");
    volBtn.title = "No playback device";
  } else {
    volBtn.classList.remove("off");
    volBtn.innerHTML = s.volume.muted
      ? ICON.volMute
      : s.volume.level > 50
        ? ICON.volHigh
        : ICON.volLow;
    volBtn.title = s.volume.muted
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
    battBtn.title = s.battery.charging
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

async function syncRegions(): Promise<void> {
  await new Promise(requestAnimationFrame);
  const rects = [appPill, clockPill, statusPill]
    .filter((el) => !el.hidden && el.offsetWidth > 0)
    .map((el) => {
      const r = el.getBoundingClientRect();
      // Grow by a couple of px so the region never clips the pill's own
      // antialiased rounded corners or its shadow.
      return {
        x: Math.floor(r.left) - 2,
        y: Math.floor(r.top) - 2,
        w: Math.ceil(r.width) + 4,
        h: Math.ceil(r.height) + 4,
      };
    });

  const sig = JSON.stringify(rects);
  if (sig === lastRegions) return;
  lastRegions = sig;
  await invoke("cmd_set_topbar_regions", { rects });
}

/* ----------------------------------------------------------- interaction */

// All four status controls open the same Windows Quick Settings panel the taskbar
// does, exactly as the brief requires.
for (const el of [netBtn, btBtn, volBtn, battBtn]) {
  el.addEventListener("click", () => void invoke("cmd_open_quick_settings"));
}

clockPill.addEventListener("click", () => void invoke("cmd_open_notifications"));

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
    volBtn.title = `Volume ${currentVolume}% — scroll to change`;
  },
  { passive: false },
);

let currentVolume = 50;

// Clicking the focused-app pill brings that app forward, which is a no-op when it is
// already focused — harmless, and it makes the pill feel live rather than decorative.
appPill.addEventListener("click", () => void invoke("cmd_focus_foreground"));

/* --------------------------------------------------------------- startup */

async function applyPrefs(): Promise<void> {
  try {
    const p = await invoke<{
      animations: boolean;
      iconShape: string;
      theme: string;
      animationSpeed: number;
    }>("cmd_system_prefs");
    const root = document.documentElement;
    root.dataset.animations = p.animations ? "on" : "off";
    root.dataset.iconShape = p.iconShape || "circle";
    if (p.theme === "light" || p.theme === "dark") root.dataset.theme = p.theme;
    root.style.setProperty("--anim-scale", p.animations ? String(p.animationSpeed) : "0");
  } catch {
    /* defaults in CSS are fine */
  }
}

async function refresh(): Promise<void> {
  try {
    const s = await invoke<Status>("cmd_status");
    if (pendingVolume === null) currentVolume = s.volume.level;
    renderStatus(s);
    await syncRegions();
  } catch (err) {
    console.error("top bar refresh failed", err);
  }
}

async function main(): Promise<void> {
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
  await getCurrentWindow().show();
}

void main();
