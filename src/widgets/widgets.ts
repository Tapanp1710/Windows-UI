/**
 * Desktop widgets.
 *
 * One transparent window covering the screen, parked just above the desktop by Rust
 * (`place_above_desktop`), holding all five widgets. The window's clickable area is
 * cut down to exactly the visible cards, so every pixel between them behaves like
 * ordinary desktop: icons, right-click, drag-select, double-click to open. That is
 * the whole reason this is one window and not five - and it is the same hit-test
 * trick the top bar uses.
 *
 * Division of labour: Rust supplies CPU, RAM, battery and the Windows media session.
 * The clock is the system clock, the calendar is arithmetic, and the weather comes
 * from Open-Meteo through the webview's own `fetch` - no API key and no HTTP client
 * in the backend.
 */

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import "../shared/theme.css";
import "./widgets.css";

type WidgetId = "clock" | "calendar" | "weather" | "media" | "stats";
const IDS: WidgetId[] = ["clock", "calendar", "weather", "media", "stats"];

interface WidgetState {
  on: boolean;
  x: number;
  y: number;
}

interface Config {
  settings: {
    widgets: Partial<Record<WidgetId, WidgetState>>;
    weatherCity: string;
    weatherLat: number;
    weatherLon: number;
    theme: string;
    animationSpeed: number;
  };
}

interface Load {
  cpuPercent: number | null;
  ramPercent: number;
  ramUsedGb: number;
  ramTotalGb: number;
}

interface Battery {
  percent: number | null;
  charging: boolean;
  present: boolean;
}

interface NowPlaying {
  active: boolean;
  title: string;
  artist: string;
  playing: boolean;
  source: string;
}

interface WidgetData {
  load: Load;
  battery: Battery;
  media: NowPlaying;
}

const $ = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;
const el = (id: WidgetId) => $<HTMLElement>(`w-${id}`);

let state: Record<WidgetId, WidgetState>;
let weather = { city: "", lat: 0, lon: 0 };

/* --------------------------------------------------------------- placement */

/**
 * Where a widget goes before the user has ever moved it.
 *
 * The brief asks for the clock top left and the calendar top right; the rest are
 * placed so nothing overlaps at any screen size. Everything is computed from the
 * real viewport rather than hard-coded, so this is still correct on another display.
 * The top inset clears the top bar, which reserves its own strip.
 */
function defaultLayout(): Record<WidgetId, WidgetState> {
  const M = 20;
  const TOP = 62;
  const right = (w: number) => Math.max(M, window.innerWidth - w - M);
  const bottom = (h: number) => Math.max(TOP, window.innerHeight - h - M);

  return {
    clock: { on: true, x: M, y: TOP },
    calendar: { on: true, x: right(244), y: TOP },
    weather: { on: true, x: M, y: TOP + 118 },
    stats: { on: true, x: right(222), y: bottom(110) },
    media: { on: true, x: M, y: bottom(108) },
  };
}

/** Keep every card on screen, e.g. after a resolution change. */
function clamp(id: WidgetId, s: WidgetState): WidgetState {
  const r = el(id).getBoundingClientRect();
  const w = r.width || 200;
  const h = r.height || 100;
  return {
    ...s,
    x: Math.min(Math.max(8, s.x), Math.max(8, window.innerWidth - w - 8)),
    y: Math.min(Math.max(8, s.y), Math.max(8, window.innerHeight - h - 8)),
  };
}

function place(id: WidgetId): void {
  const node = el(id);
  const s = state[id];
  node.hidden = !s.on;
  node.style.setProperty("--x", `${Math.round(s.x)}px`);
  node.style.setProperty("--y", `${Math.round(s.y)}px`);
}

/* ------------------------------------------------------------ hit regions */

let lastRegions = "";

/** Must match --shadow-reach in theme.css. */
const PAD = 18;
/** Must match --radius-card in theme.css. */
const CARD_RADIUS = 12;

/**
 * Hand Rust the rectangles the window is allowed to receive clicks in.
 *
 * `force` is used while dragging: the region is temporarily the whole window,
 * because a region that excludes the pixel under the pointer makes the hit test
 * fail and the drag die halfway across the screen.
 */
async function syncRegions(force?: "all"): Promise<void> {
  await new Promise(requestAnimationFrame);

  const rects =
    force === "all"
      ? [{ x: 0, y: 0, w: window.innerWidth, h: window.innerHeight }]
      : IDS.filter((id) => state[id].on)
          .map((id) => el(id).querySelector(".card") as HTMLElement)
          .filter((card) => card && card.offsetWidth > 0)
          .map((card) => {
            const r = card.getBoundingClientRect();
            // Inflated by the shadow's reach, with a matching corner radius. A
            // region tight to the card clipped the shadow into four hard corners
            // and squared off the rounded ones - the cards read as sharp slabs.
            return {
              x: Math.floor(r.left) - PAD,
              y: Math.floor(r.top) - PAD,
              w: Math.ceil(r.width) + PAD * 2,
              h: Math.ceil(r.height) + PAD * 2,
              r: CARD_RADIUS + PAD,
            };
          });

  const sig = JSON.stringify(rects);
  if (sig === lastRegions) return;
  lastRegions = sig;
  await invoke("cmd_set_topbar_regions", { rects });
}

/* ------------------------------------------------------------------ drag */

/**
 * `moved` guards against a plain click nudging a widget: nothing moves, and nothing
 * is saved, until the pointer has travelled far enough to be a deliberate drag.
 */
let drag: { id: WidgetId; dx: number; dy: number; x0: number; y0: number; moved: boolean } | null =
  null;

/** How far the pointer must travel before a press counts as a drag, in CSS px. */
const DRAG_THRESHOLD = 4;

function startDrag(e: PointerEvent): void {
  const section = (e.target as HTMLElement).closest(".w") as HTMLElement | null;
  if (!section) return;
  // Buttons inside a widget are controls, not drag handles.
  if ((e.target as HTMLElement).closest("button")) return;

  const id = section.dataset.widget as WidgetId;
  drag = {
    id,
    dx: e.clientX - state[id].x,
    dy: e.clientY - state[id].y,
    x0: e.clientX,
    y0: e.clientY,
    moved: false,
  };
  section.setPointerCapture(e.pointerId);
}

function moveDrag(e: PointerEvent): void {
  if (!drag) return;

  if (!drag.moved) {
    if (Math.abs(e.clientX - drag.x0) + Math.abs(e.clientY - drag.y0) < DRAG_THRESHOLD) return;
    drag.moved = true;
    el(drag.id).classList.add("dragging");
    // Full-window region for the duration of the drag, or the pointer leaves the
    // clickable area the moment the card moves out from under it and the drag dies
    // halfway across the screen.
    void syncRegions("all");
  }

  state[drag.id] = clamp(drag.id, {
    ...state[drag.id],
    x: e.clientX - drag.dx,
    y: e.clientY - drag.dy,
  });
  place(drag.id);
}

function endDrag(): void {
  if (!drag) return;
  const { id, moved } = drag;
  drag = null;
  if (!moved) {
    // A click, not a drag. Hand focus back and leave the layout alone.
    void invoke("cmd_widget_restore_focus");
    return;
  }
  el(id).classList.remove("dragging");
  void invoke("cmd_set_widget_positions", { positions: state });
  void syncRegions();
  void invoke("cmd_widget_restore_focus");
}

/* ----------------------------------------------------------------- clock */

function renderClock(): void {
  const now = new Date();
  // The user's own Windows formats, the same rule the top bar follows.
  $("clock-time").textContent = now.toLocaleTimeString([], {
    hour: "numeric",
    minute: "2-digit",
  });
  $("clock-date").textContent = now.toLocaleDateString([], {
    weekday: "long",
    day: "numeric",
    month: "long",
  });
}

/* -------------------------------------------------------------- calendar */

let calendarKey = "";

function renderCalendar(): void {
  const now = new Date();
  const key = `${now.getFullYear()}-${now.getMonth()}-${now.getDate()}`;
  if (key === calendarKey) return;
  calendarKey = key;

  $("cal-month").textContent = now.toLocaleDateString([], {
    month: "long",
    year: "numeric",
  });

  const grid = $("cal-grid");
  grid.textContent = "";

  // Week starts on the day the user's locale says it does, worked out from a known
  // Sunday rather than assumed.
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

/* --------------------------------------------------------------- weather */

/** Open-Meteo WMO weather codes, collapsed to a glyph and a plain description. */
function describe(code: number): [string, string] {
  if (code === 0) return ["☀️", "Clear"];
  if (code <= 2) return ["⛅", "Partly cloudy"];
  if (code === 3) return ["☁️", "Overcast"];
  if (code <= 48) return ["\u{1F32B}️", "Fog"];
  if (code <= 57) return ["\u{1F327}️", "Drizzle"];
  if (code <= 67) return ["\u{1F327}️", "Rain"];
  if (code <= 77) return ["\u{1F328}️", "Snow"];
  if (code <= 82) return ["\u{1F327}️", "Showers"];
  if (code <= 86) return ["\u{1F328}️", "Snow showers"];
  return ["⛈️", "Thunderstorm"];
}

async function refreshWeather(): Promise<void> {
  if (!state.weather.on) return;
  $("wx-city").textContent = weather.city;
  try {
    const url =
      `https://api.open-meteo.com/v1/forecast?latitude=${weather.lat}` +
      `&longitude=${weather.lon}&current=temperature_2m,weather_code&timezone=auto`;
    const r = await fetch(url);
    if (!r.ok) throw new Error(`HTTP ${r.status}`);
    const j = await r.json();
    const [icon, desc] = describe(Number(j.current?.weather_code ?? -1));
    $("wx-icon").textContent = icon;
    $("wx-temp").textContent = `${Math.round(Number(j.current?.temperature_2m))}°`;
    $("wx-desc").textContent = desc;
  } catch (e) {
    // Offline is normal, not an error worth shouting about. Say so in the card and
    // leave the last good reading's shape intact.
    $("wx-desc").textContent = "Weather unavailable";
    await invoke("cmd_log", { msg: `weather fetch failed: ${e}` });
  }
  await syncRegions();
}

/* ----------------------------------------------------------------- media */

const ICON = {
  prev: `<svg viewBox="0 0 24 24" fill="currentColor"><path d="M7 6h2v12H7zM20 6v12l-9-6z"/></svg>`,
  next: `<svg viewBox="0 0 24 24" fill="currentColor"><path d="M15 6h2v12h-2zM4 6l9 6-9 6z"/></svg>`,
  play: `<svg viewBox="0 0 24 24" fill="currentColor"><path d="M8 5v14l11-7z"/></svg>`,
  pause: `<svg viewBox="0 0 24 24" fill="currentColor"><path d="M7 5h4v14H7zM13 5h4v14h-4z"/></svg>`,
};

$("mz-prev").innerHTML = ICON.prev;
$("mz-next").innerHTML = ICON.next;
$("mz-play").innerHTML = ICON.play;

/**
 * Transport buttons.
 *
 * Each one hands focus straight back: WebView2 focuses its own child on a click,
 * which activates this window even though it carries WS_EX_NOACTIVATE, and pressing
 * "pause" must not take the caret out of whatever the user is typing in.
 */
function transport(id: string, action: string): void {
  $(id).addEventListener("click", async () => {
    await invoke("cmd_media_control", { action });
    await invoke("cmd_widget_restore_focus");
  });
}

transport("mz-prev", "previous");
transport("mz-next", "next");
transport("mz-play", "toggle");

function renderMedia(m: NowPlaying): void {
  $("mz-title").textContent = m.active && m.title ? m.title : "Nothing playing";
  $("mz-artist").textContent = m.active ? [m.artist, m.source].filter(Boolean).join(" · ") : "";
  $("mz-play").innerHTML = m.playing ? ICON.pause : ICON.play;
}

/* ----------------------------------------------------------------- stats */

function meter(barId: string, valueId: string, percent: number | null, text: string): void {
  const bar = $<HTMLElement>(barId);
  bar.style.width = `${percent ?? 0}%`;
  bar.classList.toggle("low", percent !== null && percent >= 85);
  $(valueId).textContent = text;
}

function renderStats(d: WidgetData): void {
  meter("st-cpu-bar", "st-cpu", d.load.cpuPercent, d.load.cpuPercent === null ? "–" : `${d.load.cpuPercent}%`);
  meter(
    "st-ram-bar",
    "st-ram",
    d.load.ramPercent,
    `${d.load.ramUsedGb.toFixed(1)}/${d.load.ramTotalGb.toFixed(1)}`,
  );

  const b = d.battery;
  // A "low" battery meter should be the *empty* end, the opposite of CPU and RAM,
  // so it is flagged here rather than by the shared threshold in meter().
  meter("st-bat-bar", "st-bat", b.percent, b.present && b.percent !== null ? `${b.percent}%` : "AC");
  $<HTMLElement>("st-bat-bar").classList.toggle("low", (b.percent ?? 100) <= 20 && !b.charging);
}

/* --------------------------------------------------------------- startup */

function applyPrefs(theme: string, speed: number): void {
  const root = document.documentElement;
  if (theme === "light" || theme === "dark") root.dataset.theme = theme;
  else delete root.dataset.theme;
  root.style.setProperty("--anim-scale", String(speed));
}

async function loadConfig(): Promise<void> {
  const cfg = await invoke<Config>("cmd_get_config");
  const saved = cfg.settings.widgets || {};
  const defaults = defaultLayout();
  state = Object.fromEntries(
    IDS.map((id) => [id, saved[id] ? { ...defaults[id], ...saved[id] } : defaults[id]]),
  ) as Record<WidgetId, WidgetState>;

  weather = {
    city: cfg.settings.weatherCity,
    lat: cfg.settings.weatherLat,
    lon: cfg.settings.weatherLon,
  };
  applyPrefs(cfg.settings.theme, cfg.settings.animationSpeed);

  for (const id of IDS) place(id);
  // Clamp only once the cards have a measured size.
  await new Promise(requestAnimationFrame);
  for (const id of IDS) {
    state[id] = clamp(id, state[id]);
    place(id);
  }

  // Write the layout back even when nothing was dragged. The settings window
  // toggles widgets by patching this same map, and it must not have to invent
  // coordinates for a widget that has never been placed.
  await invoke("cmd_set_widget_positions", { positions: state });
}

async function refresh(): Promise<void> {
  try {
    const d = await invoke<WidgetData>("cmd_widget_data");
    renderMedia(d.media);
    renderStats(d);
  } catch (err) {
    await invoke("cmd_log", { msg: `widget refresh failed: ${err}` });
  }
}

async function main(): Promise<void> {
  await loadConfig();

  renderClock();
  renderCalendar();
  await refresh();
  await syncRegions();
  void refreshWeather();

  document.addEventListener("pointerdown", startDrag);
  document.addEventListener("pointermove", moveDrag);
  document.addEventListener("pointerup", endDrag);
  document.addEventListener("pointercancel", endDrag);

  // Tick the clock on the minute boundary, so the displayed time is never stale.
  const scheduleClock = () => {
    const now = new Date();
    const ms = (60 - now.getSeconds()) * 1000 - now.getMilliseconds();
    window.setTimeout(() => {
      renderClock();
      renderCalendar();
      scheduleClock();
    }, ms + 50);
  };
  scheduleClock();

  // Stats and media are the only things that need a short interval; the weather
  // changes slowly enough that fifteen minutes is generous.
  window.setInterval(() => void refresh(), 2000);
  window.setInterval(() => void refreshWeather(), 15 * 60 * 1000);

  await listen("dock:prefs", async () => {
    const cfg = await invoke<Config>("cmd_get_config");
    applyPrefs(cfg.settings.theme, cfg.settings.animationSpeed);

    const saved = cfg.settings.widgets || {};
    let changed = false;
    for (const id of IDS) {
      const want = saved[id]?.on ?? true;
      if (state[id].on !== want) {
        state[id].on = want;
        place(id);
        changed = true;
      }
    }
    const moved =
      weather.lat !== cfg.settings.weatherLat || weather.lon !== cfg.settings.weatherLon;
    weather = {
      city: cfg.settings.weatherCity,
      lat: cfg.settings.weatherLat,
      lon: cfg.settings.weatherLon,
    };
    if (moved) void refreshWeather();
    if (changed) await syncRegions();
  });
}

void main();
