import { invoke } from "@tauri-apps/api/core";
import type { AlbumTrackInfo } from "@/shared/lib/remote/albumPreview.ts";
import type { Platform } from "@/shared/types.ts";

/** Mirrors the Rust `quality::AudioFormat` (snake_case on the wire). */
export interface AudioFormat {
  /** yt-dlp format id, passed back as `StartDownloadParams.format`. */
  id: string;
  codec: string;
  bitrate_kbps: number | null;
  sample_rate_hz: number | null;
  lossless: boolean;
  /** "FLAC (lossless)", "MP3 320 kbps", "Opus 131 kbps". */
  label: string;
}

interface CanonicalQualityOption {
  id: string;
  label: string;
  details: string;
  platform?: string;
  isBest?: boolean;
  lossless?: boolean;
}

export interface CanonicalQualityPlan {
  best: CanonicalQualityOption;
  options: CanonicalQualityOption[];
  hasDiscoveredCandidates: boolean;
}

/** Mirrors the Rust `quality::ExpectedQuality`. */
export interface ExpectedQuality {
  label: string;
  lossless: boolean;
}

/** Label of the primary CTA button in QualityModal ("Download" when choosing a non-best preset). Pure. */
export function downloadButtonLabel(
  isMultiSource: boolean,
  picked: string | null,
  formats: AudioFormat[] | null,
): string {
  if (isMultiSource) {
    return picked === "best" ? "Download in the best quality" : "Download";
  }
  return formats !== null && formats.length === 0 ? "Download in the best quality" : "Download";
}

/** How long the format probe usually takes: shown so a waiting user does not think the app hung. */
export const PROBE_WAIT_TEXT = "This takes 5-10 seconds.";

/** Why a Bandcamp album often has only MP3 128 kbps, and what gives the better files. */
export const BANDCAMP_QUALITY_HINT =
  "Bandcamp streams every album as MP3 128 kbps. FLAC, ALAC, WAV and MP3 320 come with the download files, " +
  "which Bandcamp gives when the artist offers the album for free or for a price you name (you can enter 0), " +
  "or once you bought the album on Bandcamp.";

/** Expected quality without a probe, for the platforms that answer at once (Bandcamp asks the backend). Pure. */
export function expectedQualityOf(platform: Platform | string): ExpectedQuality | null {
  switch (platform.toLowerCase()) {
    case "youtube":
      return { label: "Opus ~160 kbps", lossless: false };
    case "soundcloud":
      return { label: "MP3 128 / AAC 160 kbps", lossless: false };
    default:
      return null;
  }
}

/** Expected quality of a platform album or track (Bandcamp reads the page, about 1 s; the rest at once). */
export async function getExpectedQuality(platform: Platform | string, url: string): Promise<ExpectedQuality> {
  return expectedQualityOf(platform) ?? invoke<ExpectedQuality>("get_expected_quality", { platform, url });
}

/** The formats yt-dlp finds for the track, or the first track of the album (about 3-6 s). */
export async function probeAudioFormats(
  platform: Platform | string,
  url: string,
  isAlbum: boolean,
  trackUrl?: string,
): Promise<AudioFormat[]> {
  return invoke<AudioFormat[]>("probe_audio_formats", {
    platform,
    url,
    isAlbum,
    trackUrl: trackUrl ?? null,
  });
}

/** Second line of a format row: "48 kHz" etc.; empty when yt-dlp gave nothing more than the label. Pure. */
export function formatDetails(format: AudioFormat): string {
  const parts: string[] = [];
  if (format.sample_rate_hz) parts.push(`${+(format.sample_rate_hz / 1000).toFixed(1)} kHz`);
  if (format.lossless) parts.push("lossless");
  return parts.join(" · ");
}

function candidatePlatformName(cand: NonNullable<AlbumTrackInfo["candidates"]>[number]): string {
  return cand.source.kind === "http_stream"
    ? cand.source.platform.toLowerCase()
    : cand.source.kind.toLowerCase();
}

function tierDisplayLabel(
  codec: string,
  tier: string,
  bitrateKbps: number | null | undefined,
): string {
  const upper = codec.toUpperCase();
  const displayCodec = upper === "OPUS" ? "Opus" : upper === "VORBIS" ? "Vorbis" : upper;
  if (tier === "lossless") return `${displayCodec} (Lossless)`;
  if (bitrateKbps) return `${displayCodec} ${bitrateKbps} kbps`;
  if (tier === "high_lossy") return `${displayCodec} (High)`;
  if (tier === "standard_lossy") return `${displayCodec} (Standard)`;
  return displayCodec;
}

/**
 * Builds the `Automatic Best Quality` card + discovered multi-source stream rows from `tracks`.
 * Honest: never claims "lossless" unless an actual Lossless candidate was resolved. Pure.
 */
export function buildCanonicalQualityPlan(
  tracks: readonly AlbumTrackInfo[],
  selectedUrls?: readonly string[],
): CanonicalQualityPlan {
  const activeTracks = tracks.filter((t) => {
    if (!t.available || !t.url) return false;
    if (!selectedUrls || selectedUrls.length === 0) return true;
    return selectedUrls.includes(t.url);
  });

  const bestSummaryCounts = new Map<string, { label: string; count: number; lossless: boolean }>();
  const sourceGroups = new Map<
    string,
    {
      platform: string;
      tier: string;
      codec: string;
      bitrate: number | null;
      count: number;
      tierRank: number;
    }
  >();

  let tracksWithCandidates = 0;
  let anyLossless = false;

  const tierRankOf = (tier: string) =>
    tier === "lossless" ? 3 : tier === "high_lossy" ? 2 : tier === "standard_lossy" ? 1 : 0;

  for (const track of activeTracks) {
    const cands = track.candidates ?? [];
    if (cands.length === 0) continue;
    tracksWithCandidates += 1;

    const top = cands[0];
    if (top.quality_tier === "lossless") anyLossless = true;
    const topPlat = candidatePlatformName(top);
    const topCodec = tierDisplayLabel(top.codec, top.quality_tier, top.bitrate_kbps);
    const topKey = `${topPlat}::${topCodec}`;
    const prevTop = bestSummaryCounts.get(topKey);
    if (prevTop) prevTop.count += 1;
    else {
      bestSummaryCounts.set(topKey, {
        label: `${topCodec} (${topPlat.toUpperCase()})`,
        count: 1,
        lossless: top.quality_tier === "lossless",
      });
    }

    const seenInTrack = new Set<string>();
    for (const c of cands) {
      const plat = candidatePlatformName(c);
      const bucket = `${plat}::${c.quality_tier}`;
      if (seenInTrack.has(bucket)) continue;
      seenInTrack.add(bucket);
      const existing = sourceGroups.get(bucket);
      if (existing) {
        existing.count += 1;
      } else {
        sourceGroups.set(bucket, {
          platform: plat,
          tier: c.quality_tier,
          codec: c.codec,
          bitrate: c.bitrate_kbps ?? null,
          count: 1,
          tierRank: tierRankOf(c.quality_tier),
        });
      }
    }
  }

  if (tracksWithCandidates === 0) {
    return {
      best: {
        id: "best",
        label: "Automatic Best Quality",
        details: "Selects the best discovered quality per track across platforms",
        isBest: true,
        lossless: false,
      },
      options: [
        {
          id: "high",
          label: "High Quality (Opus 160k / AAC 256k)",
          details: "Prefer high-bitrate lossy streams",
          platform: "youtube",
        },
        {
          id: "standard",
          label: "Standard Quality (MP3 128k)",
          details: "Prefer compact 128 kbps streams",
          platform: "bandcamp",
        },
      ],
      hasDiscoveredCandidates: false,
    };
  }

  const totalCount = activeTracks.length;
  const bestParts = [...bestSummaryCounts.values()]
    .sort((a, b) => b.count - a.count)
    .slice(0, 3)
    .map((entry) =>
      totalCount > 1 ? `${entry.count}× ${entry.label}` : entry.label,
    );

  const sortedSources = [...sourceGroups.values()].sort(
    (a, b) => b.tierRank - a.tierRank || b.count - a.count || a.platform.localeCompare(b.platform),
  );

  const options: CanonicalQualityOption[] = sortedSources.map((src) => {
    const tierSuffix =
      src.tier === "standard_lossy" ? ":standard" : src.tier === "high_lossy" ? ":high" : "";
    const coverage =
      totalCount > 1 ? `${src.count}/${totalCount} tracks` : "Verified candidate";
    return {
      id: `source:${src.platform}${tierSuffix}`,
      label: tierDisplayLabel(src.codec, src.tier, src.bitrate),
      details: coverage,
      platform: src.platform,
      lossless: src.tier === "lossless",
    };
  });

  return {
    best: {
      id: "best",
      label: "Automatic Best Quality",
      details:
        bestParts.length > 0
          ? `Expected: ${bestParts.join(", ")}`
          : "Picks highest quality per track",
      isBest: true,
      lossless: anyLossless,
    },
    options,
    hasDiscoveredCandidates: true,
  };
}
