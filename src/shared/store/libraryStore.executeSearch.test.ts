import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { useLibraryStore } from "@/shared/store/libraryStore.ts";
import type { SearchSuggestion } from "@/shared/store/libraryStore.ts";

type Handler = (ev: { payload: unknown }) => void;

const { searchPlatformsMock, listenMock, listeners } = vi.hoisted(() => {
  const listeners = new Map<string, Set<Handler>>();
  const listenMock = vi.fn(async (event: string, handler: Handler) => {
    const handlers = listeners.get(event) ?? new Set<Handler>();
    handlers.add(handler);
    listeners.set(event, handlers);
    return () => {
      handlers.delete(handler);
    };
  });
  return { searchPlatformsMock: vi.fn(), listenMock, listeners };
});

vi.mock("@/shared/lib/search/search.ts", () => ({
  searchPlatforms: searchPlatformsMock,
}));
vi.mock("@/shared/lib/audio.ts", () => ({
  getArtistAlbums: vi.fn(),
  getArtists: vi.fn(),
}));
vi.mock("@tauri-apps/api/event", () => ({ listen: listenMock }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(), convertFileSrc: (p: string) => p }));

const item = (name: string) => ({ name }) as unknown as SearchSuggestion;

function emit(event: string, payload: unknown) {
  for (const handler of [...(listeners.get(event) ?? [])]) handler({ payload });
}

function activeListenerCount() {
  let count = 0;
  for (const handlers of listeners.values()) count += handlers.size;
  return count;
}

/**
 * Starts executeSearch and waits until it has registered its listeners and called the platform search.
 * Returns the still-running search so the test can finish it when it chooses.
 */
async function startSearch(query: string) {
  const run = useLibraryStore.getState().executeSearch(query);
  await vi.waitFor(() => expect(searchPlatformsMock).toHaveBeenCalled());
  const requestId: number = searchPlatformsMock.mock.calls[0][2];
  // Wrapped so that awaiting startSearch does not wait for the search itself.
  return { run, requestId };
}

describe("executeSearch partial results", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    listeners.clear();
    vi.stubGlobal("window", { __TAURI_INTERNALS__: {} });
    useLibraryStore.setState({
      searchQuery: "",
      searchResults: [],
      searchCache: {},
      searchWarning: null,
      partialResultsFor: null,
    });
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("shows partial results for the running query while the search is still pending", async () => {
    let finish: (value: SearchSuggestion[]) => void = () => {};
    searchPlatformsMock.mockImplementation(
      () => new Promise<SearchSuggestion[]>((resolve) => (finish = resolve)),
    );
    const partial = [item("early hit")];

    const { run, requestId } = await startSearch("Radiohead");
    emit("search-partial", { requestId, results: partial });

    expect(useLibraryStore.getState().searchResults).toEqual(partial);
    expect(useLibraryStore.getState().partialResultsFor).toBe("radiohead");

    finish([item("final hit")]);
    await run;
  });

  it("ignores partial results that belong to another request", async () => {
    let finish: (value: SearchSuggestion[]) => void = () => {};
    searchPlatformsMock.mockImplementation(
      () => new Promise<SearchSuggestion[]>((resolve) => (finish = resolve)),
    );

    const { run, requestId } = await startSearch("Radiohead");
    emit("search-partial", { requestId: requestId - 1, results: [item("other request")] });

    expect(useLibraryStore.getState().searchResults).toEqual([]);
    expect(useLibraryStore.getState().partialResultsFor).toBeNull();

    finish([]);
    await run;
  });

  it("does not let a partial event that arrives after the final results replace them", async () => {
    searchPlatformsMock.mockResolvedValue([item("final hit")]);
    const finalResults = [item("final hit")];

    const { run, requestId } = await startSearch("Radiohead");
    await run;
    expect(useLibraryStore.getState().searchResults).toEqual(finalResults);

    // The listener was removed on completion; call the handler directly to simulate a late delivery.
    const partialHandler = listenMock.mock.calls.find(([event]) => event === "search-partial")?.[1];
    expect(partialHandler).toBeDefined();
    partialHandler?.({ payload: { requestId, results: [item("late partial")] } });

    expect(useLibraryStore.getState().searchResults).toEqual(finalResults);
  });

  it("removes all search listeners after the search completes", async () => {
    searchPlatformsMock.mockResolvedValue([]);

    const { run } = await startSearch("Radiohead");
    await run;

    expect(listenMock).toHaveBeenCalledWith("search-partial", expect.any(Function));
    expect(listenMock).toHaveBeenCalledWith("search-mb-warning", expect.any(Function));
    expect(activeListenerCount()).toBe(0);
  });

  it("ignores a MusicBrainz warning that belongs to another request", async () => {
    let finish: (value: SearchSuggestion[]) => void = () => {};
    searchPlatformsMock.mockImplementation(
      () => new Promise<SearchSuggestion[]>((resolve) => (finish = resolve)),
    );
    const results = [item("final hit")];

    const { run, requestId } = await startSearch("Radiohead");
    emit("search-mb-warning", { requestId: requestId - 1, message: "MusicBrainz is unavailable" });

    finish(results);
    await run;

    expect(useLibraryStore.getState().searchWarning).toBeNull();
    expect(useLibraryStore.getState().searchCache["radiohead"]).toEqual(results);
  });

  it("shows the MusicBrainz warning of the running request and does not cache its results", async () => {
    let finish: (value: SearchSuggestion[]) => void = () => {};
    searchPlatformsMock.mockImplementation(
      () => new Promise<SearchSuggestion[]>((resolve) => (finish = resolve)),
    );

    const { run, requestId } = await startSearch("Radiohead");
    emit("search-mb-warning", { requestId, message: "MusicBrainz is unavailable" });

    finish([item("partial hit")]);
    await run;

    expect(useLibraryStore.getState().searchWarning).toBe("MusicBrainz is unavailable");
    expect(useLibraryStore.getState().searchCache).not.toHaveProperty("radiohead");
  });
});
