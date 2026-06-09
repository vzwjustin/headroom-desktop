import { describe, expect, it } from "vitest";

import {
  buildInitialProxyVerificationRows,
  getClaudeConnector,
  getCodexConnector,
  getConnectorsNeedingSetup,
  getContactRequestValidationError,
  getDisabledManagedConnectors,
  getInitialLauncherStage,
  getLauncherAutoConfigureDecision,
  isAnyManagedConnectorEnabled,
  isManagedConnectorId,
  isValidEmailAddress,
  nextAutoConfigureStep,
  nextAutoConfigureStepAfterApply,
  proxyVerificationWaitingCopy
} from "./launcherHelpers";
import type { ClientConnectorStatus } from "./types";

describe("launcher helpers", () => {
  it("validates trimmed email addresses for auth and contact flows", () => {
    expect(isValidEmailAddress("  user@example.com  ")).toBe(true);
    expect(isValidEmailAddress("missing-at-symbol")).toBe(false);
    expect(isValidEmailAddress("user@example")).toBe(false);
  });

  it("returns contact validation errors before submit work begins", () => {
    expect(getContactRequestValidationError(undefined, "user@example.com")).toBe(
      "Set VITE_HEADROOM_CONTACT_FORM_URL to enable contact requests."
    );
    expect(getContactRequestValidationError("https://example.com/form", "invalid")).toBe(
      "Enter a valid email address."
    );
    expect(
      getContactRequestValidationError("https://example.com/form", "user@example.com")
    ).toBeNull();
  });

  it("finds the managed Claude connector from mixed connector lists", () => {
    const connectors: ClientConnectorStatus[] = [
      { clientId: "cursor", name: "Cursor", installed: true, enabled: false, verified: false },
      {
        clientId: "claude_code",
        name: "Claude Code",
        installed: true,
        enabled: true,
        verified: true
      }
    ];

    expect(getClaudeConnector(connectors)).toEqual(connectors[1]);
  });

  it("finds the managed Codex connector from mixed connector lists", () => {
    const connectors: ClientConnectorStatus[] = [
      { clientId: "cursor", name: "Cursor", installed: true, enabled: false, verified: false },
      {
        clientId: "codex_cli",
        name: "Codex",
        installed: true,
        enabled: true,
        verified: true
      }
    ];

    expect(getCodexConnector(connectors)).toEqual(connectors[1]);
  });

  it("decides whether launcher auto-setup should wait, apply setup, or continue", () => {
    expect(getLauncherAutoConfigureDecision([])).toBe("show_client_setup");
    expect(
      getLauncherAutoConfigureDecision([
        {
          clientId: "claude_code",
          name: "Claude Code",
          installed: false,
          enabled: false,
          verified: false
        },
        {
          clientId: "codex_cli",
          name: "Codex",
          installed: true,
          enabled: false,
          verified: false
        }
      ])
    ).toBe("apply_client_setup");
    expect(
      getLauncherAutoConfigureDecision([
        {
          clientId: "claude_code",
          name: "Claude Code",
          installed: true,
          enabled: false,
          verified: false
        }
      ])
    ).toBe("apply_client_setup");
    expect(
      getLauncherAutoConfigureDecision([
        {
          clientId: "claude_code",
          name: "Claude Code",
          installed: true,
          enabled: true,
          verified: false
        }
      ])
    ).toBe("begin_proxy_verification");
    expect(
      getLauncherAutoConfigureDecision([
        {
          clientId: "claude_code",
          name: "Claude Code",
          installed: true,
          enabled: true,
          verified: false
        },
        {
          clientId: "codex_cli",
          name: "Codex",
          installed: true,
          enabled: false,
          verified: false
        }
      ])
    ).toBe("apply_client_setup");
  });

  it("lists disabled managed connectors for re-enable flows", () => {
    const connectors: ClientConnectorStatus[] = [
      {
        clientId: "claude_code",
        name: "Claude Code",
        installed: true,
        enabled: false,
        verified: false
      },
      {
        clientId: "codex_cli",
        name: "Codex",
        installed: true,
        enabled: false,
        verified: false
      }
    ];

    expect(getDisabledManagedConnectors(connectors)).toEqual(connectors);
    expect(isManagedConnectorId("codex_cli")).toBe(true);
    expect(isManagedConnectorId("cursor")).toBe(false);
  });

  it("lists installed managed connectors that still need setup", () => {
    const connectors: ClientConnectorStatus[] = [
      {
        clientId: "claude_code",
        name: "Claude Code",
        installed: true,
        enabled: true,
        verified: false
      },
      {
        clientId: "codex_cli",
        name: "Codex",
        installed: true,
        enabled: false,
        verified: false
      }
    ];

    expect(getConnectorsNeedingSetup(connectors)).toEqual([connectors[1]]);
    expect(isAnyManagedConnectorEnabled(connectors)).toBe(true);
  });

  it("builds initial proxy verification rows from enabled installed managed connectors", () => {
    const rows = buildInitialProxyVerificationRows([
      { clientId: "cursor", name: "Cursor", installed: true, enabled: true, verified: false },
      {
        clientId: "claude_code",
        name: "Claude Code",
        installed: true,
        enabled: true,
        verified: false
      },
      {
        clientId: "codex_cli",
        name: "Codex",
        installed: true,
        enabled: true,
        verified: false
      },
      {
        clientId: "codex_cli",
        name: "Codex Beta",
        installed: true,
        enabled: false,
        verified: false
      }
    ]);

    expect(rows).toEqual([
      {
        clientId: "claude_code",
        name: "Claude Code",
        state: "processing",
        message: proxyVerificationWaitingCopy("claude_code")
      },
      {
        clientId: "codex_cli",
        name: "Codex",
        state: "processing",
        message: proxyVerificationWaitingCopy("codex_cli")
      }
    ]);
  });

  describe("getInitialLauncherStage", () => {
    it("returns null in non-launcher windows regardless of bootstrap state", () => {
      expect(getInitialLauncherStage("dashboard", true, true, "first_run")).toBeNull();
      expect(getInitialLauncherStage("tray", true, true, "resume")).toBeNull();
    });

    it("returns null in the launcher window until bootstrap is complete", () => {
      expect(getInitialLauncherStage("launcher", false, false, "first_run")).toBeNull();
    });

    it("lands first-run users on install when bootstrap completed during startup", () => {
      expect(getInitialLauncherStage("launcher", true, false, "first_run")).toBe("install");
    });

    it("lands first-run users on install when bootstrap was already complete", () => {
      expect(getInitialLauncherStage("launcher", false, true, "first_run")).toBe("install");
    });

    it("lands returning users on post_install once bootstrap is complete", () => {
      expect(getInitialLauncherStage("launcher", true, true, "resume")).toBe("post_install");
      expect(getInitialLauncherStage("launcher", false, true, "dashboard")).toBe(
        "post_install"
      );
    });
  });

  describe("nextAutoConfigureStep", () => {
    const claude: ClientConnectorStatus = {
      clientId: "claude_code",
      name: "Claude Code",
      installed: true,
      enabled: false,
      verified: false
    };
    const codex: ClientConnectorStatus = {
      clientId: "codex_cli",
      name: "Codex",
      installed: true,
      enabled: false,
      verified: false
    };

    it("routes show_client_setup decisions to manual setup", () => {
      expect(nextAutoConfigureStep("show_client_setup", [claude])).toEqual({
        kind: "show_client_setup"
      });
    });

    it("routes apply_client_setup to an apply step using the first connector needing setup", () => {
      expect(nextAutoConfigureStep("apply_client_setup", [claude, codex])).toEqual({
        kind: "apply",
        clientId: "claude_code"
      });
    });

    it("falls back to manual setup when apply_client_setup has no connectors needing setup", () => {
      expect(nextAutoConfigureStep("apply_client_setup", [])).toEqual({
        kind: "show_client_setup"
      });
    });

    it("routes begin_proxy_verification straight to proxy verification", () => {
      expect(nextAutoConfigureStep("begin_proxy_verification", [])).toEqual({
        kind: "begin_proxy_verification"
      });
    });
  });

  describe("nextAutoConfigureStepAfterApply", () => {
    it("advances to proxy verification when apply produced a verified setup", () => {
      expect(nextAutoConfigureStepAfterApply("begin_proxy_verification")).toEqual({
        kind: "begin_proxy_verification"
      });
    });

    it("falls back to manual setup when post-apply state still needs attention", () => {
      expect(nextAutoConfigureStepAfterApply("show_client_setup")).toEqual({
        kind: "show_client_setup"
      });
      expect(nextAutoConfigureStepAfterApply("apply_client_setup")).toEqual({
        kind: "show_client_setup"
      });
    });
  });
});
