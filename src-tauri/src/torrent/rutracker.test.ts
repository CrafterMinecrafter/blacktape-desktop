import { readFileSync } from "node:fs";
import { runInNewContext } from "node:vm";
import { describe, expect, it, vi } from "vitest";

const script = readFileSync(new URL("./rutracker.js", import.meta.url), "utf8");

function searchWithResponse(response: Response) {
  const lookup = runInNewContext(`(${script})`, {
    location: new URL("https://rutracker.org/forum/index.php"),
    fetch: vi.fn().mockResolvedValue(response),
  });
  return lookup("/forum/tracker.php?nm=test", "search", new AbortController().signal);
}

describe("RuTracker access errors", () => {
  it("reports the Cloudflare challenge that blocks authentication", async () => {
    const response = new Response(null, { status: 403, headers: { "Cf-Mitigated": "challenge" } });
    await expect(searchWithResponse(response)).resolves.toEqual({ error: "cloudflare_challenge" });
  });

  it.each([
    [401, "sign_in"],
    [403, "blocked"],
    [429, "rate_limited"],
  ])("reports a plain HTTP %s as %s", async (status, error) => {
    await expect(searchWithResponse(new Response(null, { status }))).resolves.toEqual({ error });
  });
});
