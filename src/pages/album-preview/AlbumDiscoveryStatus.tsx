import type { TorrentDiscovery } from "@/features/download-quality/lib/useTorrentDiscovery.ts";
import type { AlbumTrackInfo } from "@/shared/lib/remote/albumPreview.ts";
import styles from "./album-preview.module.css";

export function AlbumDiscoveryStatus({
  loadingEditions, switchingRelease, resolvingCandidates, tracks, torrent,
}: {
  loadingEditions: boolean;
  switchingRelease: boolean;
  resolvingCandidates: boolean;
  tracks: readonly AlbumTrackInfo[];
  torrent: TorrentDiscovery;
}) {
  const status: { text: string; busy: boolean }[] = [];
  if (loadingEditions) {
    status.push({ text: "Looking up editions on MusicBrainz and other platforms…", busy: true });
  }
  if (switchingRelease) {
    status.push({ text: "Loading selected edition…", busy: true });
  } else if (resolvingCandidates) {
    const available = tracks.filter((track) => track.available);
    const ready = available.filter((track) =>
      track.candidates?.some((candidate) => candidate.source.kind === "http_stream"),
    ).length;
    status.push({ text: `Checking streaming sources · ${ready}/${available.length} tracks with sources`, busy: true });
  }
  if (torrent.phase === "searching" || torrent.phase === "estimated") {
    const progress = torrent.progress;
    const text = progress?.stage === "releases"
      ? `Matching torrent files · ${progress.checked}/${progress.total} releases checked`
      : progress
        ? `Searching torrents · ${progress.checked}/${progress.total} indexes checked`
        : "Searching torrent indexes…";
    status.push({ text, busy: true });
  }
  if (torrent.phase === "done") {
    const covered = new Set(torrent.resolutions.flatMap((release) => release.tracks.map((track) => track.expected_index)));
    const result = torrent.failed
      ? "Torrent search failed"
      : covered.size > 0
        ? `Torrent sources matched ${covered.size}/${tracks.length} tracks`
        : "No matching torrent sources found";
    const unavailable = torrent.unavailableProviders?.join(", ");
    status.push({ text: unavailable ? `${result} · Unavailable indexes: ${unavailable}` : result, busy: false });
  }
  return (
    <div className={styles.discoveryStatus} aria-live="polite">
      {status.map(({ text, busy }) => (
        <span key={text} className={styles.discoveryStep}>
          {busy && <span className={styles.miniSpinner} aria-hidden="true" />}
          {text}
        </span>
      ))}
    </div>
  );
}
