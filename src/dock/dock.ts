/**
 * Dock — visual replacement for the Windows taskbar.
 *
 * Division of labour with the Rust side:
 *   - Rust owns anything Win32: which apps exist, their icons, focus, and the
 *     cursor-at-the-bottom-edge test that drives auto-hide.
 *   - This file owns layout and motion only. The Win32 window is resized once when
 *     the item list changes and otherwise never moves; the slide animation is a CSS
 *     transform inside a stationary window, because moving a real window every frame
 *     is what makes docks stutter.
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

type ItemKind = "start" | "app" | "recycle";

interface DockItem {
  id: string;
  kind: ItemKind;
  name: string;
  /** Data URL of the extracted icon, or null for drawn items (Start). */
  icon: string | null;
  running: boolean;
  /** More than one top-level window -> grouped, wider indicator. */
  windowCount: number;
  attention: boolean;
  /** 0..1, or null when the app reports no progress. */
  progress: number | null;
  progressState: "none" | "normal" | "indeterminate" | "error" | "paused";
  pinned: boolean;
}

const itemsEl = document.getElementById("items") as HTMLDivElement;
const dockEl = document.getElementById("dock") as HTMLDivElement;
const sliderEl = document.getElementById("slider") as HTMLDivElement;
const tooltipEl = document.getElementById("tooltip") as HTMLDivElement;

let items: DockItem[] = [];
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
  } else {
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
 * Size the Win32 window to exactly the dock plus shadow padding.
 *
 * This matters for behaviour, not just looks: the window is always-on-top and
 * transparent, so any area it covers stops clicks reaching the desktop underneath.
 * Keeping it tight to the island means the rest of the screen behaves normally.
 */
async function resizeWindowToContent(): Promise<void> {
  // Let layout settle so offsetWidth reflects the new children.
  await new Promise(requestAnimationFrame);

  const w = Math.ceil(dockEl.offsetWidth) + SHADOW_PAD * 2;
  const h = Math.ceil(dockEl.offsetHeight) + SHADOW_PAD * 2;
  if (w === lastSize.w && h === lastSize.h) return;
  lastSize = { w, h };

  await invoke("cmd_resize_dock", { cssWidth: w, cssHeight: h });
}

/* ------------------------------------------------------- magnification */

/**
 * Scale each icon by its distance from the pointer.
 *
 * Distance is measured in "icons away" rather than pixels so the falloff looks the
 * same whatever the configured dock size.
 */
function applyMagnification(hoverIndex: number | null): void {
  items.forEach((item, i) => {
    const el = nodes.get(item.id);
    if (!el) return;

    let mag = 1;
    let lift = 0;
    if (hoverIndex !== null) {
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
  tooltipTimer = window.setTimeout(() => {
    tooltipEl.textContent = text;
    tooltipEl.classList.add("show");
    tooltipEl.setAttribute("aria-hidden", "false");

    // Measure after the text is in, then centre above the icon and keep it inside
    // the window so it is never clipped by the transparent window edge.
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
}

/* --------------------------------------------------------- interaction */

function indexOfNode(target: EventTarget | null): number | null {
  const el = (target as HTMLElement | null)?.closest(".item") as HTMLDivElement | null;
  if (!el) return null;
  const i = items.findIndex((it) => it.id === el.dataset.id);
  return i === -1 ? null : i;
}

itemsEl.addEventListener("pointermove", (e) => {
  const i = indexOfNode(e.target);
  applyMagnification(i);
  if (i !== null) {
    const el = nodes.get(items[i].id);
    if (el) showTooltip(el, items[i].name);
  } else {
    hideTooltip();
  }
});

itemsEl.addEventListener("pointerleave", () => {
  applyMagnification(null);
  hideTooltip();
});

/* --------------------------------------------------------- auto-hide */

let revealed = true;

function setRevealed(next: boolean): void {
  if (next === revealed) return;
  revealed = next;
  sliderEl.classList.toggle("hidden", !next);
  if (!next) hideTooltip();
}

/* ------------------------------------------------------------- startup */

/** Mirror Windows' theme and animation settings onto <html>. */
async function applySystemPreferences(): Promise<void> {
  try {
    const prefs = await invoke<{ animations: boolean; iconShape: string }>(
      "cmd_system_prefs",
    );
    document.documentElement.dataset.animations = prefs.animations ? "on" : "off";
    document.documentElement.dataset.iconShape = prefs.iconShape;
  } catch {
    // A missing backend command must not leave the dock unstyled.
    document.documentElement.dataset.animations = "on";
    document.documentElement.dataset.iconShape = "circle";
  }
}

/** Placeholder content, replaced by real taskbar data in the next step. */
function placeholderItems(): DockItem[] {
  const blank = (id: string, name: string, running: boolean, n = 1): DockItem => ({
    id,
    kind: "app",
    name,
    icon: null,
    running,
    windowCount: n,
    attention: false,
    progress: null,
    progressState: "none",
    pinned: true,
  });
  return [
    {
      id: "start",
      kind: "start",
      name: "Start",
      icon: null,
      running: false,
      windowCount: 0,
      attention: false,
      progress: null,
      progressState: "none",
      pinned: true,
    },
    blank("a1", "File Explorer", true),
    blank("a2", "Edge", true, 3),
    blank("a3", "Terminal", false),
    blank("a4", "Settings", false),
    {
      id: "recycle",
      kind: "recycle",
      name: "Recycle Bin",
      icon: null,
      running: false,
      windowCount: 0,
      attention: false,
      progress: null,
      progressState: "none",
      pinned: true,
    },
  ];
}

async function main(): Promise<void> {
  document.title = "macui dock [js-started]";
  await applySystemPreferences();

  items = placeholderItems();
  render();

  await listen("dock:reveal", () => setRevealed(true));
  await listen("dock:conceal", () => setRevealed(false));

  await getCurrentWindow().show();
  document.title = "macui dock [ok items=" + items.length + " w=" + lastSize.w + " h=" + lastSize.h + "]";
}

void main();
