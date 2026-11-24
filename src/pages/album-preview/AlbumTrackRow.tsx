import { useState, useRef, useEffect, type MouseEvent } from "react";
import clsx from "clsx";
import { DownloadSplitButton } from "@/shared/ui/download-split-button/DownloadSplitButton.tsx";
import { MdKeyboardArrowDown, MdPause, MdPlayArrow } from "react-icons/md";
import { FiCheck } from "react-icons/fi";
import rowStyles from "@/features/library/ui/song-list-item/song-list-item.module.css";
import { isStreamingUrl, useStreamStore } from "@/features/preview/model/streamStore.ts";
import { DownloadStatusBadge } from "@/shared/ui/download-status-badge/DownloadStatusBadge.tsx";
import { flyToLibrary } from "@/shared/lib/ui/flyAnimation.ts";
import {
  trackRowStatus,
  type AlbumTrackInfo,
  type TrackTaskState,
} from "@/shared/lib/remote/albumPreview.ts";
import type { AudioCandidate } from "@/shared/types/canonical.ts";
import type { DownloadState } from "@/shared/lib/library/libraryIndex.ts";
import { formatTrackDuration } from "@/shared/lib/library/trackDuration.ts";
import { decodeHtmlEntities } from "@/shared/lib/ui/html.ts";
import { useAudioStore } from "@/shared/store/audioStore.ts";
import styles from "./album-preview.module.css";

const tierClass = (tier: string) => {
  switch (tier) {
    case "lossless":
      return styles.lossless;
    case "high_lossy":
      return styles.high;
    case "standard_lossy":
      return styles.standard;
    case "low":
    default:
      return styles.low;
  }
};

const candidateShortBadge = (cand: AudioCandidate) => {
  const codec = cand.codec.toUpperCase();
  switch (cand.quality_tier) {
    case "lossless":
      return `${codec} · Lossless`;
    case "high_lossy":
      return `${codec} · High`;
    case "standard_lossy":
      return `${codec} · Standard`;
    case "low":
    default:
      return `${codec} · Low`;
  }
};

const CandidateSelector = ({
  candidates,
  selectedIdx,
  onSelect,
}: {
  candidates: AudioCandidate[];
  selectedIdx: number;
  onSelect?: (idx: number) => void;
}) => {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);
  const selected = candidates[selectedIdx] ?? candidates[0];

  useEffect(() => {
    if (!open) return;
    const handleClickOutside = (e: globalThis.MouseEvent | PointerEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) {
        setOpen(false);
      }
    };
    document.addEventListener("pointerdown", handleClickOutside);
    return () => document.removeEventListener("pointerdown", handleClickOutside);
  }, [open]);

  if (!selected) return null;

  return (
    <div
      ref={ref}
      className={styles.candidateSelector}
      onClick={(e) => e.stopPropagation()}
      onDoubleClick={(e) => e.stopPropagation()}
    >
      <button
        type="button"
        className={clsx(styles.candidateBadgeButton, tierClass(selected.quality_tier))}
        onClick={() => setOpen((v) => !v)}
        title={`Audio source: ${selected.display_label} (${candidates.length} candidate${candidates.length > 1 ? "s" : ""})`}
      >
        <span>{candidateShortBadge(selected)}</span>
        {candidates.length > 1 && (
          <MdKeyboardArrowDown className={clsx(styles.arrowIcon, open && styles.arrowOpen)} />
        )}
      </button>

      {open && candidates.length > 1 && (
        <div className={styles.candidateMenu} role="menu">
          <div className={styles.candidateMenuHeader}>
            Available Audio Sources ({candidates.length})
          </div>
          {candidates.map((cand, idx) => {
            const isSelected = idx === selectedIdx;
            return (
              <button
                key={idx}
                type="button"
                role="menuitem"
                className={clsx(
                  styles.candidateMenuItem,
                  isSelected && styles.candidateMenuItemActive,
                )}
                onClick={() => {
                  onSelect?.(idx);
                  setOpen(false);
                }}
              >
                <span className={clsx(styles.candidateItemBadge, tierClass(cand.quality_tier))}>
                  {cand.codec.toUpperCase()}
                </span>
                <div className={styles.candidateDetails}>
                  <span className={styles.candidateTitle}>{cand.display_label}</span>
                  <span className={styles.candidateMeta}>
                    {cand.is_strict_match ? "Strict duration match" : "Similar title"}
                    {cand.bitrate_kbps ? ` · ${cand.bitrate_kbps} kbps` : ""}
                  </span>
                </div>
                {isSelected && <FiCheck className={styles.candidateCheck} />}
              </button>
            );
          })}
        </div>
      )}
    </div>
  );
};

export interface AlbumTrackRowProps {
  track: AlbumTrackInfo;
  position: number;
  selected: boolean;
  selectedAbove: boolean;
  selectedBelow: boolean;
  /** Library state of the track (from `useTrackStatuses`). */
  libraryState: DownloadState | undefined;
  /** Download task that serves this row, when there is one. */
  task: TrackTaskState | undefined;
  cover: string | null;
  onSelect: (e: MouseEvent) => void;
  onPlay: () => void;
  onDownload: () => void;
  /** Opens the quality menu for this track. */
  onChooseQuality: () => void;
  onSelectCandidate?: (candidateIdx: number) => void;
  onContextMenu: (e: MouseEvent) => void;
}

/** One track of the album preview: number, play button, title, then duration and the download slot. */
export const AlbumTrackRow = ({
  track,
  position,
  selected,
  selectedAbove,
  selectedBelow,
  libraryState,
  task,
  cover,
  onSelect,
  onPlay,
  onDownload,
  onChooseQuality,
  onSelectCandidate,
  onContextMenu,
}: AlbumTrackRowProps) => {
  const currentSong = useAudioStore((s) => s.currentSong);
  const isStreaming = useAudioStore((s) => s.isStreaming);
  const isPlaying = useAudioStore((s) => s.isPlaying);
  const streamLoading = useAudioStore((s) => s.streamLoading);
  const loadingUrl = useStreamStore((s) => s.pendingUrl);
  const active = track.available && isStreamingUrl(currentSong, isStreaming, track.url);
  const playing = active && isPlaying && !streamLoading;
  const loading = track.available && (loadingUrl === track.url || (active && streamLoading));
  const status = trackRowStatus(track, libraryState, task);

  return (
    <div
      className={clsx(rowStyles.song, styles.trackRow, {
        [rowStyles.selected]: selected,
        [rowStyles.selectedTop]: selected && !selectedAbove,
        [rowStyles.selectedBottom]: selected && !selectedBelow,
        [styles.unavailable]: !track.available,
      })}
      data-song-row
      aria-selected={selected}
      aria-disabled={!track.available}
      title={track.available ? undefined : (track.note ?? "Not available")}
      role="button"
      tabIndex={0}
      onClick={(e) => track.available && onSelect(e)}
      onDoubleClick={() => track.available && onPlay()}
      onContextMenu={track.available ? onContextMenu : undefined}
    >
      <div className={rowStyles.songLeft}>
        <div className={rowStyles.numSlot}>
          <div className={clsx(rowStyles.numContent, (active || loading) && styles.numHidden)}>
            <div className={rowStyles.songTrackNumber}>
              {String(track.track_number ?? position + 1).padStart(2, "0")}
            </div>
          </div>
          {track.available && (
            <button
              className={clsx(rowStyles.rowPlayButton, (active || loading) && styles.alwaysShown)}
              title={playing ? "Pause" : "Play preview"}
              aria-label={playing ? "Pause" : "Play preview"}
              onClick={(e) => {
                e.stopPropagation();
                onPlay();
              }}
              onDoubleClick={(e) => e.stopPropagation()}
            >
              {loading ? (
                <span className={styles.spinner} aria-hidden="true" />
              ) : playing ? (
                <MdPause size={22} />
              ) : (
                <MdPlayArrow size={22} className={rowStyles.playIcon} />
              )}
            </button>
          )}
        </div>
        <div className={clsx(rowStyles.songTitle, "truncate")}>
          {decodeHtmlEntities(track.title)}
          {track.artists.length > 0 && (
            <span className={styles.trackArtist}> · {track.artists.join(", ")}</span>
          )}
        </div>
      </div>
      <div className={rowStyles.songRight}>
        {!track.available && track.note && (
          <div className={clsx(styles.note, "truncate")}>{track.note}</div>
        )}
        {track.candidates && track.candidates.length > 0 && (
          <CandidateSelector
            candidates={track.candidates}
            selectedIdx={track.selected_candidate_idx ?? 0}
            onSelect={onSelectCandidate}
          />
        )}
        <span className={styles.duration}>{formatTrackDuration(track.duration_ms)}</span>
        <div className={styles.rowAction}>
          {status.kind === "downloaded" && <DownloadStatusBadge state="downloaded" size={16} />}
          {status.kind === "running" && (
            <span
              className={clsx(
                styles.percent,
                status.percent >= 100
                  ? styles.percentCompleted
                  : status.percent > 0
                    ? styles.percentActive
                    : styles.percentPending,
              )}
              aria-live="polite"
            >
              {status.percent}%
            </span>
          )}
          {status.kind === "download" && (
            <span onDoubleClick={(e) => e.stopPropagation()}>
              <DownloadSplitButton
                size={30}
                className={styles.rowSplit}
                title={`Download ${track.title}`}
                onDownload={(e) => {
                  flyToLibrary(e.currentTarget.getBoundingClientRect(), cover ?? undefined);
                  onDownload();
                }}
                onChooseQuality={onChooseQuality}
              />
            </span>
          )}
        </div>
      </div>
    </div>
  );
};
