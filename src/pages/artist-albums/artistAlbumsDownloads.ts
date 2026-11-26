import type { Album, Song } from "@/shared/types.ts";
import type { LibraryAlbumSource } from "@/shared/lib/library/libraryIndex.ts";
import { sameUrl } from "@/shared/lib/remote/urlKey.ts";
import type { DownloadProgress } from "@/shared/store/libraryStore.ts";

const ACTIVE_STATUSES = new Set(["downloading", "idle", "processing"]);

export function groupMissingSelection<T extends { album: string; track: { url: string } }>(
  tracks: readonly T[],
  canDownload: (url: string) => boolean,
): Map<string, T[]> {
  const groups = new Map<string, T[]>();
  for (const item of tracks) {
    if (!canDownload(item.track.url)) continue;
    const group = groups.get(item.album);
    if (group) group.push(item);
    else groups.set(item.album, [item]);
  }
  return groups;
}

export function isActiveLibraryDownload(d: Pick<DownloadProgress, "status" | "url">): boolean {
  return ACTIVE_STATUSES.has(d.status) && !d.url.startsWith("resolve:");
}

export function normalizeAlbumTitle(title: string): string {
  return title.trim().toLowerCase();
}

export async function prepareAlbumDeletion(
  album: Pick<Album, "title"> & { songs: readonly Pick<Song, "id">[] },
  artist: string,
  downloads: readonly DownloadProgress[],
  linkedSource: string | undefined,
  cancel: (id: string) => Promise<void>,
  fetchAlbums: (artist: string) => Promise<Album[]>,
): Promise<Song[]> {
  const songIds = new Set(album.songs.map((song) => song.id));
  const matching = downloads.filter((task) => {
    if (!ACTIVE_STATUSES.has(task.status)) return false;
    if (task.replaceSongId !== undefined && songIds.has(task.replaceSongId)) return true;
    if ([task.url, ...(task.trackUrls ?? [])].some((url) => {
      const match = /^resolve:song:(\d+)$/.exec(url);
      return match && songIds.has(Number(match[1]));
    })) return true;
    if (linkedSource && sameUrl(task.url, linkedSource)) return true;
    return unifiedAlbumKey(task.artistName, task.albumTitle) === unifiedAlbumKey(artist, album.title);
  });
  await Promise.all(matching.map((task) => cancel(task.id)));
  const albums = await fetchAlbums(artist);
  return albums.find((item) => normalizeAlbumTitle(item.title) === normalizeAlbumTitle(album.title))?.songs ?? [];
}

/** Finds the library album title an active download belongs to (by linked source URL first, then normalized album title). */
export function matchDownloadToLibraryAlbum(
  d: Pick<DownloadProgress, "url" | "itemType" | "albumTitle">,
  albums: readonly Pick<Album, "title">[],
  missingSources: Readonly<Record<string, LibraryAlbumSource>>,
  missingReady: boolean,
): string | undefined {
  if (missingReady) {
    const byUrl = Object.entries(missingSources).find(([, source]) =>
      sameUrl(source.source_url, d.url),
    )?.[0];
    if (byUrl !== undefined) return byUrl;
  }
  const norm = normalizeAlbumTitle(d.albumTitle);
  if (!norm) return undefined;
  return albums.find((a) => normalizeAlbumTitle(a.title) === norm)?.title;
}

export interface IntegratedDownloadingTrack {
  title: string;
  trackNumber: number;
  discNumber: number;
  percent: number;
  status: "pending" | "idle" | "downloading" | "completed" | "failed";
}

export function unifiedAlbumKey(artist: string, albumTitle: string): string {
  return `${artist.trim().toLowerCase()}::${normalizeAlbumTitle(albumTitle)}`;
}

export function downloadingAlbumMetaLine(
  d: Pick<DownloadProgress, "releaseYear">,
): string | null {
  return typeof d.releaseYear === "number" && d.releaseYear > 0
    ? String(d.releaseYear)
    : null;
}

/**
 * Extracts downloading tracks belonging to `album` that are not already represented by a saved
 * library song or an already-displayed missing-track row.
 */
export function extraDownloadingTracksForAlbum(
  albumSongs: readonly Pick<Song, "title">[],
  missingTrackTitles: readonly string[],
  downloadsForAlbum: readonly DownloadProgress[],
): IntegratedDownloadingTrack[] {
  const existingTitles = new Set<string>();
  for (const s of albumSongs) existingTitles.add(normalizeAlbumTitle(s.title));
  for (const t of missingTrackTitles) existingTitles.add(normalizeAlbumTitle(t));

  const out: IntegratedDownloadingTrack[] = [];
  const seenDownloading = new Set<string>();

  for (const d of downloadsForAlbum) {
    if (d.tracks && d.tracks.length > 0) {
      d.tracks.forEach((tr, idx) => {
        const key = normalizeAlbumTitle(tr.title);
        if (!key || existingTitles.has(key) || seenDownloading.has(key)) return;
        seenDownloading.add(key);
        out.push({
          title: tr.title,
          trackNumber: tr.trackNumber ?? idx + 1,
          discNumber: tr.discNumber ?? 1,
          percent: tr.percent ?? 0,
          status: tr.status,
        });
      });
    } else if (d.placement?.title) {
      const key = normalizeAlbumTitle(d.placement.title);
      if (key && !existingTitles.has(key) && !seenDownloading.has(key)) {
        seenDownloading.add(key);
        out.push({
          title: d.placement.title,
          trackNumber: d.placement.track_number ?? albumSongs.length + 1,
          discNumber: 1,
          percent: d.overallPercent ?? 0,
          status: d.status === "completed" ? "completed" : "downloading",
        });
      }
    }
  }

  out.sort((a, b) => a.discNumber - b.discNumber || a.trackNumber - b.trackNumber);
  return out;
}
