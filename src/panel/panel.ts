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

const SVG = (d: string, extra = "") =>
  `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">${d}${extra}</svg>`;

const ICON: Record<string, string> = {
  wifi: SVG(`<path d="M2.5 8.5a15 15 0 0 1 19 0"/><path d="M5.5 12a10.5 10.5 0 0 1 13 0"/><path d="M8.5 15.3a6 6 0 0 1 7 0"/><circle cx="12" cy="19" r="1.2" fill="currentColor" stroke="none"/>`),
  wifiOff: SVG(`<path d="M8.5 15.3a6 6 0 0 1 7 0"/><circle cx="12" cy="19" r="1.2" fill="currentColor" stroke="none"/><path d="M3 3l18 18"/>`),
  bluetooth: SVG(`<path d="M7 7.5 17 16.5 12 21V3l5 4.5L7 16.5"/>`),
  airplane: SVG(`<path d="M10.5 3.5a1.5 1.5 0 0 1 3 0V9l7.5 4.2v2.3L13.5 13v4.6l2.5 1.7v1.9L12 20l-4 1.2v-1.9l2.5-1.7V13L3 15.5v-2.3L10.5 9z"/>`),
  battsaver: SVG(`<rect x="2.5" y="7.5" width="16" height="9" rx="2.5"/><path d="M21 11v2"/><path d="M11.5 9.8 8.8 13h3l-1 2.4L13.8 12h-3z" fill="currentColor" stroke="none"/>`),
  nightlight: SVG(`<path d="M20 13.5A8 8 0 0 1 10.5 4a8.2 8.2 0 1 0 9.5 9.5z"/>`),
  accessibility: SVG(`<circle cx="12" cy="4.6" r="1.8"/><path d="M4.5 8.2c4.8 1.6 10.2 1.6 15 0"/><path d="M12 8.6v5m0 0-2.6 6.4M12 13.6l2.6 6.4"/>`),
  nearby: SVG(`<path d="M12 20.5 3.5 12 12 3.5 20.5 12z"/><path d="M9 12h6m-2.5-2.5L15 12l-2.5 2.5"/>`),
  cast: SVG(`<rect x="2.5" y="4.5" width="19" height="13" rx="2.5"/><path d="M8 20.5h8"/><path d="M6 14.5a3.5 3.5 0 0 1 3.5 3.5"/>`),
  project: SVG(`<rect x="2.5" y="4.5" width="19" height="12" rx="2"/><path d="M8 20h8m-4-3.5V20"/>`),
  vpn: SVG(`<rect x="4.5" y="10.5" width="15" height="9.5" rx="2.5"/><path d="M8 10.5V7.8a4 4 0 0 1 8 0v2.7"/>`),
  hotspot: SVG(`<circle cx="12" cy="13" r="2.2"/><path d="M8.4 9.4a5 5 0 0 0 0 7.2m7.2-7.2a5 5 0 0 1 0 7.2"/><path d="M5.6 6.6a9 9 0 0 0 0 12.8m12.8-12.8a9 9 0 0 1 0 12.8"/>`),
  brightness: SVG(`<circle cx="12" cy="12" r="4"/><path d="M12 2.5v2M12 19.5v2M2.5 12h2M19.5 12h2M5.2 5.2l1.4 1.4M17.4 17.4l1.4 1.4M18.8 5.2l-1.4 1.4M6.6 17.4l-1.4 1.4"/>`),
  volHigh: SVG(`<path d="M4 9.5h3L11 6v12l-4-3.5H4z" fill="currentColor"/><path d="M15.5 9a4 4 0 0 1 0 6"/><path d="M18 6.5a7.5 7.5 0 0 1 0 11"/>`),
  volMute: SVG(`<path d="M4 9.5h3L11 6v12l-4-3.5H4z" fill="currentColor"/><path d="M16 9.5l5 5M21 9.5l-5 5"/>`),
  chevron: SVG(`<path d="M9 6l6 6-6 6"/>`),
};

/**
 * Everything the Windows taskbar's Quick Settings offers, in the order it offers it.
 *
 * Three kinds of tile, and the difference matters:
 *
 * - `toggle` drives real hardware through `Windows.Devices.Radios`. Wi-Fi, Bluetooth
 *   and Airplane mode are switches, not pictures of switches.
 * - `page` opens the real Windows surface for things Windows exposes no API to set.
 *   Night light, Battery saver, Nearby sharing, VPN and Mobile hotspot have no
 *   supported way to toggle from another process, and faking one would mean driving
 *   undocumented registry state behind Windows' back - exactly what the brief rules
 *   out. So the tile takes you to the real control in one click instead of silently
 *   doing nothing.
 * - A toggle tile that also has a `page` grows a chevron, the way Windows' own
 *   Wi-Fi tile does: the switch on the left, "take me to the full list" on the right.
 */
interface Tile {
  id: string;
  name: string;
  icon: string;
  /** Drives real hardware. */
  toggle?: boolean;
  /** ms-settings: URI this tile opens, from the chevron or the whole tile. */
  page?: string;
  /** The projection flyout - what Win+P shows. Not an ms-settings page. */
  project?: boolean;
}

const TILES: Tile[] = [
  { id: "wifi", name: "Wi-Fi", icon: "wifi", toggle: true, page: "ms-settings:network-wifi" },
  { id: "bluetooth", name: "Bluetooth", icon: "bluetooth", toggle: true, page: "ms-settings:bluetooth" },
  { id: "airplane", name: "Airplane mode", icon: "airplane", toggle: true, page: "ms-settings:network-airplanemode" },
  { id: "battsaver", name: "Battery saver", icon: "battsaver", page: "ms-settings:batterysaver" },
  { id: "nightlight", name: "Night light", icon: "nightlight", page: "ms-settings:nightlight" },
  { id: "accessibility", name: "Accessibility", icon: "accessibility", page: "ms-settings:easeofaccess" },
  { id: "nearby", name: "Nearby sharing", icon: "nearby", page: "ms-settings:crossdevice" },
  { id: "cast", name: "Cast", icon: "cast", page: "ms-settings:connecteddevices" },
  { id: "project", name: "Project", icon: "project", project: true },
  { id: "vpn", name: "VPN", icon: "vpn", page: "ms-settings:network-vpn" },
  { id: "hotspot", name: "Mobile hotspot", icon: "hotspot", page: "ms-settings:network-mobilehotspot" },
];

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
  ssid: string | null;
}

/** The joined network, for the Wi-Fi tile's sub-label. */
let ssid: string | null = null;

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

let radios: RadioInfo[] = [];

/** Airplane mode is on when every radio we can see is off. */
function airplaneOn(): boolean {
  return radios.length > 0 && radios.every((r) => !r.on);
}

function tileState(t: Tile): { on: boolean; enabled: boolean; sub: string } {
  if (!t.toggle) return { on: false, enabled: true, sub: "Open settings" };
  if (t.id === "airplane") {
    const any = radios.some((r) => r.canToggle);
    const on = airplaneOn();
    return { on, enabled: any, sub: any ? (on ? "On" : "Off") : "No radios" };
  }
  const r = radios.find((x) => x.kind === t.id);
  if (!r) return { on: false, enabled: false, sub: "Not available" };
  if (t.id === "wifi" && r.on && ssid) return { on: true, enabled: r.canToggle, sub: ssid };
  return { on: r.on, enabled: r.canToggle, sub: r.on ? "On" : "Off" };
}

function buildTiles(): void {
  const host = $("tiles");
  host.textContent = "";
  for (const t of TILES) {
    const tile = document.createElement("button");
    tile.type = "button";
    tile.className = "tile";
    tile.dataset.id = t.id;
    if (t.toggle) tile.classList.add("toggle");
    tile.innerHTML = `
      <span class="tile-icon"></span>
      <span class="tile-text">
        <span class="tile-name">${t.name}</span>
        <span class="tile-sub">—</span>
      </span>`;

    tile.addEventListener("click", () => void onTile(t));

    // A toggle that also has a settings page gets the chevron Windows gives it.
    if (t.toggle && t.page) {
      const go = document.createElement("button");
      go.type = "button";
      go.className = "tile-go";
      go.innerHTML = ICON.chevron;
      go.setAttribute("aria-label", `${t.name} settings`);
      // Without this the click would also fire the tile underneath and flip the radio.
      go.addEventListener("click", (e) => {
        e.stopPropagation();
        void openPage(t.page!);
      });
      tile.appendChild(go);
    }
    host.appendChild(tile);
  }
  paintTiles();
}

function paintTiles(): void {
  for (const t of TILES) {
    const el = document.querySelector<HTMLButtonElement>(`.tile[data-id="${t.id}"]`);
    if (!el) continue;
    const { on, enabled, sub } = tileState(t);
    el.setAttribute("aria-pressed", t.toggle ? String(on) : "false");
    el.classList.toggle("on", on);
    el.disabled = !enabled;
    el.querySelector(".tile-sub")!.textContent = sub;
    const icon = t.id === "wifi" && !on ? "wifiOff" : t.icon;
    el.querySelector(".tile-icon")!.innerHTML = ICON[icon];
  }
}

async function openPage(page: string): Promise<void> {
  await invoke("cmd_open_ms_settings", { page });
  await invoke("cmd_close_panel");
}

async function onTile(t: Tile): Promise<void> {
  if (t.project) {
    await invoke("cmd_open_project");
    await invoke("cmd_close_panel");
    return;
  }
  if (!t.toggle) {
    if (t.page) await openPage(t.page);
    return;
  }

  const { on, enabled } = tileState(t);
  if (!enabled) return;

  // Paint the new state at once. A radio takes a moment to actually switch, and a
  // tile that does nothing for a second feels broken. Rust queues the change and
  // the next snapshot corrects this if the radio refuses.
  if (t.id === "airplane") {
    for (const r of radios) if (r.canToggle) r.on = on;
    paintTiles();
    await invoke("cmd_set_airplane", { on: !on });
  } else {
    const r = radios.find((x) => x.kind === t.id);
    if (!r) return;
    r.on = !r.on;
    paintTiles();
    await invoke("cmd_set_radio", { kind: t.id, on: r.on });
  }
}

async function refreshRadios(): Promise<void> {
  try {
    radios = await invoke<RadioInfo[]>("cmd_radios");
  } catch {
    radios = [];
  }
  paintTiles();
}

/* ---------------------------------------------------------------- sliders */

async function refreshStatus(): Promise<void> {
  let s: Status;
  try {
    s = await invoke<Status>("cmd_status");
  } catch {
    return;
  }

  if (s.ssid !== ssid) {
    ssid = s.ssid;
    paintTiles();
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

/**
 * The brightness slider, where the machine has a panel that can be dimmed.
 *
 * Desktops with external monitors have no WMI brightness instance, so Rust answers
 * null and the row stays hidden rather than showing a slider that moves nothing.
 * The minimum is 10, not 0: WmiSetBrightness(0) blanks some panels completely.
 */
async function wireBrightness(): Promise<void> {
  const level = await invoke<number | null>("cmd_brightness").catch(() => null);
  if (level === null) return;

  const row = $<HTMLElement>("bright-row");
  const slider = $<HTMLInputElement>("bright");
  row.hidden = false;
  $("bright-icon").innerHTML = ICON.brightness;
  slider.value = String(Math.max(10, level));
  $("bright-out").textContent = `${level}%`;

  slider.addEventListener("input", () => {
    $("bright-out").textContent = `${slider.value}%`;
  });
  // Same reasoning as the volume slider: each change is a WMI round trip.
  slider.addEventListener("change", () =>
    void invoke("cmd_set_brightness", { level: Number(slider.value) }),
  );
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

/* ----------------------------------------------------------------- shelf */

interface ShelfItem {
  id: string;
  name: string;
  kind: string;
  icon: string | null;
}

const KIND_ICON: Record<string, string> = {
  folder: SVG(`<path d="M3 7.5a2 2 0 0 1 2-2h4l2 2.2h8a2 2 0 0 1 2 2v7.8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/>`),
  file: SVG(`<path d="M6 3.5h8l4 4v13a1 1 0 0 1-1 1H6a1 1 0 0 1-1-1v-16a1 1 0 0 1 1-1z"/><path d="M14 3.5v4h4"/>`),
  image: SVG(`<rect x="3.5" y="5" width="17" height="14" rx="2"/><circle cx="9" cy="10" r="1.6"/><path d="m4.5 17 4.5-4.5 3.5 3.5 2.5-2.5 4.5 4.5"/>`),
};

async function renderShelf(): Promise<void> {
  const items = await invoke<ShelfItem[]>("cmd_shelf").catch(() => []);
  const grid = $("shelf-grid");
  grid.textContent = "";
  $<HTMLElement>("shelf-empty").hidden = items.length > 0;

  for (const it of items) {
    const chip = document.createElement("div");
    chip.className = "chip";
    chip.innerHTML = `
      <span class="thumb">${it.icon ? `<img alt="" src="${it.icon}" />` : KIND_ICON[it.kind] ?? KIND_ICON.file}</span>
      <span>${it.name}</span>
      <button class="drop-x" type="button" aria-label="Remove ${it.name}">×</button>`;

    chip.addEventListener("click", async (e) => {
      if ((e.target as HTMLElement).classList.contains("drop-x")) {
        await invoke("cmd_shelf_remove", { id: it.id });
        await renderShelf();
        return;
      }
      await invoke("cmd_shelf_open", { id: it.id });
      await invoke("cmd_close_panel");
    });

    // Dragging a chip out hands the real path to Explorer through an OLE drag in
    // Rust; a web view cannot hand a filesystem path to a native drop target.
    chip.addEventListener("pointerdown", (down) => {
      if (down.button !== 0) return;
      const start = { x: down.clientX, y: down.clientY };
      const move = (m: PointerEvent) => {
        if (Math.hypot(m.clientX - start.x, m.clientY - start.y) < 6) return;
        cleanup();
        void invoke("cmd_shelf_drag_out", { ids: [it.id] });
      };
      const cleanup = () => {
        document.removeEventListener("pointermove", move);
        document.removeEventListener("pointerup", cleanup);
      };
      document.addEventListener("pointermove", move);
      document.addEventListener("pointerup", cleanup);
    });

    grid.appendChild(chip);
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

  if (kind === "shelf") {
    $("shelf").hidden = false;
    await renderShelf();
  } else if (kind === "calendar") {
    $("calendar").hidden = false;
    renderCalendar();
    $("open-notifications").addEventListener("click", async () => {
      await invoke("cmd_close_panel");
      await invoke("cmd_open_notifications");
    });
  } else {
    $("quick").hidden = false;
    buildTiles();
    wireVolume();
    await Promise.all([refreshRadios(), refreshStatus(), wireBrightness()]);
    // Radios are polled in Rust on their own thread, so asking for the snapshot is
    // a memory read; it is safe to do it on the same tick as the rest.
    window.setInterval(() => {
      void refreshStatus();
      void refreshRadios();
    }, 1500);
  }
}

void main();
