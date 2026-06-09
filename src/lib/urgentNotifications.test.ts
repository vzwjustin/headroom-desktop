import { afterEach, describe, expect, it, vi } from "vitest";

import type { HeadroomPricingStatus, RuntimeStatus } from "./types";
import {
  maybeFireUrgentPricingNotifications,
  maybeFireUrgentRuntimeNotification,
} from "./urgentNotifications";

const { invokeMock, isVisibleMock } = vi.hoisted(() => ({
  invokeMock: vi.fn(),
  isVisibleMock: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: invokeMock,
}));

vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({ isVisible: isVisibleMock }),
}));

function installStorage(initial: Record<string, string> = {}) {
  const values = new Map(Object.entries(initial));
  Object.defineProperty(globalThis, "localStorage", {
    configurable: true,
    value: {
      getItem: vi.fn((key: string) => values.get(key) ?? null),
      setItem: vi.fn((key: string, value: string) => {
        values.set(key, value);
      }),
    },
  });
  return values;
}

function makePricing(
  overrides: Partial<HeadroomPricingStatus> = {}
): HeadroomPricingStatus {
  return {
    authenticated: true,
    localGraceStartedAt: new Date().toISOString(),
    localGraceEndsAt: new Date().toISOString(),
    localGraceActive: false,
    accountSyncError: null,
    needsAuthentication: false,
    optimizationAllowed: true,
    shouldNudge: false,
    nudgeLevel: 0,
    gateReason: null,
    gateMessage: "",
    nudgeThresholdPercent: null,
    effectiveNudgeThresholdsPercent: null,
    disableThresholdPercent: null,
    effectiveDisableThresholdPercent: null,
    recommendedSubscriptionTier: null,
    claude: {
      authMethod: "claude_ai_oauth",
      email: null,
      displayName: null,
      planTier: "free",
      hasExtraUsageEnabled: false,
    },
    account: null,
    launchDiscountActive: false,
    ...overrides,
  };
}

function makeRuntime(overrides: Partial<RuntimeStatus> = {}): RuntimeStatus {
  return {
    platform: "darwin",
    supportTier: "supported",
    installed: true,
    running: true,
    starting: false,
    paused: false,
    proxyReachable: true,
    headroomLearnSupported: true,
    rtk: {
      installed: true,
      pathConfigured: true,
      hookConfigured: true,
    },
    ...overrides,
  };
}

describe("maybeFireUrgentPricingNotifications", () => {
  it("is a no-op in open-source builds", async () => {
    await expect(
      maybeFireUrgentPricingNotifications({} as HeadroomPricingStatus)
    ).resolves.toBeUndefined();
    expect(invokeMock).not.toHaveBeenCalled();
  });
});

describe("maybeFireUrgentRuntimeNotification", () => {
  afterEach(() => {
    invokeMock.mockReset();
    isVisibleMock.mockReset();
  });

  it("fires when the runtime is installed but not running", async () => {
    isVisibleMock.mockResolvedValue(false);
    installStorage();

    await maybeFireUrgentRuntimeNotification(
      makeRuntime({ running: false })
    );

    expect(invokeMock).toHaveBeenCalledWith("show_notification", {
      title: "Headroom stopped running",
      body: "Headroom isn't running. Open the tray to restart it.",
      action: "runtime",
    });
  });

  it("surfaces the startup error when one is present", async () => {
    isVisibleMock.mockResolvedValue(false);
    installStorage();

    await maybeFireUrgentRuntimeNotification(
      makeRuntime({ running: false, startupError: "port 6767 busy" })
    );

    expect(invokeMock).toHaveBeenCalledWith("show_notification", {
      title: "Headroom stopped running",
      body: "Headroom isn't running: port 6767 busy",
      action: "runtime",
    });
  });

  it("prefers the resolution hint over the raw startup error", async () => {
    isVisibleMock.mockResolvedValue(false);
    installStorage();

    await maybeFireUrgentRuntimeNotification(
      makeRuntime({
        running: false,
        startupError: "never opened port 6768 within 60000ms",
        startupErrorHint: "Wait a moment and click Retry.",
      })
    );

    expect(invokeMock).toHaveBeenCalledWith("show_notification", {
      title: "Headroom stopped running",
      body: "Headroom isn't running. Wait a moment and click Retry.",
      action: "runtime",
    });
  });

  it("does not fire while the runtime is starting", async () => {
    isVisibleMock.mockResolvedValue(false);
    installStorage();

    await maybeFireUrgentRuntimeNotification(
      makeRuntime({ running: false, starting: true })
    );

    expect(invokeMock).not.toHaveBeenCalled();
  });

  it("does not fire while the runtime is paused", async () => {
    isVisibleMock.mockResolvedValue(false);
    installStorage();

    await maybeFireUrgentRuntimeNotification(
      makeRuntime({ running: false, paused: true })
    );

    expect(invokeMock).not.toHaveBeenCalled();
  });

  it("does not fire when the runtime isn't installed", async () => {
    isVisibleMock.mockResolvedValue(false);
    installStorage();

    await maybeFireUrgentRuntimeNotification(
      makeRuntime({ installed: false, running: false })
    );

    expect(invokeMock).not.toHaveBeenCalled();
  });

  it("does not fire while the window is visible", async () => {
    isVisibleMock.mockResolvedValue(true);
    installStorage();

    await maybeFireUrgentRuntimeNotification(
      makeRuntime({ running: false })
    );

    expect(invokeMock).not.toHaveBeenCalled();
  });

  it("does not repeat within the same day", async () => {
    isVisibleMock.mockResolvedValue(false);
    const today = new Date().toISOString().slice(0, 10);
    installStorage({ headroom_urgent_runtime_down_date: today });

    await maybeFireUrgentRuntimeNotification(
      makeRuntime({ running: false })
    );

    expect(invokeMock).not.toHaveBeenCalled();
  });
});
