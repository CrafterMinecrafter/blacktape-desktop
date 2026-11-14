import { describe, expect, it } from "vitest";
import { maximizeButtonLabel } from "@/layouts/maximizeButton.ts";

describe("maximizeButtonLabel", () => {
  it("offers to maximize a normal window", () => {
    expect(maximizeButtonLabel(false)).toBe("Maximize");
  });

  it("offers to restore a maximized window", () => {
    expect(maximizeButtonLabel(true)).toBe("Restore");
  });
});
