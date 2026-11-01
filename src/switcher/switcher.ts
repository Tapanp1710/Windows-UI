/**
 * Alt+Tab switcher, as a carousel.
 *
 * Rust owns the keyboard, the window list and the selection; this draws it. The
 * split matters: the keyboard hook has to stay fast enough that Windows does not
 * drop it, so nothing about a keystroke waits on a webview.
 *
 * The open windows sit on a ring around a glass disc. Tab (from Rust) turns the
 * ring so the selection comes round to the bottom. Pointing at a window selects it
 * and previews it *without* turning the ring - a ring that turned on hover would
 * move the next icon under the pointer, which would select that one, and so on.
 *
 * The preview in the middle is one DWM thumbnail drawn over the page into `#shot`.
 * The hole never moves; this reports which window should fill it, and Rust
 * cross-fades from the last one.
 */

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import "../shared/theme.css";
import { followTint } from "../shared/tint";
import "./switcher.css";

interface Entry {
  hwnd: number;
  title: string;
  icon: string | null;
}

/** The disc's diameter as a share of the screen's height. */
const DISC_SHARE = 0.5;
/** The window is this big (tauri.conf.json), so the disc can never be bigger. */
const DISC_MAX = 740;
/** How far the ring winds in from when it opens. */
const WIND_IN = 70;

const discEl = document.getElementById("disc") as HTMLDivElement;
const ringEl = document.getElementById("ring") as HTMLDivElement;
const shotEl = document.getElementById("shot") as HTMLDivElement;
const captionEl = document.getElementById("caption") as HTMLDivElement;
const countEl = document.getElementById("count") as HTMLDivElement;

let entries: Entry[] = [];
let selected = 0;
let gen = 0;
/** The ring's turn, in degrees. Never wrapped, so every turn takes the short way
 * round instead of spinning back past 360. */
let spin = 0;
/** Where the pointer last really was, to tell a moved pointer from icons moving
 * under a still one while the ring turns. */
let lastPointer = "";

const step = () => 360 / Math.max(1, entries.length);

function items(): HTMLElement[] {
  return [...ringEl.querySelectorAll<HTMLElement>(".item")];
}

/** The turn that brings `index` to the bottom of the ring, taken the short way
 * from where the ring is now. */
function spinFor(index: number): number {
  const target = 180 - index * step();
  const delta = ((((target - spin) % 360) + 540) % 360) - 180;
  return spin + delta;
}

function setSpin(deg: number): void {
  spin = deg;
  ringEl.style.setProperty("--spin", `${deg}deg`);
}

/** Ask Rust to put this window's live preview in the hole. */
function preview(index: number): void {
  const e = entries[index];
  if (!e) return;
  const r = shotEl.getBoundingClientRect();
  void invoke("cmd_switcher_slots", {
    slots: [{ hwnd: e.hwnd, x: r.left, y: r.top, w: r.width, h: r.height }],
    gen,
  });
}

function paint(): void {
  const n = entries.length;
  items().forEach((el, i) => {
    const d = Math.min((i - selected + n) % n, (selected - i + n) % n);
    el.classList.toggle("on", d === 0);
    el.classList.toggle("near", d === 1);
  });
  captionEl.textContent = entries[selected]?.title ?? "";
  countEl.textContent = n > 1 ? `${selected + 1} of ${n}` : "";
}

/**
 * Size the disc from the screen: half its height across, so it never covers more
 * than the middle of the screen. The ring, the icons and the preview hole all
 * follow from that one number.
 */
function layoutFor(n: number): void {
  const disc = Math.round(Math.min(DISC_MAX, window.screen.height * DISC_SHARE));
  // Icons as large as the disc allows, shrunk further when there are many windows
  // so neighbours never overlap.
  let item = Math.min(64, Math.max(32, disc * 0.1));
  // The selected icon grows to 1.24x; it must stay inside the rim.
  let ring = disc / 2 - (item * 1.24) / 2 - 12;
  const room = (2 * Math.PI * ring) / Math.max(n, 1) - 12;
  if (room < item) {
    item = Math.max(26, room);
    ring = disc / 2 - (item * 1.24) / 2 - 12;
  }
  // The preview and its title fit inside the ring of icons.
  const shotW = Math.round(disc * 0.48);
  const shotH = Math.round(shotW * 0.57);

  const root = document.documentElement.style;
  root.setProperty("--disc", `${disc}px`);
  root.setProperty("--ring-r", `${Math.round(ring)}px`);
  root.setProperty("--item", `${Math.round(item)}px`);
  root.setProperty("--shot-w", `${shotW}px`);
  root.setProperty("--shot-h", `${shotH}px`);
}

function render(list: Entry[], sel: number): void {
  entries = list;
  selected = sel;
  discEl.classList.remove("in");
  ringEl.textContent = "";

  layoutFor(entries.length);
  const n = entries.length;

  entries.forEach((e, i) => {
    const item = document.createElement("div");
    item.className = "item";
    item.style.setProperty("--a", `${i * step()}deg`);
    item.setAttribute("aria-label", e.title);

    const bubble = document.createElement("div");
    bubble.className = "bubble";
    if (e.icon) {
      const img = document.createElement("img");
      img.src = e.icon;
      img.alt = "";
      img.decoding = "sync";
      bubble.appendChild(img);
    } else {
      const f = document.createElement("span");
      f.className = "fallback";
      bubble.appendChild(f);
    }
    // Stagger the icons growing in, starting from the selected one.
    const order = (i - sel + n) % n;
    bubble.style.transitionDelay = `${Math.min(order, 12) * 18}ms`;
    item.appendChild(bubble);

    item.addEventListener("pointermove", (ev) => {
      const at = `${ev.screenX},${ev.screenY}`;
      if (at === lastPointer) return;
      lastPointer = at;
      if (i === selected) return;
      selected = i;
      paint();
      preview(i);
      void invoke("cmd_switcher_hover", { index: i, gen });
    });
    item.addEventListener("click", () => void invoke("cmd_switcher_pick", { hwnd: e.hwnd }));
    ringEl.appendChild(item);
  });

  // Start wound back and small; `switcher:visible` lets it turn into place.
  ringEl.classList.add("instant", "opening");
  setSpin(spinFor(sel) - WIND_IN);
  paint();
  preview(sel);
}

function settle(): void {
  // Commit the wound-back start before turning, or it never animates.
  void ringEl.offsetWidth;
  ringEl.classList.remove("instant", "opening");
  setSpin(spin + WIND_IN);
  // Delays are only for the entrance; Tab must answer at once.
  window.setTimeout(() => {
    for (const b of ringEl.querySelectorAll<HTMLElement>(".bubble")) b.style.transitionDelay = "";
  }, 500);
}

async function main(): Promise<void> {
  // Glass tint first, so nothing is drawn in the wrong colour.
  await followTint();
  await listen<[Entry[], number, number]>("switcher:show", (e) => {
    gen = e.payload[2];
    render(e.payload[0], e.payload[1]);
  });
  // Sent once the window is actually on screen, so the entrance is seen rather
  // than spent while hidden.
  await listen<number>("switcher:visible", (e) => {
    if (e.payload !== gen) return;
    discEl.classList.add("in");
    settle();
  });
  await listen<number>("switcher:select", (e) => {
    selected = e.payload;
    setSpin(spinFor(selected));
    paint();
    preview(selected);
  });
  // Blank while hidden, so the next opening never flashes this one's ring.
  await listen("switcher:hide", () => {
    discEl.classList.remove("in");
  });

  // Created hidden and shown by Rust when Alt+Tab fires; showing it here would put
  // an empty disc on screen at startup.
  await getCurrentWindow().hide();
}

void main();
