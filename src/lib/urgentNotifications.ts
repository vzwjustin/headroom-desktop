import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";

import type { RuntimeStatus } from "./types";

const RUNTIME_DOWN_KEY = "headroom_urgent_runtime_down_date";

export async function maybeFireUrgentRuntimeNotification(
  runtime: RuntimeStatus
): Promise<void> {
  if (await isWindowVisible()) return;

  const runtimeDown =
    runtime.installed && !runtime.running && !runtime.starting && !runtime.paused;
  if (!runtimeDown) return;

  const body = runtime.startupErrorHint
    ? `Headroom isn't running. ${runtime.startupErrorHint}`
    : runtime.startupError
    ? `Headroom isn't running: ${runtime.startupError}`
    : "Headroom isn't running. Open the tray to restart it.";

  await fireOncePerDay(
    RUNTIME_DOWN_KEY,
    "Headroom stopped running",
    body,
    "runtime"
  );
}

async function fireOncePerDay(
  storageKey: string,
  title: string,
  body: string,
  action: string
): Promise<void> {
  const today = new Date().toISOString().slice(0, 10);
  if (localStorage.getItem(storageKey) === today) return;
  try {
    await invoke("show_notification", { title, body, action });
    localStorage.setItem(storageKey, today);
  } catch {
    // best-effort
  }
}

async function isWindowVisible(): Promise<boolean> {
  return getCurrentWindow()
    .isVisible()
    .catch(() => true);
}
