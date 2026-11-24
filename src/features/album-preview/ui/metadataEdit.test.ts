import { describe, expect, it } from "vitest";
import { applyMetadataPatch } from "./MetadataEditModal";
import type { AlbumPreview } from "@/shared/lib/remote/albumPreview";

describe("applyMetadataPatch", () => {
  const basePreview: AlbumPreview = {
    platform: "youtube",
    url: "canonical:mbid-1",
    title: "Original Title",
    artist: "Original Artist",
    cover_url: null,
    year: 1995,
    genres: [],
    tracks: [
      {
        index: 0,
        title: "Track 1",
        artists: ["Original Artist"],
        track_number: 1,
        url: "https://music.youtube.com/watch?v=1",
        available: true,
        note: null,
        duration_ms: 180000,
      },
      {
        index: 1,
        title: "Track 2",
        artists: ["Original Artist"],
        track_number: 2,
        url: "https://music.youtube.com/watch?v=2",
        available: true,
        note: null,
        duration_ms: 200000,
      },
    ],
  };

  it("updates album title, artist, year and specific track titles", () => {
    const updated = applyMetadataPatch(basePreview, {
      title: "New Title",
      artist: "New Artist",
      year: 2026,
      tracks: [{ index: 1, title: "Updated Track 2" }],
    });

    expect(updated.title).toBe("New Title");
    expect(updated.artist).toBe("New Artist");
    expect(updated.year).toBe(2026);
    expect(updated.tracks[0].title).toBe("Track 1");
    expect(updated.tracks[1].title).toBe("Updated Track 2");
  });

  it("preserves unmodified fields when partial patch provided", () => {
    const updated = applyMetadataPatch(basePreview, {
      title: "Just Title Changed",
    });

    expect(updated.title).toBe("Just Title Changed");
    expect(updated.artist).toBe("Original Artist");
    expect(updated.year).toBe(1995);
    expect(updated.tracks[0].title).toBe("Track 1");
  });
});
