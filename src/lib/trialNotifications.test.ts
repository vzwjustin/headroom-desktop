import { describe, expect, it, vi } from "vitest";

import type { HeadroomPricingStatus } from "./types";
import { maybeFireTrialNotifications } from "./trialNotifications";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({ isVisible: vi.fn().mockResolvedValue(false) }),
}));

describe("maybeFireTrialNotifications", () => {
  it("is a no-op in open-source builds", async () => {
    const status = { authenticated: false } as HeadroomPricingStatus;
    await expect(maybeFireTrialNotifications(status)).resolves.toBeUndefined();
  });
});
