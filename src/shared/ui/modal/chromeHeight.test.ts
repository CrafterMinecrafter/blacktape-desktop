import { describe, expect, it } from "vitest";
import {
  CHROME_HEIGHT_VAR,
  applyChromeHeight,
} from "@/shared/ui/modal/chromeHeight.ts";

function fakeRoot() {
  const values = new Map<string, string>();
  return {
    values,
    style: {
      setProperty: (name: string, value: string) => void values.set(name, value),
      removeProperty: (name: string) => values.delete(name),
    },
  };
}

describe("applyChromeHeight", () => {
  it("publishes the header height in px", () => {
    const root = fakeRoot();
    applyChromeHeight(root, 48);
    expect(root.values.get(CHROME_HEIGHT_VAR)).toBe("48px");
  });

  it("removes it again when cleaned up", () => {
    const root = fakeRoot();
    const cleanup = applyChromeHeight(root, 40);
    cleanup();
    expect(root.values.has(CHROME_HEIGHT_VAR)).toBe(false);
  });
});
