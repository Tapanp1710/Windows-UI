/**
 * Dock — visual replacement for the Windows taskbar.
 *
 * Division of labour with the Rust side:
 *   - Rust owns anything Win32: which apps exist, their icons, focus, launching, and
 *     the cursor-at-the-bottom-edge test that drives auto-hide.
 *   - This file owns layout and motion only. The Win32 window is resized when the item
 *     list changes and otherwise never moves; the slide animation is a CSS transform
 *     inside a stationary window, because moving a real window every frame is what
 *     makes docks stutter.
 *
 * Click behaviour deliberately mirrors the Windows taskbar exactly — see onClick().
 */

import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { listen } from "@tauri-apps/api/event";
import "../shared/theme.css";
import "./dock.css";

/** Must match `.slider { padding }` in dock.css. */
const SHADOW_PAD = 18;

/** Hover magnification, from the design brief. */
const MAG_HOVER = 1.15;
const MAG_NEIGHBOUR = 1.07;
const MAG_FAR = 1.02;
const LIFT_HOVER = 4;
const LIFT_NEIGHBOUR = 2;

/** How often to re-read the app list. Cheap: icons are cached in Rust. */
const REFRESH_MS = 1000;

interface DockItem {
  id: string;
  kind: "start" | "app" | "recycle";
  name: string;
  icon: string | null;
  running: boolean;
  windowCount: number;
  attention: boolean;
  progress: number | null;
  progressState: "none" | "normal" | "indeterminate" | "error" | "paused";
  pinned: boolean;
  hwnds: number[];
  parsingName: string | null;
}

interface SystemPrefs {
  animations: boolean;
  iconShape: string;
  theme: string;
  animationSpeed: number;
  magnification: boolean;
  alwaysVisible: boolean;
  iconSize: number;
}

const itemsEl = document.getElementById("items") as HTMLDivElement;
const dockEl = document.getElementById("dock") as HTMLDivElement;
const sliderEl = document.getElementById("slider") as HTMLDivElement;
const tooltipEl = document.getElementById("tooltip") as HTMLDivElement;
const menuEl = document.getElementById("menu") as HTMLDivElement;

let items: DockItem[] = [];
let prefs: SystemPrefs = {
  animations: true,
  iconShape: "circle",
  theme: "auto",
  animationSpeed: 1,
  magnification: true,
  alwaysVisible: false,
  iconSize: 44,
};
const nodes = new Map<string, HTMLDivElement>();

/* ------------------------------------------------------------------ render */

const START_SVG = `
<svg viewBox="0 0 24 24" fill="none" aria-hidden="true">
  <circle cx="12" cy="12" r="9" fill="rgba(255,255,255,0.95)"/>
</svg>`;

const RECYCLE_SVG = `
<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6"
     stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
  <path d="M4 7h16"/>
  <path d="M9 7V5.5A1.5 1.5 0 0 1 10.5 4h3A1.5 1.5 0 0 1 15 5.5V7"/>
  <path d="M6.5 7l.8 11.2A2 2 0 0 0 9.3 20h5.4a2 2 0 0 0 2-1.8L17.5 7"/>
  <path d="M10 11v5M14 11v5"/>
</svg>`;

function buildNode(item: DockItem): HTMLDivElement {
  const el = document.createElement("div");
  el.className = "item";
  el.dataset.id = item.id;
  el.setAttribute("role", "button");
  el.setAttribute("aria-label", item.name);
  el.tabIndex = -1;
  el.draggable = item.pinned;

  const plate = document.createElement("div");
  plate.className = "plate";

  if (item.kind === "start") {
    el.classList.add("start");
    plate.innerHTML = START_SVG;
  } else if (item.icon) {
    const img = document.createElement("img");
    img.src = item.icon;
    img.alt = "";
    plate.appendChild(img);
  } else if (item.kind === "recycle") {
    plate.innerHTML = RECYCLE_SVG;
    plate.style.color = "var(--text-dim)";
  }

  const dot = document.createElement("span");
  dot.className = "dot";

  const prog = document.createElement("span");
  prog.className = "progress";
  prog.appendChild(document.createElement("i"));

  el.append(plate, dot, prog);
  return el;
}

/** Apply the parts of an item that change often, without rebuilding the node. */
function applyState(el: HTMLDivElement, item: DockItem): void {
  el.classList.toggle("running", item.running);
  el.classList.toggle("grouped", item.windowCount > 1);
  el.classList.toggle("attention", item.attention);
  el.setAttribute("aria-label", item.name);

  const hasProgress = item.progressState !== "none";
  el.classList.toggle("has-progress", hasProgress);
  el.classList.toggle("progress-indeterminate", item.progressState === "indeterminate");
  el.classList.toggle("progress-error", item.progressState === "error");
  el.classList.toggle("progress-paused", item.progressState === "paused");
  if (hasProgress && item.progress !== null) {
    const bar = el.querySelector(".progress > i") as HTMLElement | null;
    if (bar) bar.style.width = `${Math.round(item.progress * 100)}%`;
  }
}

/** Icon URL changes are rare; swapping src every tick would flicker. */
function syncIcon(el: HTMLDivElement, item: DockItem): void {
  if (!item.icon) return;
  const img = el.querySelector("img") as HTMLImageElement | null;
  if (img) {
    if (img.src !== item.icon) img.src = item.icon;
    return;
  }
  const plate = el.querySelector(".plate") as HTMLElement;
  plate.textContent = "";
  const fresh = document.createElement("img");
  fresh.src = item.icon;
  fresh.alt = "";
  plate.appendChild(fresh);
}

function render(): void {
  const seen = new Set<string>();
  const frag = document.createDocumentFragment();

  items.forEach((item, i) => {
    // The divider sits immediately before the Recycle Bin, as in the brief.
    if (item.kind === "recycle" && i > 0) {
      const d = document.createElement("div");
      d.className = "divider";
      frag.appendChild(d);
    }
    let el = nodes.get(item.id);
    if (!el) {
      el = buildNode(item);
      nodes.set(item.id, el);
    }
    applyState(el, item);
    syncIcon(el, item);
    frag.appendChild(el);
    seen.add(item.id);
  });

  for (const [id, el] of nodes) {
    if (!seen.has(id)) {
      el.remove();
      nodes.delete(id);
    }
  }

  itemsEl.replaceChildren(frag);
  void resizeWindowToContent();
}

/* ------------------------------------------------------- window sizing */

let lastSize = { w: 0, h: 0 };

/**
 * Extra transparent space reserved ABOVE the dock.
 *
 * A transparent window clips anything drawn outside its rect, so a tooltip or a
 * context menu positioned above the dock is invisible unless the window is tall
 * enough to contain it. The window is also always-on-top, though, so every pixel of
 * headroom is a pixel that stops clicks reaching what is underneath — which would be
 * a real change to how Windows behaves.
 *
 * So headroom is granted only while a popup actually needs it, and given back as soon
 * as it closes. While it is granted the pointer is already on the dock or on the menu,
 * so the reserved strip is exactly where the user is looking anyway.
 */
let headroom = 0;
const TOOLTIP_HEADROOM = 46;

function setHeadroom(px: number): void {
  if (px === headroom) return;
  headroom = px;
  void resizeWindowToContent();
}

/**
 * Size the Win32 window to exactly the dock plus shadow padding.
 *
 * This matters for behaviour, not just looks: the window is always-on-top and
 * transparent, so any area it covers stops clicks reaching whatever is underneath.
 * Keeping it tight to the island means the rest of the screen behaves normally.
 */
async function resizeWindowToContent(): Promise<void> {
  await new Promise(requestAnimationFrame);

  const dockW = Math.ceil(dockEl.offsetWidth) + SHADOW_PAD * 2;
  const popupW = menuEl.classList.contains("show")
    ? Math.ceil(menuEl.offsetWidth) + SHADOW_PAD * 2
    : 0;

  const w = Math.max(dockW, popupW);
  const h = Math.ceil(dockEl.offsetHeight) + SHADOW_PAD * 2 + headroom;
  if (w === lastSize.w && h === lastSize.h) return;
  lastSize = { w, h };

  await invoke("cmd_resize_dock", { cssWidth: w, cssHeight: h });
  // The window moved, so anything anchored to the dock needs re-anchoring.
  await new Promise(requestAnimationFrame);
}

/* ------------------------------------------------------- magnification */

function applyMagnification(hoverIndex: number | null): void {
  const on = prefs.magnification;
  items.forEach((item, i) => {
    const el = nodes.get(item.id);
    if (!el) return;

    let mag = 1;
    let lift = 0;
    if (on && hoverIndex !== null) {
      const d = Math.abs(i - hoverIndex);
      if (d === 0) {
        mag = MAG_HOVER;
        lift = LIFT_HOVER;
      } else if (d === 1) {
        mag = MAG_NEIGHBOUR;
        lift = LIFT_NEIGHBOUR;
      } else if (d === 2) {
        mag = MAG_FAR;
      }
    }
    el.style.setProperty("--mag", String(mag));
    el.style.setProperty("--lift", `${lift}px`);
  });
}

/* ------------------------------------------------------------- tooltip */

let tooltipTimer: number | undefined;

function showTooltip(el: HTMLElement, text: string): void {
  window.clearTimeout(tooltipTimer);
  tooltipTimer = window.setTimeout(async () => {
    tooltipEl.textContent = text;
    // Reserve the strip the tooltip will occupy, then let layout settle before
    // measuring, or the tooltip lands at a stale position.
    setHeadroom(Math.max(headroom, TOOLTIP_HEADROOM));
    await new Promise(requestAnimationFrame);
    tooltipEl.classList.add("show");
    tooltipEl.setAttribute("aria-hidden", "false");

    const r = el.getBoundingClientRect();
    const tw = tooltipEl.offsetWidth;
    const x = Math.min(
      Math.max(4, r.left + r.width / 2 - tw / 2),
      window.innerWidth - tw - 4,
    );
    tooltipEl.style.left = `${Math.round(x)}px`;
    tooltipEl.style.top = `${Math.round(r.top - tooltipEl.offsetHeight - 8)}px`;
  }, 350);
}

function hideTooltip(): void {
  window.clearTimeout(tooltipTimer);
  tooltipEl.classList.remove("show");
  tooltipEl.setAttribute("aria-hidden", "true");
  if (!menuEl.classList.contains("show")) setHeadroom(0);
}

/* ------------------------------------------------------------ popup menu */

type MenuEntry = { label: string; action: () => void; disabled?: boolean };

function closeMenu(): void {
  if (!menuEl.classList.contains("show")) return;
  menuEl.classList.remove("show");
  menuEl.textContent = "";
  setHeadroom(0);
}

async function openMenu(anchor: HTMLElement, entries: MenuEntry[]): Promise<void> {
  menuEl.textContent = "";
  for (const e of entries) {
    const b = document.createElement("button");
    b.className = "menu-item";
    b.textContent = e.label;
    b.disabled = !!e.disabled;
    b.addEventListener("click", () => {
      closeMenu();
      e.action();
    });
    menuEl.appendChild(b);
  }
  menuEl.classList.add("show");

  // Measure once visible, reserve exactly that much headroom, then anchor.
  await new Promise(requestAnimationFrame);
  // The icon sits SHADOW_PAD + the dock's own padding below the window top, so the
  // headroom has to cover the menu plus that offset. Measure after a frame so the
  // height reflects the real laid-out entries, not a pre-layout guess.
  setHeadroom(menuEl.offsetHeight + SHADOW_PAD + 16);
  await resizeWindowToContent();

  const r = anchor.getBoundingClientRect();
  const mw = menuEl.offsetWidth;
  const x = Math.min(
    Math.max(4, r.left + r.width / 2 - mw / 2),
    window.innerWidth - mw - 4,
  );
  menuEl.style.left = `${Math.round(x)}px`;
  // Clamp to the window: a menu that renders past the top edge is silently cut off,
  // because a transparent window clips rather than overflowing.
  const top = Math.max(4, r.top - menuEl.offsetHeight - 8);
  menuEl.style.top = `${Math.round(top)}px`;
}

document.addEventListener("pointerdown", (e) => {
  if (!menuEl.contains(e.target as Node)) closeMenu();
});

/* --------------------------------------------------------- interaction */

function itemFor(target: EventTarget | null): { item: DockItem; el: HTMLDivElement } | null {
  const el = (target as HTMLElement | null)?.closest(".item") as HTMLDivElement | null;
  if (!el) return null;
  const item = items.find((it) => it.id === el.dataset.id);
  return item ? { item, el } : null;
}

itemsEl.addEventListener("pointermove", (e) => {
  const hit = itemFor(e.target);
  applyMagnification(hit ? items.indexOf(hit.item) : null);
  if (hit) showTooltip(hit.el, hit.item.name);
  else hideTooltip();
});

itemsEl.addEventListener("pointerleave", () => {
  applyMagnification(null);
  hideTooltip();
});

/**
 * Left click. The Windows taskbar's rules, exactly:
 *   - not running          -> launch
 *   - one window, focused  -> minimise
 *   - one window, else     -> activate
 *   - several windows      -> show the chooser
 */
async function onClick(item: DockItem, el: HTMLDivElement): Promise<void> {
  hideTooltip();

  if (item.kind === "start") {
    // PHASE 3 owns the launcher. Until then the Start button must still do
    // something correct rather than nothing, so it opens the real Start menu.
    await invoke("cmd_open_start_menu");
    return;
  }

  if (item.windowCount === 0) {
    if (item.parsingName) await invoke("cmd_dock_launch", { parsingName: item.parsingName });
    return;
  }

  if (item.windowCount === 1) {
    await invoke("cmd_dock_activate", { hwnd: item.hwnds[0] });
    return;
  }

  const titles = await invoke<string[]>("cmd_window_titles", { hwnds: item.hwnds });
  await openMenu(
    el,
    item.hwnds.map((h, i) => ({
      label: titles[i] || item.name,
      action: () => void invoke("cmd_dock_activate", { hwnd: h }),
    })),
  );
}

itemsEl.addEventListener("click", (e) => {
  const hit = itemFor(e.target);
  if (hit) void onClick(hit.item, hit.el);
});

/** Middle click opens a new instance, like the Windows taskbar. */
itemsEl.addEventListener("auxclick", (e) => {
  if (e.button !== 1) return;
  const hit = itemFor(e.target);
  if (hit?.item.parsingName) {
    void invoke("cmd_dock_launch", { parsingName: hit.item.parsingName });
  }
});

/**
 * Right-clicking the dock itself (not an icon) opens the app menu.
 *
 * The tray icon carries the same menu, but it lives in the notification area, which
 * is part of the Windows taskbar we hide - so this, and the top bar's own context
 * menu, are the reachable copies while the UI layer is running.
 */
dockEl.addEventListener("contextmenu", (e) => {
  if (itemFor(e.target)) return; // an icon handles its own menu
  e.preventDefault();
  void openMenu(dockEl, [
    { label: "Settings…", action: () => void invoke("cmd_open_settings") },
    { label: "Restart UI", action: () => void invoke("cmd_app_restart") },
    {
      label: "Quit (restores the Windows taskbar)",
      action: () => void invoke("cmd_app_quit"),
    },
  ]);
});

itemsEl.addEventListener("contextmenu", (e) => {
  e.preventDefault();
  const hit = itemFor(e.target);
  if (!hit) return;
  const { item, el } = hit;
  if (item.kind === "start") return;

  const entries: MenuEntry[] = [];

  if (item.parsingName) {
    entries.push({
      label: item.name,
      action: () => void invoke("cmd_dock_launch", { parsingName: item.parsingName! }),
    });
  }

  if (item.kind === "app") {
    entries.push(
      item.pinned
        ? { label: "Unpin from dock", action: () => void invoke("cmd_dock_unpin", { id: item.id }).then(refresh) }
        : {
            label: "Pin to dock",
            action: () =>
              void invoke("cmd_dock_pin", {
                name: item.name,
                parsingName: item.parsingName,
              }).then(refresh),
            disabled: !item.parsingName,
          },
    );
  }

  if (item.windowCount > 0) {
    entries.push({
      label: item.windowCount > 1 ? `Close all ${item.windowCount} windows` : "Close window",
      action: () => {
        for (const h of item.hwnds) void invoke("cmd_dock_close_window", { hwnd: h });
      },
    });
  }

  if (entries.length) void openMenu(el, entries);
});

/* ----------------------------------------------------- drag to reorder */

let dragId: string | null = null;

itemsEl.addEventListener("dragstart", (e) => {
  const hit = itemFor(e.target);
  if (!hit || !hit.item.pinned) {
    e.preventDefault();
    return;
  }
  dragId = hit.item.id;
  hit.el.classList.add("dragging");
  e.dataTransfer?.setData("text/plain", hit.item.id);
});

itemsEl.addEventListener("dragover", (e) => {
  if (!dragId) return;
  e.preventDefault();
  const hit = itemFor(e.target);
  if (!hit || !hit.item.pinned || hit.item.id === dragId) return;

  const from = items.findIndex((i) => i.id === dragId);
  const to = items.findIndex((i) => i.id === hit.item.id);
  if (from < 0 || to < 0) return;

  const [moved] = items.splice(from, 1);
  items.splice(to, 0, moved);
  render();
});

itemsEl.addEventListener("dragend", () => {
  if (!dragId) return;
  nodes.get(dragId)?.classList.remove("dragging");
  dragId = null;
  void invoke("cmd_dock_set_pinned_order", {
    ids: items.filter((i) => i.pinned).map((i) => i.id),
  });
});

/* --------------------------------------------------------- auto-hide */

// Must match the `hidden` class in dock.html and the Rust sampler's initial state.
let revealed = false;

function setRevealed(next: boolean): void {
  if (next === revealed) return;
  revealed = next;
  sliderEl.classList.toggle("hidden", !next);
  if (!next) {
    hideTooltip();
    closeMenu();
  }
}

/* ------------------------------------------------------------- startup */

function applyPrefs(p: SystemPrefs): void {
  prefs = p;
  const root = document.documentElement;
  root.dataset.animations = p.animations ? "on" : "off";
  root.dataset.iconShape = p.iconShape || "circle";
  if (p.theme === "light" || p.theme === "dark") root.dataset.theme = p.theme;
  else delete root.dataset.theme;
  root.style.setProperty("--anim-scale", p.animations ? String(p.animationSpeed) : "0");
  // The icon-size setting existed from PHASE 1 but nothing read it until the
  // settings window could change it.
  root.style.setProperty("--dock-icon", `${p.iconSize}px`);
}

/** Only re-render when something actually changed, to keep idle CPU near zero. */
let lastSignature = "";

async function refresh(): Promise<void> {
  try {
    const next = await invoke<DockItem[]>("cmd_dock_items");
    // Never reorder under the user's fingers mid-drag.
    if (dragId) return;

    const sig = JSON.stringify(
      next.map((i) => [i.id, i.running, i.windowCount, i.attention, i.progressState, i.progress, i.name, !!i.icon]),
    );
    if (sig === lastSignature) return;
    lastSignature = sig;

    items = next;
    render();
  } catch (err) {
    console.error("dock refresh failed", err);
  }
}


/* ===========================================================================
 * The tray
 *
 * Network, Bluetooth, sound, battery, the clock and the shelf — everything the top
 * bar used to carry, at the right-hand end of the dock's own pill.
 *
 * It renders inside `.dock`, so `resizeWindowToContent()` already covers it and no
 * hit-test region is involved. That matters: the top bar's region update waited on
 * a `requestAnimationFrame` in a window clipped to nothing, and a window that is
 * not composited has its frames throttled to roughly one a second — which is
 * exactly the delay before it appeared. See D-038.
 * =========================================================================== */

const SVG = (d: string) =>
  `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">${d}</svg>`;

const TRAY_ICON = {
  wifi: SVG(`<path d="M2.5 8.5a15 15 0 0 1 19 0"/><path d="M5.5 12a10.5 10.5 0 0 1 13 0"/><path d="M8.5 15.3a6 6 0 0 1 7 0"/><circle cx="12" cy="19" r="1.2" fill="currentColor" stroke="none"/>`),
  wifiOff: SVG(`<path d="M8.5 15.3a6 6 0 0 1 7 0"/><circle cx="12" cy="19" r="1.2" fill="currentColor" stroke="none"/><path d="M3 3l18 18"/>`),
  bt: SVG(`<path d="M7 7.5 17 16.5 12 21V3l5 4.5L7 16.5"/>`),
  vol: SVG(`<path d="M4 9.5h3L11 6v12l-4-3.5H4z" fill="currentColor"/><path d="M15.5 9a4 4 0 0 1 0 6"/>`),
  volMute: SVG(`<path d="M4 9.5h3L11 6v12l-4-3.5H4z" fill="currentColor"/><path d="M16 9.5l5 5M21 9.5l-5 5"/>`),
  shelf: SVG(`<path d="M4 7.5h16M4 7.5v10a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2v-10"/><path d="M9.5 4.5h5l1.5 3h-8z"/>`),
};

/** Battery drawn as a fill, so the level reads without a number. */
function battSvg(pct: number, charging: boolean): string {
  const w = Math.max(0, Math.min(1, pct / 100)) * 15.5;
  const fill = charging ? "var(--accent)" : pct <= 20 ? "#e5484d" : "currentColor";
  return `<svg viewBox="0 0 32 24" fill="none" stroke="currentColor" stroke-width="1.6">
    <rect x="1.5" y="6" width="22" height="12" rx="3.5"/>
    <path d="M25.5 10.5v3" stroke-linecap="round"/>
    <rect x="3.7" y="8.2" width="${w}" height="7.6" rx="2" fill="${fill}" stroke="none"/>
    ${charging ? '<path d="M13.6 8.6 10.4 13h3.2l-1 3.2L16 11.6h-3z" fill="var(--accent-text)" stroke="none"/>' : ""}
  </svg>`;
}

interface TrayStatus {
  battery: { percent: number | null; charging: boolean; present: boolean };
  volume: { level: number; muted: boolean; available: boolean };
  network: { connected: boolean; internet: boolean };
  bluetooth: { present: boolean; on: boolean };
  ssid: string | null;
}

const trayEl = document.getElementById("tray") as HTMLDivElement;
const traySepEl = document.getElementById("tray-sep") as HTMLDivElement;
const shelfBtn = document.getElementById("tray-shelf") as HTMLButtonElement;

/** Quick-settings panel size. Must match the content in panel.html. */
const QUICK_W = 348;
const QUICK_H = 478;
const CAL_W = 300;
const CAL_H = 330;

function openPanelAbove(kind: string, anchor: HTMLElement, width: number, height: number): void {
  const r = anchor.getBoundingClientRect();
  void invoke("cmd_open_panel", {
    kind,
    anchorX: r.left + r.width / 2,
    width,
    height,
    // The bar is at the bottom now, so its flyouts grow upward.
    above: true,
  });
}

function tickClock(): void {
  const now = new Date();
  document.getElementById("clock-time")!.textContent = now.toLocaleTimeString([], {
    hour: "numeric",
    minute: "2-digit",
  });
  document.getElementById("clock-date")!.textContent = now.toLocaleDateString([], {
    day: "numeric",
    month: "short",
  });
}

function paintTray(s: TrayStatus): void {
  const wifi = document.getElementById("ico-wifi")!;
  wifi.innerHTML = s.network.connected ? TRAY_ICON.wifi : TRAY_ICON.wifiOff;
  wifi.classList.toggle("off", !s.network.connected);
  // The network you are on, not the adapter's name — same as the Windows tile.
  wifi.setAttribute("data-ssid", s.ssid ?? "");

  const bt = document.getElementById("ico-bt")!;
  bt.innerHTML = TRAY_ICON.bt;
  bt.hidden = !s.bluetooth.present;
  bt.classList.toggle("off", !s.bluetooth.on);

  const vol = document.getElementById("ico-vol")!;
  vol.innerHTML = s.volume.muted || s.volume.level === 0 ? TRAY_ICON.volMute : TRAY_ICON.vol;

  const batt = document.getElementById("ico-batt")!;
  batt.hidden = !s.battery.present;
  if (s.battery.present) batt.innerHTML = battSvg(s.battery.percent ?? 0, s.battery.charging);

  const label = [
    s.ssid ? `Wi-Fi: ${s.ssid}` : s.network.connected ? "Connected" : "No network",
    s.bluetooth.present ? `Bluetooth ${s.bluetooth.on ? "on" : "off"}` : null,
    `Volume ${s.volume.level}%`,
    s.battery.present ? `Battery ${s.battery.percent ?? 0}%` : null,
  ]
    .filter(Boolean)
    .join(" · ");
  document.getElementById("tray-status")!.setAttribute("aria-label", label);
}

async function refreshTray(): Promise<void> {
  try {
    const before = trayEl.offsetWidth;
    paintTray(await invoke<TrayStatus>("cmd_status"));
    // Hiding the battery or the Bluetooth glyph changes the pill's width, and the
    // window is sized to the pill.
    if (trayEl.offsetWidth !== before) await resizeWindowToContent();
  } catch {
    /* a failed poll just leaves the last paint up */
  }
}

/**
 * Files dragged onto the dock get parked on the shelf.
 *
 * Tauri reports the native drop target's events for this window, so the whole dock
 * is the target; the shelf button lights up to say where they are going. This used
 * to be the island's job.
 */
async function wireDrops(): Promise<void> {
  try {
    const { getCurrentWebview } = await import("@tauri-apps/api/webview");
    await getCurrentWebview().onDragDropEvent(async (e) => {
      const t = e.payload.type;
      if (t === "enter" || t === "over") {
        shelfBtn.hidden = false;
        shelfBtn.classList.add("drop");
        await resizeWindowToContent();
      } else if (t === "drop") {
        shelfBtn.classList.remove("drop");
        const paths = (e.payload as { paths?: string[] }).paths ?? [];
        if (paths.length) await invoke("cmd_shelf_add", { paths });
        await refreshShelf();
      } else {
        shelfBtn.classList.remove("drop");
        await refreshShelf();
      }
    });
  } catch (err) {
    await invoke("cmd_log", { msg: `dock drag-drop listener failed: ${err}` });
  }
}

async function refreshShelf(): Promise<void> {
  try {
    const items = await invoke<unknown[]>("cmd_shelf");
    shelfBtn.hidden = items.length === 0;
    document.getElementById("shelf-count")!.textContent = String(items.length);
  } catch {
    shelfBtn.hidden = true;
  }
}

function wireTray(): void {
  trayEl.hidden = false;
  traySepEl.hidden = false;
  document.getElementById("ico-shelf")!.innerHTML = TRAY_ICON.shelf;

  document
    .getElementById("tray-status")!
    .addEventListener("click", (e) =>
      openPanelAbove("quick", e.currentTarget as HTMLElement, QUICK_W, QUICK_H),
    );
  document
    .getElementById("tray-clock")!
    .addEventListener("click", (e) =>
      openPanelAbove("calendar", e.currentTarget as HTMLElement, CAL_W, CAL_H),
    );
  shelfBtn.addEventListener("click", (e) =>
    openPanelAbove("shelf", e.currentTarget as HTMLElement, 330, 260),
  );

  tickClock();
  // Tick on the minute rather than every second: the clock shows minutes.
  const schedule = () => {
    const ms = 60_000 - (Date.now() % 60_000);
    window.setTimeout(() => {
      tickClock();
      schedule();
    }, ms + 50);
  };
  schedule();

  void wireDrops();
  void refreshTray();
  void refreshShelf();
  window.setInterval(() => {
    void refreshTray();
    void refreshShelf();
  }, 2000);
}

async function main(): Promise<void> {
  try {
    applyPrefs(await invoke<SystemPrefs>("cmd_system_prefs"));
  } catch {
    applyPrefs(prefs);
  }

  await refresh();

  try {
    await listen("dock:reveal", () => setRevealed(true));
    await listen("dock:conceal", () => setRevealed(false));
  } catch (e) {
    // Without these the dock can never auto-hide, so make the failure loud in the
    // log rather than leaving a permanently visible dock and no explanation.
    await invoke("cmd_log", { msg: `listen() FAILED, auto-hide disabled: ${e}` });
  }

  // Sync to the sampler's actual state. Any reveal/conceal emitted while this page
  // was still loading is gone, so without this the dock can sit permanently
  // concealed when the pointer was already at the bottom edge at launch.
  try {
    setRevealed(await invoke<boolean>("cmd_dock_revealed"));
  } catch {
    /* keep the concealed default */
  }
  await listen("dock:prefs", async () => {
    applyPrefs(await invoke<SystemPrefs>("cmd_system_prefs"));
  });

  wireTray();
  await resizeWindowToContent();

  window.setInterval(() => void refresh(), REFRESH_MS);
  await getCurrentWindow().show();
  // Re-enable transitions only after the concealed start state has been painted.
  requestAnimationFrame(() => sliderEl.classList.remove("no-anim"));
}

void main();
