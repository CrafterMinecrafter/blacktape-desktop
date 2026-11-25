import { describe, it, expect, vi, beforeEach } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import {
  buildCanonicalQualityPlan,
  downloadButtonLabel,
  expectedQualityOf,
  extractResolveSongIds,
  formatDetails,
  getExpectedQuality,
  probeAudioFormats,
  type AudioFormat,
} from "./downloadQuality.ts";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

const format = (over: Partial<AudioFormat> = {}): AudioFormat => ({
  id: "251",
  codec: "Opus",
  bitrate_kbps: 133,
  sample_rate_hz: 48000,
  lossless: false,
  label: "Opus 133 kbps",
  ...over,
});

beforeEach(() => {
  vi.clearAllMocks();
});

describe("expectedQualityOf", () => {
  it("gives the expected quality of YouTube and SoundCloud without asking the backend", () => {
    expect(expectedQualityOf("youtube")).toEqual({ label: "Opus ~160 kbps", lossless: false });
    expect(expectedQualityOf("soundcloud")).toEqual({ label: "MP3 128 / AAC 160 kbps", lossless: false });
  });

  it("matches the platform ignoring case", () => {
    expect(expectedQualityOf("YouTube")?.label).toBe("Opus ~160 kbps");
    expect(expectedQualityOf("SOUNDCLOUD")?.label).toBe("MP3 128 / AAC 160 kbps");
  });

  it("gives null for Bandcamp (asked from the backend) and for unknown or local platforms", () => {
    expect(expectedQualityOf("bandcamp")).toBeNull();
    expect(expectedQualityOf("local")).toBeNull();
    expect(expectedQualityOf("myspace")).toBeNull();
  });
});

describe("getExpectedQuality", () => {
  it("answers YouTube at once, without an invoke", async () => {
    await expect(getExpectedQuality("youtube", "https://www.youtube.com/watch?v=a")).resolves.toEqual({
      label: "Opus ~160 kbps",
      lossless: false,
    });
    expect(invoke).not.toHaveBeenCalled();
  });

  it("asks the backend for Bandcamp", async () => {
    vi.mocked(invoke).mockResolvedValueOnce({ label: "FLAC (lossless)", lossless: true });
    const result = await getExpectedQuality("bandcamp", "https://a.bandcamp.com/album/x");
    expect(invoke).toHaveBeenCalledWith("get_expected_quality", {
      platform: "bandcamp",
      url: "https://a.bandcamp.com/album/x",
    });
    expect(result).toEqual({ label: "FLAC (lossless)", lossless: true });
  });
});

describe("probeAudioFormats", () => {
  it("sends the platform, url and album flag, with trackUrl null when none is given", async () => {
    vi.mocked(invoke).mockResolvedValueOnce([format()]);
    const formats = await probeAudioFormats("bandcamp", "https://a.bandcamp.com/album/x", true);
    expect(invoke).toHaveBeenCalledWith("probe_audio_formats", {
      platform: "bandcamp",
      url: "https://a.bandcamp.com/album/x",
      isAlbum: true,
      trackUrl: null,
    });
    expect(formats).toEqual([format()]);
  });

  it("passes the track url when the caller knows the track", async () => {
    vi.mocked(invoke).mockResolvedValueOnce([]);
    await probeAudioFormats("youtube", "https://www.youtube.com/watch?v=a", false, "https://www.youtube.com/watch?v=a");
    expect(invoke).toHaveBeenCalledWith("probe_audio_formats", {
      platform: "youtube",
      url: "https://www.youtube.com/watch?v=a",
      isAlbum: false,
      trackUrl: "https://www.youtube.com/watch?v=a",
    });
  });
});

describe("formatDetails", () => {
  it("gives the sample rate in kHz, without trailing zeros", () => {
    expect(formatDetails(format({ sample_rate_hz: 48000 }))).toBe("48 kHz");
    expect(formatDetails(format({ sample_rate_hz: 44100 }))).toBe("44.1 kHz");
  });

  it("adds lossless for lossless formats", () => {
    expect(formatDetails(format({ lossless: true, sample_rate_hz: null }))).toBe("lossless");
  });

  it("joins the sample rate and lossless with a middle dot", () => {
    expect(formatDetails(format({ lossless: true, sample_rate_hz: 44100 }))).toBe("44.1 kHz · lossless");
  });

  it("is empty when yt-dlp gave no sample rate and the format is lossy", () => {
    expect(formatDetails(format({ sample_rate_hz: null, lossless: false }))).toBe("");
  });
});

describe("buildCanonicalQualityPlan", () => {
  it("falls back to semantic tiers before candidates finish resolving", () => {
    const plan = buildCanonicalQualityPlan([]);
    expect(plan.hasDiscoveredCandidates).toBe(false);
    expect(plan.best.id).toBe("best");
    expect(plan.best.lossless).toBe(false);
    expect(plan.options.map((o) => o.id)).toEqual(["high", "standard"]);
  });

  it("reports honest opus summary instead of claiming lossless when only opus exists", () => {
    const plan = buildCanonicalQualityPlan([
      {
        index: 0,
        title: "Track 1",
        artists: ["Aphex Twin"],
        track_number: 1,
        url: "https://youtube.com/watch?v=1",
        available: true,
        note: null,
        selected_candidate_idx: 0,
        candidates: [
          {
            source: { kind: "http_stream", platform: "youtube", url: "https://youtube.com/watch?v=1" },
            quality_tier: "high_lossy",
            codec: "Opus",
            bitrate_kbps: 160,
            duration_ms: 200_000,
            match_confidence: 0.95,
            is_strict_match: true,
            display_label: "YouTube · Opus 160 kbps",
          },
          {
            source: { kind: "http_stream", platform: "bandcamp", url: "https://a.bandcamp.com/track/1" },
            quality_tier: "standard_lossy",
            codec: "MP3",
            bitrate_kbps: 128,
            duration_ms: 200_000,
            match_confidence: 0.95,
            is_strict_match: true,
            display_label: "Bandcamp · MP3 128 kbps",
          },
        ],
      },
    ]);

    expect(plan.hasDiscoveredCandidates).toBe(true);
    expect(plan.best.lossless).toBe(false);
    expect(plan.best.details).toContain("Opus 160 kbps (YOUTUBE)");
    expect(plan.options.map((o) => o.id)).toEqual(["source:youtube:high", "source:bandcamp:standard"]);
    expect(plan.options[0].platform).toBe("youtube");
    expect(plan.options[1].platform).toBe("bandcamp");
  });

  it("marks discovered Bandcamp free lossless FLAC as best and includes source:bandcamp:lossless", () => {
    const plan = buildCanonicalQualityPlan([
      {
        index: 0,
        title: "In Absentia",
        artists: ["Heaven Pierce Her"],
        track_number: 1,
        url: "https://heavenpierceher.bandcamp.com/track/in-absentia",
        available: true,
        note: null,
        selected_candidate_idx: 0,
        candidates: [
          {
            source: { kind: "http_stream", platform: "bandcamp", url: "https://heavenpierceher.bandcamp.com/track/in-absentia" },
            quality_tier: "lossless",
            codec: "FLAC",
            duration_ms: 273_000,
            match_confidence: 0.99,
            is_strict_match: true,
            display_label: "Bandcamp · FLAC (Lossless)",
          },
          {
            source: { kind: "http_stream", platform: "youtube", url: "https://youtube.com/watch?v=1" },
            quality_tier: "high_lossy",
            codec: "Opus",
            bitrate_kbps: 160,
            duration_ms: 273_000,
            match_confidence: 0.99,
            is_strict_match: true,
            display_label: "YouTube Music (Opus 160k)",
          },
        ],
      },
    ]);

    expect(plan.best.lossless).toBe(true);
    expect(plan.best.details).toContain("FLAC (Lossless) (BANDCAMP)");
    expect(plan.options.map((o) => o.id)).toEqual(["source:bandcamp:lossless", "source:youtube:high"]);
    expect(plan.options[0].lossless).toBe(true);
  });

  it("reports upgradableCount and skips redundant upgrades via local_tier notes", () => {
    const plan = buildCanonicalQualityPlan([
      {
        index: 0,
        title: "Already High",
        artists: ["Artist"],
        track_number: 1,
        url: "resolve:song:1",
        available: true,
        note: "local_tier:high",
        selected_candidate_idx: 0,
        candidates: [
          {
            source: { kind: "http_stream", platform: "youtube", url: "https://youtube.com/watch?v=1" },
            quality_tier: "high_lossy",
            codec: "Opus",
            bitrate_kbps: 160,
            duration_ms: 180_000,
            match_confidence: 0.99,
            is_strict_match: true,
            display_label: "YouTube · Opus 160 kbps",
          },
        ],
      },
      {
        index: 1,
        title: "Needs Upgrade",
        artists: ["Artist"],
        track_number: 2,
        url: "resolve:song:2",
        available: true,
        note: "local_tier:low",
        selected_candidate_idx: 0,
        candidates: [
          {
            source: { kind: "http_stream", platform: "youtube", url: "https://youtube.com/watch?v=2" },
            quality_tier: "high_lossy",
            codec: "Opus",
            bitrate_kbps: 160,
            duration_ms: 180_000,
            match_confidence: 0.99,
            is_strict_match: true,
            display_label: "YouTube · Opus 160 kbps",
          },
        ],
      },
    ]);

    expect(plan.best.upgradableCount).toBe(1);
    expect(plan.best.details).toContain("Upgrades 1/2:");

    const allHighPlan = buildCanonicalQualityPlan([
      {
        index: 0,
        title: "Already High",
        artists: ["Artist"],
        track_number: 1,
        url: "resolve:song:1",
        available: true,
        note: "local_tier:high",
        selected_candidate_idx: 0,
        candidates: [
          {
            source: { kind: "http_stream", platform: "youtube", url: "https://youtube.com/watch?v=1" },
            quality_tier: "high_lossy",
            codec: "Opus",
            bitrate_kbps: 160,
            duration_ms: 180_000,
            match_confidence: 0.99,
            is_strict_match: true,
            display_label: "YouTube · Opus 160 kbps",
          },
        ],
      },
    ]);

    expect(allHighPlan.best.upgradableCount).toBe(0);
    expect(allHighPlan.best.details).toBe("Already at equal or higher quality (no upgrade needed)");
    expect(allHighPlan.options[0].upgradableCount).toBe(0);
  });
});

describe("extractResolveSongIds", () => {
  it("parses single, batch, and album resolve urls into deduplicated numeric ids", () => {
    expect(extractResolveSongIds("resolve:song:42")).toEqual([42]);
    expect(extractResolveSongIds("resolve:batch:10,20,10")).toEqual([10, 20]);
    expect(
      extractResolveSongIds("resolve:album:Artist::Album", ["resolve:song:5", "resolve:song:6", "resolve:song:5"]),
    ).toEqual([5, 6]);
  });
});

describe("downloadButtonLabel", () => {
  it("labels best-quality option accurately and uses plain Download for other presets", () => {
    expect(downloadButtonLabel(true, "best", [])).toBe("Download in the best quality");
    expect(downloadButtonLabel(true, "high", [])).toBe("Download");
    expect(downloadButtonLabel(true, "standard", [])).toBe("Download");
    expect(downloadButtonLabel(true, "source:bandcamp:standard", [])).toBe("Download");
    expect(downloadButtonLabel(false, null, [])).toBe("Download in the best quality");
    expect(downloadButtonLabel(false, "251", [format()])).toBe("Download");
  });
});
