/**
 * Top bar panels.
 *
 * One page serving two flyouts, picked by asking Rust which one this window is:
 * quick settings and the calendar. Both open under the pill that was clicked, instead of down by the
 * Windows taskbar where Windows' own flyouts appear.
 *
 * These are not pictures of the Windows panels. The Wi-Fi and Bluetooth switches
 * drive the real radios through `Windows.Devices.Radios`, the slider drives the real
 * audio endpoint, and everything deliberately not duplicated here - picking a
 * network, pairing a device - is one click away on the matching Windows page.
 */

import { invoke } from "@tauri-apps/api/core";
import "../shared/theme.css";
import "./panel.css";

const $ = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;

const ICON = {
  wifiOn: `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round"><path d="M2.5 8.5a15 15 0 0 1 19 0"/><path d="M5.5 12a10.5 10.5 0 0 1 13 0"/><path d="M8.5 15.3a6 6 0 0 1 7 0"/><circle cx="12" cy="19" r="1.2" fill="currentColor" stroke="none"/></svg>`,
  wifiOff: `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round"><path d="M8.5 15.3a6 6 0 0 1 7 0"/><circle cx="12" cy="19" r="1.2" fill="currentColor" stroke="none"/><path d="M3 3l18 18" opacity="0.9"/></svg>`,
  bt: `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linejoin="round" stroke-linecap="round"><path d="M7 7.5 17 16.5 12 21V3l5 4.5L7 16.5"/></svg>`,
  volHigh: `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M4 9.5h3L11 6v12l-4-3.5H4z" fill="currentColor"/><path d="M15.5 9a4 4 0 0 1 0 6"/><path d="M18 6.5a7.5 7.5 0 0 1 0 11"/></svg>`,
  volMute: `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M4 9.5h3L11 6v12l-4-3.5H4z" fill="currentColor"/><path d="M16 9.5l5 5M21 9.5l-5 5"/></svg>`,
};

interface RadioInfo {
  kind: string;
  on: boolean;
  canToggle: boolean;
  name: string;
}

interface Status {
  battery: { percent: number | null; charging: boolean; present: boolean };
  volume: { level: number; muted: boolean; available: boolean };
  network: { connected: boolean; internet: boolean };
}

/* ------------------------------------------------------------------ glass */

async function applyGlass(): Promise<void> {
  const dark = window.matchMedia("(prefers-color-scheme: dark)").matches;
  const forced = document.documentElement.dataset.theme;
  try {
    const ok = await invoke<boolean>("cmd_glass", {
      dark: forced ? forced === "dark" : dark,
    });
    if (ok) document.documentElement.dataset.acrylic = "on";
  } catch {
    /* painted fallback */
  }
}

/* ------------------------------------------------------- quick settings */

function paintTile(id: string, iconId: string, subId: string, r: RadioInfo | undefined, sub: string): void {
  const tile = $<HTMLButtonElement>(id);
  const on = Boolean(r?.on);
  tile.setAttribute("aria-pressed", on ? "true" : "false");
  tile.disabled = !r || !r.canToggle;
  $(subId).textContent = r ? sub : "Not available";
  $(iconId).innerHTML =
    iconId === "i-wifi" ? (on ? ICON.wifiOn : ICON.wifiOff) : ICON.bt;
  $(iconId).style.opacity = on ? "1" : "0.65";
}

let radios: RadioInfo[] = [];

async function refreshRadios(): Promise<void> {
  try {
    radios = await invoke<RadioInfo[]>("cmd_radios");
  } catch {
    radios = [];
  }
  const wifi = radios.find((r) => r.kind === "wifi");
  const bt = radios.find((r) => r.kind === "bluetooth");
  paintTile("t-wifi", "i-wifi", "s-wifi", wifi, wifi?.on ? "On" : "Off");
  paintTile("t-bt", "i-bt", "s-bt", bt, bt?.on ? "On" : "Off");
}

function wireTile(id: string, kind: string): void {
  $(id).addEventListener("click", async () => {
    const current = radios.find((r) => r.kind === kind);
    if (!current || !current.canToggle) return;
    // Paint the new state at once; a radio takes a moment to actually switch and a
    // tile that does nothing for a second feels broken.
    current.on = !current.on;
    await refreshPaintOnly();
    await invoke("cmd_set_radio", { kind, on: current.on });
    await refreshRadios();
  });
}

async function refreshPaintOnly(): Promise<void> {
  const wifi = radios.find((r) => r.kind === "wifi");
  const bt = radios.find((r) => r.kind === "bluetooth");
  paintTile("t-wifi", "i-wifi", "s-wifi", wifi, wifi?.on ? "On" : "Off");
  paintTile("t-bt", "i-bt", "s-bt", bt, bt?.on ? "On" : "Off");
}

async function refreshStatus(): Promise<void> {
  let s: Status;
  try {
    s = await invoke<Status>("cmd_status");
  } catch {
    return;
  }

  const vol = $<HTMLInputElement>("vol");
  if (document.activeElement !== vol) vol.value = String(s.volume.level);
  $("vol-out").textContent = `${s.volume.level}%`;
  $("mute").innerHTML = s.volume.muted || s.volume.level === 0 ? ICON.volMute : ICON.volHigh;

  const b = s.battery;
  const bar = $<HTMLElement>("batt-bar");
  if (!b.present) {
    $<HTMLElement>("batt-row").hidden = true;
  } else {
    bar.style.width = `${b.percent ?? 0}%`;
    bar.classList.toggle("low", (b.percent ?? 100) <= 20 && !b.charging);
    bar.classList.toggle("charging", b.charging);
    $("batt-value").textContent = `${b.percent ?? 0}%${b.charging ? " ⚡" : ""}`;
  }
}

function wireVolume(): void {
  const vol = $<HTMLInputElement>("vol");
  vol.addEventListener("input", () => {
    $("vol-out").textContent = `${vol.value}%`;
  });
  // `change`, not `input`: dragging fires continuously and each one is a COM call
  // into the audio endpoint.
  vol.addEventListener("change", () => void invoke("cmd_set_volume", { level: Number(vol.value) }));

  $("mute").addEventListener("click", async () => {
    const level = Number(vol.value) > 0 ? 0 : 35;
    vol.value = String(level);
    $("vol-out").textContent = `${level}%`;
    await invoke("cmd_set_volume", { level });
  });
}

/* ------------------------------------------------------------- calendar */

function renderCalendar(): void {
  const now = new Date();
  $("cal-dow").textContent = now.toLocaleDateString([], { weekday: "long" });
  $("cal-date").textContent = now.toLocaleDateString([], { day: "numeric", month: "long" });
  $("cal-month").textContent = now.toLocaleDateString([], { month: "long", year: "numeric" });

  const grid = $("cal-grid");
  grid.textContent = "";

  // Weekday initials from the locale, worked out from a known Sunday.
  const sunday = new Date(2024, 0, 7);
  for (let i = 0; i < 7; i++) {
    const d = new Date(sunday);
    d.setDate(sunday.getDate() + i);
    const h = document.createElement("span");
    h.className = "dow";
    h.textContent = d.toLocaleDateString([], { weekday: "narrow" });
    grid.appendChild(h);
  }

  const first = new Date(now.getFullYear(), now.getMonth(), 1);
  const days = new Date(now.getFullYear(), now.getMonth() + 1, 0).getDate();
  for (let i = 0; i < first.getDay(); i++) grid.appendChild(document.createElement("span"));
  for (let d = 1; d <= days; d++) {
    const cell = document.createElement("span");
    cell.textContent = String(d);
    if (d === now.getDate()) cell.className = "today";
    grid.appendChild(cell);
  }
}

/* -------------------------------------------------------------- startup */

async function main(): Promise<void> {
  const kind = await invoke<string>("cmd_panel_kind").catch(() => "quick");

  // Every link opens the real Windows page for the thing we are not duplicating.
  for (const b of document.querySelectorAll<HTMLButtonElement>("[data-page]")) {
    b.addEventListener("click", async () => {
      await invoke("cmd_open_ms_settings", { page: b.dataset.page });
      await invoke("cmd_close_panel");
    });
  }

  // Escape closes, like any flyout. (Clicking elsewhere closes it too — that is
  // handled in Rust, on the window losing focus.)
  document.addEventListener("keydown", (e) => {
    if (e.key === "Escape") void invoke("cmd_close_panel");
  });

  await applyGlass();

  if (kind === "calendar") {
    $("calendar").hidden = false;
    renderCalendar();
    $("open-notifications").addEventListener("click", async () => {
      await invoke("cmd_close_panel");
      await invoke("cmd_open_notifications");
    });
  } else {
    $("quick").hidden = false;
    wireTile("t-wifi", "wifi");
    wireTile("t-bt", "bluetooth");
    wireVolume();
    await Promise.all([refreshRadios(), refreshStatus()]);
    window.setInterval(() => void refreshStatus(), 1500);
  }
}

void main();
