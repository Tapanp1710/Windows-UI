/**
 * The island — the top bar's centre pill, which grows.
 *
 * It is two things in one place:
 *
 *   - **What is going on.** Collapsed it shows the album art of whatever is playing
 *     with a small equaliser, and it expands by itself for a few seconds when the
 *     volume changes, the charger goes in or out, or the track changes.
 *   - **A shelf.** Drop files, folders or images on it and they stay parked there
 *     until you drag them somewhere else. Items are *references*, not copies, so
 *     parking something costs nothing and dragging it off hands over the real path.
 *
 * Deliberately not a clock: that moved to the status pill, where it is always
 * readable, because the island is for things that change.
 *
 * Dragging an item OUT cannot be done from the web side - HTML drag-and-drop has no
 * way to hand a filesystem path to Explorer - so a drag that leaves the island hands
 * over to a real Win32 OLE drag in Rust (win/dragout.rs).
 */

import { invoke } from "@tauri-apps/api/core";

export interface IslandStatus {
  battery: { percent: number | null; charging: boolean; present: boolean };
  volume: { level: number; muted: boolean };
  media: { active: boolean; title: string; artist: string; playing: boolean; source: string };
}

interface ShelfItem {
  id: string;
  name: string;
  kind: string;
  icon: string | null;
}

const $ = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;

const SVG = {
  prev: `<svg viewBox="0 0 24 24" fill="currentColor"><path d="M7 6h2v12H7zM20 6v12l-9-6z"/></svg>`,
  next: `<svg viewBox="0 0 24 24" fill="currentColor"><path d="M15 6h2v12h-2zM4 6l9 6-9 6z"/></svg>`,
  play: `<svg viewBox="0 0 24 24" fill="currentColor"><path d="M8 5v14l11-7z"/></svg>`,
  pause: `<svg viewBox="0 0 24 24" fill="currentColor"><path d="M7 5h4v14H7zM13 5h4v14h-4z"/></svg>`,
  note: `<svg viewBox="0 0 24 24" fill="currentColor"><path d="M9 18V6l10-2v12"/><circle cx="7" cy="18" r="2.4"/><circle cx="17" cy="16" r="2.4"/></svg>`,
  vol: `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M4 9.5h3L11 6v12l-4-3.5H4z" fill="currentColor"/><path d="M15.5 9a4 4 0 0 1 0 6"/></svg>`,
  mute: `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M4 9.5h3L11 6v12l-4-3.5H4z" fill="currentColor"/><path d="M16 9.5l5 5M21 9.5l-5 5"/></svg>`,
  plug: `<svg viewBox="0 0 24 24" fill="currentColor"><path d="M13 2 4.5 13.5H11l-1 8.5 8.5-11.5H12z"/></svg>`,
  batt: `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7"><rect x="2" y="7.5" width="17" height="9" rx="2.5"/><path d="M21 11v2" stroke-linecap="round"/></svg>`,
  folder: `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linejoin="round"><path d="M3 7.5A1.5 1.5 0 0 1 4.5 6h4L11 8.5h8.5A1.5 1.5 0 0 1 21 10v7.5a1.5 1.5 0 0 1-1.5 1.5h-15A1.5 1.5 0 0 1 3 17.5z"/></svg>`,
  file: `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linejoin="round"><path d="M6 3.5h7L19 9v11.5H6z"/><path d="M13 3.5V9h6"/></svg>`,
  shelf: `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linejoin="round"><path d="M3 13h5l1.5 2.5h5L16 13h5"/><path d="M4.5 13 6 5.5h12L19.5 13v5a1.5 1.5 0 0 1-1.5 1.5H6A1.5 1.5 0 0 1 4.5 18z"/></svg>`,
};

/** How long an unprompted expansion stays up. */
const ACTIVITY_MS = 2600;

const island = $<HTMLDivElement>("island");
const body = $<HTMLDivElement>("island-body");
const lead = $<HTMLSpanElement>("island-lead");
const eq = $<HTMLSpanElement>("island-eq");
const labelEl = $<HTMLSpanElement>("island-label");
const badgeEl = $<HTMLSpanElement>("island-badge");

let enabled = true;
let hovered = false;
let dragging = false;
let activityUntil = 0;
let activityTimer: number | undefined;
let lastArtTitle = "\u0000";
let shelf: ShelfItem[] = [];
let onStateChange: (() => void) | undefined;

/** Previous sample, so a change can be spotted rather than a value reported. */
let prev: { volume: number; charging: boolean; title: string } | null = null;

/* ------------------------------------------------------------------ state */

function setState(): void {
  if (!enabled) {
    island.dataset.state = "idle";
    return;
  }
  const activity = Date.now() < activityUntil;
  const next = dragging ? "drop" : hovered ? "open" : activity ? "activity" : "idle";
  if (island.dataset.state === next) return;
  island.dataset.state = next;
  body.setAttribute("aria-hidden", next === "idle" ? "true" : "false");
  onStateChange?.();
}

export function setOnStateChange(f: () => void): void {
  onStateChange = f;
}

export function isOpen(): boolean {
  return island.dataset.state !== "idle";
}

/** Called by the top bar while a file is being dragged near the bar. */
export function setDragging(on: boolean): void {
  dragging = on;
  setState();
}

/* --------------------------------------------------------------- activity */

function showActivity(icon: string, text: string, fill: number | null): void {
  $("act-icon").innerHTML = icon;
  $("act-text").textContent = text;
  const bar = $<HTMLElement>("act-bar");
  bar.style.width = fill === null ? "0" : `${fill}%`;
  $<HTMLElement>("np-activity").hidden = false;

  activityUntil = Date.now() + ACTIVITY_MS;
  window.clearTimeout(activityTimer);
  activityTimer = window.setTimeout(() => {
    $<HTMLElement>("np-activity").hidden = true;
    setState();
  }, ACTIVITY_MS);
  setState();
}

/* ------------------------------------------------------------ now playing */

async function paintArt(title: string): Promise<void> {
  if (title === lastArtTitle) return;
  lastArtTitle = title;

  const art = $<HTMLElement>("np-art");
  lead.replaceChildren();
  lead.classList.remove("on");

  if (!title) {
    art.innerHTML = SVG.note;
    return;
  }
  try {
    const url = await invoke<string | null>("cmd_media_art");
    if (url) {
      const big = document.createElement("img");
      big.src = url;
      big.alt = "";
      art.replaceChildren(big);

      const small = document.createElement("img");
      small.src = url;
      small.alt = "";
      lead.replaceChildren(small);
      lead.classList.add("on");
      return;
    }
  } catch {
    /* fall through to the placeholder */
  }
  art.innerHTML = SVG.note;
}

/** What the collapsed pill says, which depends on what there is to say. */
function paintCollapsed(media: IslandStatus["media"]): void {
  eq.classList.toggle("on", media.playing);

  if (media.active && media.title) {
    labelEl.textContent = media.title;
  } else if (shelf.length) {
    labelEl.textContent = "Shelf";
  } else {
    labelEl.textContent = "Shelf";
  }

  badgeEl.hidden = shelf.length === 0;
  badgeEl.textContent = String(shelf.length);
}

export function paint(s: IslandStatus): void {
  if (!enabled) return;

  const m = s.media;
  $("np-title").textContent = m.active && m.title ? m.title : "Nothing playing";
  $("np-artist").textContent = m.active ? [m.artist, m.source].filter(Boolean).join(" · ") : "";
  $("np-play").innerHTML = m.playing ? SVG.pause : SVG.play;
  void paintArt(m.active ? m.title : "");
  paintCollapsed(m);

  // Spot the changes worth an unprompted glance.
  const now = { volume: s.volume.level, charging: s.battery.charging, title: m.title };
  if (prev) {
    if (now.volume !== prev.volume) {
      showActivity(
        s.volume.muted || now.volume === 0 ? SVG.mute : SVG.vol,
        `${now.volume}%`,
        now.volume,
      );
    } else if (now.charging !== prev.charging) {
      showActivity(
        now.charging ? SVG.plug : SVG.batt,
        now.charging ? "Charging" : "On battery",
        s.battery.percent,
      );
    } else if (now.title && now.title !== prev.title) {
      showActivity(SVG.note, now.title.slice(0, 24), null);
    }
  }
  prev = now;
}

/* ------------------------------------------------------------------ shelf */

function renderShelf(): void {
  const strip = $("shelf-strip");
  strip.replaceChildren();

  for (const item of shelf) {
    const chip = document.createElement("div");
    chip.className = "chip";
    chip.draggable = false;
    chip.dataset.id = item.id;

    const thumb = document.createElement("div");
    thumb.className = "thumb";
    if (item.icon) {
      const img = document.createElement("img");
      img.src = item.icon;
      img.alt = "";
      thumb.replaceChildren(img);
    } else {
      thumb.innerHTML = item.kind === "folder" ? SVG.folder : SVG.file;
    }

    const name = document.createElement("span");
    name.textContent = item.name;

    const x = document.createElement("button");
    x.className = "drop-x";
    x.type = "button";
    x.textContent = "×";
    x.setAttribute("aria-label", `Remove ${item.name}`);
    x.addEventListener("click", async (e) => {
      e.stopPropagation();
      shelf = await invoke<ShelfItem[]>("cmd_shelf_remove", { id: item.id });
      renderShelf();
    });

    chip.append(thumb, name, x);
    wireChipDrag(chip, item);
    strip.appendChild(chip);
  }

  $<HTMLElement>("shelf-hint").hidden = shelf.length > 0;
  badgeEl.hidden = shelf.length === 0;
  badgeEl.textContent = String(shelf.length);
  onStateChange?.();
}

/**
 * A press that turns into a movement hands over to a real Windows drag.
 *
 * `DoDragDrop` takes the mouse for the rest of the gesture, so from the page's point
 * of view the pointer simply stops reporting - which is exactly right, because the
 * drag now belongs to the system.
 *
 * The move and release are watched on the DOCUMENT, not on the chip. A chip is about
 * forty pixels tall, so the very first movement of a drag is usually already off it;
 * listening on the chip meant the drag never started, and pointer capture is not
 * dependable enough here to lean on.
 */
let pending: { id: string; x: number; y: number } | null = null;

function wireChipDrag(chip: HTMLElement, item: ShelfItem): void {
  chip.addEventListener("pointerdown", (e) => {
    if ((e.target as HTMLElement).closest(".drop-x")) return;
    pending = { id: item.id, x: e.clientX, y: e.clientY };
  });
}

function wireShelfGestures(): void {
  document.addEventListener("pointermove", (e) => {
    if (!pending) return;
    if (Math.abs(e.clientX - pending.x) + Math.abs(e.clientY - pending.y) < 6) return;
    const { id } = pending;
    pending = null;
    void invoke("cmd_shelf_drag_out", { ids: [id] });
  });

  document.addEventListener("pointerup", () => {
    if (!pending) return;
    const { id } = pending;
    pending = null;
    // A press that never moved is a click: open it, the way double-clicking it in
    // Explorer would.
    void invoke("cmd_shelf_open", { id });
  });
}

export async function addToShelf(paths: string[]): Promise<void> {
  if (!paths.length) return;
  shelf = await invoke<ShelfItem[]>("cmd_shelf_add", { paths });
  renderShelf();
}

async function loadShelf(): Promise<void> {
  try {
    shelf = await invoke<ShelfItem[]>("cmd_shelf");
  } catch {
    shelf = [];
  }
  renderShelf();
}

/* --------------------------------------------------------------- startup */

export function initIsland(on: boolean): void {
  enabled = on;
  island.hidden = false;

  $("np-prev").innerHTML = SVG.prev;
  $("np-next").innerHTML = SVG.next;
  $("np-play").innerHTML = SVG.play;
  $("np-art").innerHTML = SVG.note;

  for (const [id, action] of [
    ["np-prev", "previous"],
    ["np-next", "next"],
    ["np-play", "toggle"],
  ] as const) {
    $(id).addEventListener("click", (e) => {
      // The pill behind this opens the calendar; a transport button must not.
      e.stopPropagation();
      void invoke("cmd_media_control", { action });
    });
  }

  // Hover anywhere on the island - pill or body - keeps it open, so moving the
  // pointer from the pill down to the controls does not collapse it mid-reach.
  island.addEventListener("pointerenter", () => {
    hovered = true;
    setState();
  });
  island.addEventListener("pointerleave", () => {
    hovered = false;
    setState();
  });

  wireShelfGestures();
  void loadShelf();
  setState();
}

export function setIslandEnabled(on: boolean): void {
  enabled = on;
  setState();
}
