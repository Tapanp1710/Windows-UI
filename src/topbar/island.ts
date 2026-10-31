/**
 * The island — the top bar's centre pill, which grows.
 *
 * Collapsed it is the clock. Hovering expands it to whatever is playing. It also
 * expands on its own for a few seconds when something happens worth a glance: the
 * volume changes, the charger goes in or out, the track changes.
 *
 * It lives inside the top bar's window, which is deliberately much taller than the
 * bar itself, so expanding is a CSS transition rather than a Win32 window resize.
 * The window's clickable area still only covers the pills, except while the island
 * is open, when it covers the body too - so an island that is not open costs the
 * screen underneath nothing.
 */

import { invoke } from "@tauri-apps/api/core";

export interface IslandStatus {
  battery: { percent: number | null; charging: boolean; present: boolean };
  volume: { level: number; muted: boolean };
  media: { active: boolean; title: string; artist: string; playing: boolean; source: string };
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
};

/** How long an unprompted expansion stays up. */
const ACTIVITY_MS = 2600;

const island = $<HTMLDivElement>("island");
const body = $<HTMLDivElement>("island-body");
const lead = $<HTMLSpanElement>("island-lead");
const trail = $<HTMLSpanElement>("island-trail");

let enabled = true;
let hovered = false;
let activityUntil = 0;
let activityTimer: number | undefined;
let lastArtTitle = "";

/** Previous sample, so a change can be spotted rather than a value reported. */
let prev: { volume: number; charging: boolean; title: string } | null = null;

/* ------------------------------------------------------------------ state */

function setState(): void {
  if (!enabled) {
    island.dataset.state = "idle";
    return;
  }
  const activity = Date.now() < activityUntil;
  const next = hovered ? "open" : activity ? "activity" : "idle";
  if (island.dataset.state === next) return;
  island.dataset.state = next;
  body.setAttribute("aria-hidden", next === "idle" ? "true" : "false");
  onStateChange?.();
}

/** Set by the top bar so the hit-test region can follow the island as it grows. */
export let onStateChange: (() => void) | undefined;
export function setOnStateChange(f: () => void): void {
  onStateChange = f;
}

export function isOpen(): boolean {
  return island.dataset.state !== "idle";
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
  lead.innerHTML = "";
  lead.classList.remove("on");

  if (!title) {
    art.innerHTML = SVG.note;
    art.style.display = "flex";
    return;
  }
  try {
    const url = await invoke<string | null>("cmd_media_art");
    if (url) {
      art.innerHTML = `<img src="${url}" alt="" />`;
      lead.innerHTML = `<img src="${url}" alt="" />`;
      lead.classList.add("on");
      return;
    }
  } catch {
    /* fall through to the placeholder */
  }
  art.innerHTML = SVG.note;
}

export function paint(s: IslandStatus): void {
  if (!enabled) return;

  const m = s.media;
  $("np-title").textContent = m.active && m.title ? m.title : "Nothing playing";
  $("np-artist").textContent = m.active ? [m.artist, m.source].filter(Boolean).join(" · ") : "";
  $("np-play").innerHTML = m.playing ? SVG.pause : SVG.play;
  void paintArt(m.active ? m.title : "");

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
      showActivity(SVG.note, now.title.slice(0, 22), null);
    }
  }
  prev = now;

  trail.classList.toggle("on", m.playing);
  trail.innerHTML = m.playing ? SVG.note : "";
}

/* --------------------------------------------------------------- startup */

export function initIsland(on: boolean): void {
  enabled = on;
  island.hidden = false;

  $("np-prev").innerHTML = SVG.prev;
  $("np-next").innerHTML = SVG.next;
  $("np-play").innerHTML = SVG.play;

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

  setState();
}

export function setIslandEnabled(on: boolean): void {
  enabled = on;
  setState();
}
