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
 */

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import "../shared/theme.css";
import "./switcher.css";

interface Entry {
  hwnd: number;
  title: string;
  icon: string | null;
}

const cardsEl = document.getElementById("cards") as HTMLDivElement;
const captionEl = document.getElementById("caption") as HTMLDivElement;

let entries: Entry[] = [];
let selected = 0;

function paintSelection(): void {
  const cards = [...cardsEl.children] as HTMLElement[];
  cards.forEach((c, i) => c.classList.toggle("on", i === selected));
  captionEl.textContent = entries[selected]?.title ?? "";
  // Keep the selection in view when there are more windows than fit.
  cards[selected]?.scrollIntoView({ block: "nearest", inline: "center" });
}

async function render(list: Entry[], sel: number): Promise<void> {
  entries = list;
  selected = sel;
  cardsEl.textContent = "";

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
      row.appendChild(img);
    }
    const label = document.createElement("span");
    label.textContent = e.title;
    row.appendChild(label);

    card.append(shot, row);
    card.addEventListener("click", () => void invoke("cmd_switcher_pick", { hwnd: e.hwnd }));
    cardsEl.appendChild(card);
  }

  paintSelection();

  // Measure after layout, then hand the holes to Rust to register previews in.
  await new Promise(requestAnimationFrame);
  const slots = [...cardsEl.querySelectorAll(".shot")].map((el, i) => {
    const r = el.getBoundingClientRect();
    return {
      hwnd: entries[i].hwnd,
      x: r.left,
      y: r.top,
      w: r.width,
      h: r.height,
    };
  });
  await invoke("cmd_switcher_slots", { slots });
}

async function main(): Promise<void> {
  await listen<[Entry[], number]>("switcher:show", (e) => {
    void render(e.payload[0], e.payload[1]);
  });
  await listen<number>("switcher:select", (e) => {
    selected = e.payload;
    paintSelection();
  });

  // Created hidden and shown by Rust when Alt+Tab fires; showing it here would put
  // an empty bar on screen at startup.
  await getCurrentWindow().hide();
}

void main();
