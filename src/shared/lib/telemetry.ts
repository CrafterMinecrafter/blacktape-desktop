export function captureFrontendError(error: unknown, context?: string): void {
  if (context) {
    console.error(`[${context}]`, error);
  } else {
    console.error(error);
  }

  // Only forward when tauri-plugin-sentry injected the bridge (i.e. SENTRY_DSN was set at build time)
  const hasSentryBridge =
    typeof window !== "undefined" &&
    Boolean((window as unknown as { __SENTRY__?: unknown }).__SENTRY__);
  if (!hasSentryBridge) return;

  // Rethrow asynchronously so tauri-plugin-sentry's global error listener captures it
  // and passes it to Rust `sanitize_event` without interrupting the caller's control flow.
  const normalized = error instanceof Error ? error : new Error(String(error));
  if (context) {
    normalized.message = `[${context}] ${normalized.message}`;
  }

  queueMicrotask(() => {
    throw normalized;
  });
}
