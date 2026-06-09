import { describe, expect, it } from "vitest";
import { describeInvokeError } from "./appHelpers";

describe("describeInvokeError", () => {
  it("prefers Error.message", () => {
    expect(describeInvokeError(new Error("network down"), "fallback")).toBe("network down");
  });

  it("accepts string errors", () => {
    expect(describeInvokeError("permission denied", "fallback")).toBe("permission denied");
  });

  it("reads message and error fields from objects", () => {
    expect(describeInvokeError({ message: "typed message" }, "fallback")).toBe("typed message");
    expect(describeInvokeError({ error: "nested error" }, "fallback")).toBe("nested error");
  });

  it("falls back when message is blank", () => {
    expect(describeInvokeError({ message: "   " }, "fallback")).toBe("fallback");
  });
});
