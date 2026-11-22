import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { captureFrontendError } from "./telemetry.ts";

describe("captureFrontendError", () => {
  beforeEach(() => {
    vi.spyOn(console, "error").mockImplementation(() => {});
  });

  afterEach(() => {
    delete (globalThis as unknown as { window?: unknown }).window;
    vi.restoreAllMocks();
  });

  it("logs error to console without rethrowing if window is undefined", () => {
    captureFrontendError(new Error("test error"), "testContext");

    expect(console.error).toHaveBeenCalledWith(
      "[testContext]",
      expect.any(Error),
    );
  });

  it("logs error to console without rethrowing if Sentry bridge is not present", () => {
    (globalThis as unknown as { window: unknown }).window = {};

    captureFrontendError(new Error("test error"), "testContext");

    expect(console.error).toHaveBeenCalledWith(
      "[testContext]",
      expect.any(Error),
    );
  });

  it("rethrows error in microtask when Sentry bridge is active", async () => {
    (globalThis as unknown as { window: unknown }).window = {
      __SENTRY__: {},
    };

    let caughtError: unknown = null;
    const originalQueueMicrotask = globalThis.queueMicrotask;
    globalThis.queueMicrotask = (fn: () => void) => {
      try {
        fn();
      } catch (err) {
        caughtError = err;
      }
    };

    try {
      captureFrontendError("network timeout", "api");
      expect(caughtError).toBeInstanceOf(Error);
      expect((caughtError as Error).message).toBe("[api] network timeout");
    } finally {
      globalThis.queueMicrotask = originalQueueMicrotask;
    }
  });
});
