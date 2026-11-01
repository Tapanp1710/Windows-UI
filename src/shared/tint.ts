/**
 * The glass tint, applied to whichever window imports this.
 *
 * theme.css builds every colour from `--hue` and `--tint`; this sets them from the
 * `glassHue` setting and keeps them in step. Rust emits `dock:prefs` to every
 * window whenever settings change, so picking a hue in the dock's tray recolours
 * the dock, the launcher, the panels and the switcher at once.
 */

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

export function applyTint(hue: number | null | undefined): void {
  const root = document.documentElement;
  const tinted = typeof hue === "number";
  root.style.setProperty("--hue", String(tinted ? hue : 220));
  root.style.setProperty("--tint", tinted ? "1" : "0");
  root.dataset.tint = tinted ? "hue" : "clear";
}

/**
 * Apply the current tint now and on every change. `onChange` is for windows whose
 * acrylic backdrop is tinted in Rust and has to be asked for again.
 */
export async function followTint(onChange?: () => void): Promise<void> {
  const read = async () => {
    try {
      const p = await invoke<{ glassHue: number | null }>("cmd_system_prefs");
      applyTint(p.glassHue);
    } catch {
      /* clear glass is the CSS default */
    }
  };
  await read();
  await listen("dock:prefs", async () => {
    await read();
    onChange?.();
  });
}
