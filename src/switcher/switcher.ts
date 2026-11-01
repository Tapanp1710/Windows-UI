/**
 * Alt+Tab switcher.
 *
 * Rust owns the keyboard, the window list and the selection; this draws it. The
 * split matters: the keyboard hook has to stay fast enough that Windows does not
 * drop it, so nothing about a keystroke waits on a webview.
 *
 * Each card leaves a hole for its DWM preview. The compositor draws the real window
 * into that rectangle **over** this page, so `.shot` must stay empty, and its
 * position has to be reported back after layout.
 *
 * That is also why nothing here moves with a transform: a preview stays where it
 * was placed, so a card that slid or scaled would leave its preview behind. The
 * bar only fades (Rust fades the previews with it), and the selection is a
 * separate highlight that slides between cards that stay still.
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

const barEl = document.getElementById("bar") as HTMLDivElement;
const cardsEl = document.getElementById("cards") as HTMLDivElement;
const captionEl = document.getElementById("caption") as HTMLDivElement;

let entries: Entry[] = [];
let selected = 0;
let gen = 0;

function cards(): HTMLElement[] {
  return [...cardsEl.querySelectorAll<HTMLElement>(".card")];
}

/** Hand the preview holes to Rust. Layout is forced here, so no frame wait. */
function reportSlots(): void {
  const slots = cards().map((card, i) => {
    const r = (card.querySelector(".shot") as HTMLElement).getBoundingClientRect();
    return { hwnd: entries[i].hwnd, x: r.left, y: r.top, w: r.width, h: r.height };
  });
  void invoke("cmd_switcher_slots", { slots, gen });
}

function paintSelection(animate: boolean): void {
  const all = cards();
  all.forEach((c, i) => c.classList.toggle("on", i === selected));
  captionEl.textContent = entries[selected]?.title ?? "";

  const card = all[selected];
  if (!card) return;

  // Keep the selection in view when there are more windows than fit. Scrolling
  // moves the holes, so the previews have to be placed again.
  const before = cardsEl.scrollLeft;
  card.scrollIntoView({ block: "nearest", inline: "nearest" });
  const scrolled = cardsEl.scrollLeft !== before;

  const hl = document.getElementById("hl") as HTMLDivElement;
  hl.classList.toggle("instant", !animate || scrolled);
  hl.style.width = `${card.offsetWidth}px`;
  hl.style.height = `${card.offsetHeight}px`;
  // The highlight lives inside the scrolling row, so it scrolls with the cards and
  // their offsets are already in its coordinates.
  hl.style.transform = `translate(${card.offsetLeft}px, ${card.offsetTop}px)`;

  if (scrolled) reportSlots();
}

function render(list: Entry[], sel: number): void {
  entries = list;
  selected = sel;
  barEl.classList.remove("in");
  cardsEl.textContent = "";

  const hl = document.createElement("div");
  hl.id = "hl";
  hl.className = "hl instant";
  cardsEl.appendChild(hl);

  for (const e of entries) {
    const card = document.createElement("div");
    card.className = "card";

    const shot = document.createElement("div");
    shot.className = "shot";

    const row = document.createElement("div");
    row.className = "row";
    if (e.icon) {
      const img = document.createElement("img");
      img.src = e.icon;
      img.alt = "";
      img.decoding = "sync";
      row.appendChild(img);
    }
    const label = document.createElement("span");
    label.textContent = e.title;
    row.appendChild(label);

    card.append(shot, row);
    card.addEventListener("click", () => void invoke("cmd_switcher_pick", { hwnd: e.hwnd }));
    cardsEl.appendChild(card);
  }

  paintSelection(false);
  reportSlots();
}

async function main(): Promise<void> {
  // Glass tint first, so nothing is drawn in the wrong colour.
  await followTint();
  await listen<[Entry[], number, number]>("switcher:show", (e) => {
    gen = e.payload[2];
    render(e.payload[0], e.payload[1]);
  });
  // Sent once the window is actually on screen, so the fade is seen rather than
  // spent while hidden.
  await listen<number>("switcher:visible", (e) => {
    if (e.payload === gen) barEl.classList.add("in");
  });
  await listen<number>("switcher:select", (e) => {
    selected = e.payload;
    paintSelection(true);
  });
  // Blank while hidden, so the next opening never flashes this one's cards.
  await listen("switcher:hide", () => {
    barEl.classList.remove("in");
  });

  // Created hidden and shown by Rust when Alt+Tab fires; showing it here would put
  // an empty bar on screen at startup.
  await getCurrentWindow().hide();
}

void main();
