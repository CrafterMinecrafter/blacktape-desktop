import { describe, expect, it } from "vitest";
import { getNextVersion } from "./bump.js";

describe("getNextVersion", () => {
  it("increments patch version", () => {
    expect(getNextVersion("0.5.2", "patch")).toBe("0.5.3");
    expect(getNextVersion("1.0.9", "patch")).toBe("1.0.10");
  });

  it("increments minor version and resets patch", () => {
    expect(getNextVersion("0.5.2", "minor")).toBe("0.6.0");
    expect(getNextVersion("1.2.3", "minor")).toBe("1.3.0");
  });

  it("increments major version and resets minor and patch", () => {
    expect(getNextVersion("0.5.2", "major")).toBe("1.0.0");
    expect(getNextVersion("1.2.3", "major")).toBe("2.0.0");
  });

  it("accepts an exact version string", () => {
    expect(getNextVersion("0.5.2", "0.5.3")).toBe("0.5.3");
    expect(getNextVersion("0.5.2", "v0.6.0")).toBe("0.6.0");
    expect(getNextVersion("0.5.2", "1.0.0-beta.1")).toBe("1.0.0-beta.1");
  });

  it("throws on invalid version or release type", () => {
    expect(() => getNextVersion("0.5.2", "invalid")).toThrow("Unknown version or release type");
  });
});
