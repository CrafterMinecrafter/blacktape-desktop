import { describe, it, expect } from "vitest";
import {
  downloadingAlbumMetaLine,
  isActiveLibraryDownload,
  normalizeAlbumTitle,
  matchDownloadToLibraryAlbum,
  extraDownloadingTracksForAlbum,
  unifiedAlbumKey,
} from "./artistAlbumsDownloads.ts";
import { ItemType, type DownloadProgress } from "@/shared/store/libraryStore.ts";

function dl(over: Partial<DownloadProgress> = {}): DownloadProgress {
  return {
    id: "dl-1",
    url: "https://example.test/album/morrison-hotel",
    platform: "bandcamp",
    itemType: ItemType.Album,
    artistName: "The Doors",
    albumTitle: "Morrison Hotel",
    overallPercent: 20,
    current: 2,
    total: 11,
    message: "Downloading...",
    status: "downloading",
    tracks: [
      { title: "Roadhouse Blues", status: "downloading", percent: 50, trackNumber: 1 },
      { title: "Waiting for the Sun", status: "pending", percent: 0, trackNumber: 2 },
    ],
    ...over,
  };
}

describe("artistAlbumsDownloads", () => {
  it("detects active downloads and ignores completed/failed/cancelled or resolve URLs", () => {
    expect(isActiveLibraryDownload(dl({ status: "downloading" }))).toBe(true);
    expect(isActiveLibraryDownload(dl({ status: "idle" }))).toBe(true);
    expect(isActiveLibraryDownload(dl({ status: "processing" }))).toBe(true);
    expect(isActiveLibraryDownload(dl({ status: "downloading", url: "resolve:abc" }))).toBe(false);
    expect(isActiveLibraryDownload(dl({ status: "completed" }))).toBe(false);
    expect(isActiveLibraryDownload(dl({ status: "failed" }))).toBe(false);
    expect(isActiveLibraryDownload(dl({ status: "cancelled" }))).toBe(false);
  });

  it("normalizes album titles by trimming and lowercasing", () => {
    expect(normalizeAlbumTitle("  Morrison Hotel  ")).toBe("morrison hotel");
    expect(normalizeAlbumTitle("THE MONEY STORE")).toBe("the money store");
  });

  it("matches downloads to existing library albums by linked source URL or normalized album title", () => {
    const albums = [{ title: "Morrison Hotel" }, { title: "L.A. Woman" }];
    const missingSources = {
      "Morrison Hotel": {
        album: "Morrison Hotel",
        platform: "bandcamp" as const,
        source_url: "https://doors.bandcamp.com/album/morrison-hotel",
        cover_url: "",
        total: 11,
        owned: 0,
        missing: 11,
      },
    };

    expect(
      matchDownloadToLibraryAlbum(
        dl({ url: "https://doors.bandcamp.com/album/morrison-hotel", albumTitle: "Other Title" }),
        albums,
        missingSources,
        true,
      ),
    ).toBe("Morrison Hotel");

    expect(
      matchDownloadToLibraryAlbum(
        dl({ url: "https://youtube.com/playlist?list=123", albumTitle: "morrison hotel" }),
        albums,
        missingSources,
        true,
      ),
    ).toBe("Morrison Hotel");

    expect(
      matchDownloadToLibraryAlbum(
        dl({ url: "https://youtube.com/playlist?list=999", albumTitle: "Strange Days" }),
        albums,
        missingSources,
        true,
      ),
    ).toBeUndefined();
  });

  it("filters out downloading tracks that already exist in the library album or missing tracks", () => {
    const librarySongs = [{ title: "Roadhouse Blues" }];
    const extra = extraDownloadingTracksForAlbum(librarySongs, ["Peace Frog"], [dl()]);
    expect(extra.map((t) => t.title)).toEqual(["Waiting for the Sun"]);
  });

  it("produces identical unifiedAlbumKey across downloading and library albums and formats release year meta", () => {
    expect(unifiedAlbumKey("Aphex Twin", " Drukqs ")).toBe(
      unifiedAlbumKey("aphex twin", "drukqs"),
    );
    expect(downloadingAlbumMetaLine({ releaseYear: 2001 })).toBe("2001");
    expect(downloadingAlbumMetaLine({ releaseYear: undefined })).toBeNull();
  });
});
