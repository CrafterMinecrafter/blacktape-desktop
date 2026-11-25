import { describe, expect, it } from "vitest";
import fs from "fs";
import path from "path";

describe("Tauri default capabilities", () => {
  it("includes window permissions for focus, unminimize, and show", () => {
    const raw = fs.readFileSync(path.resolve(__dirname, "./default.json"), "utf8");
    const json = JSON.parse(raw);
    expect(json.permissions).toContain("core:window:allow-set-focus");
    expect(json.permissions).toContain("core:window:allow-unminimize");
    expect(json.permissions).toContain("core:window:allow-show");
  });
});

describe("torrent capability", () => {
  it("grants the main window only the metadata commands used by the frontend bridge", () => {
    const capability = JSON.parse(fs.readFileSync(path.resolve(__dirname, "torrent.json"), "utf8"));
    const bridge = fs.readFileSync(path.resolve(__dirname, "../../src/shared/lib/audio.ts"), "utf8");
    const invokes = [...bridge.matchAll(/invoke\("plugin:torrent\|([a-z_]+)"/g)]
      .map(([, command]) => `torrent:allow-${command.replaceAll("_", "-")}`);
    expect(capability.windows).toEqual(["main"]);
    expect(invokes).toHaveLength(4);
    expect(capability.permissions).toEqual(invokes);
  });
});
