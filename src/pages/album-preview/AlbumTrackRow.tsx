import { useState, useRef, useEffect, type MouseEvent } from "react";
import clsx from "clsx";
import { HiOutlineMusicNote } from "react-icons/hi";
import { HiMiniPause } from "react-icons/hi2";
import { DownloadSplitButton } from "@/shared/ui/download-split-button/DownloadSplitButton.tsx";
import { MdKeyboardArrowDown, MdPause, MdPlayArrow } from "react-icons/md";
import { FiCheck } from "react-icons/fi";
import rowStyles from "@/features/library/ui/song-list-item/song-list-item.module.css";
import { useStreamStore } from "@/features/preview/model/streamStore.ts";
import { DownloadStatusBadge } from "@/shared/ui/download-status-badge/DownloadStatusBadge.tsx";
import { flyToLibrary } from "@/shared/lib/ui/flyAnimation.ts";
import {
  isTrackRowActive,
  trackRowStatus,
  type AlbumTrackInfo,
  type EstimatedTrackQuality,
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

const shortPlatformTag = (cand: AudioCandidate): string => {
  const plat = cand.source.kind === "http_stream" ? cand.source.platform.toLowerCase() : cand.source.kind;
  switch (plat) {
    case "youtube":
      return "YT";
    case "bandcamp":
      return "BC";
    case "soundcloud":
      return "SC";
    case "torrent":
      return "P2P";
    default:
      return plat.toUpperCase();
  }
};

const candidateShortBadge = (cand: AudioCandidate) => {
  const plat = shortPlatformTag(cand);
  const codec = cand.codec.toUpperCase();
  switch (cand.quality_tier) {
    case "lossless":
      return `${plat} · ${codec} · Lossless`;
    case "high_lossy":
      return `${plat} · ${codec} · High`;
    case "standard_lossy":
      return `${plat} · ${codec} · Standard`;
    case "low":
    default:
      return `${plat} · ${codec} · Low`;
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
  albumTitle?: string | null;
  resolving?: boolean;
  estimate?: EstimatedTrackQuality;
  localSongId?: number;
  editing?: boolean;
  onChangeTitle?: (title: string) => void;
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
  albumTitle,
  resolving = false,
  estimate,
  localSongId,
  editing = false,
  onChangeTitle,
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
  const active = isTrackRowActive(track, localSongId, currentSong, albumTitle);
  const playing = active && isPlaying && !(isStreaming && streamLoading);
  const loading = track.available && (loadingUrl === track.url || (active && isStreaming && streamLoading));
  const status = trackRowStatus(track, libraryState, task);

  return (
    <div
      className={clsx(rowStyles.song, styles.trackRow, {
        [rowStyles.playing]: active,
        [styles.playing]: active,
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
      onDoubleClick={() => track.available && !editing && onPlay()}
      onContextMenu={track.available ? onContextMenu : undefined}
    >
      <div className={rowStyles.songLeft}>
        <div className={rowStyles.numSlot}>
          <div className={clsx(rowStyles.numContent, loading && styles.numHidden)}>
            {active ? (
              isPlaying ? (
                <HiOutlineMusicNote className={rowStyles.playingIcon} />
              ) : (
                <HiMiniPause className={rowStyles.pauseIcon} />
              )
            ) : (
              <div className={rowStyles.songTrackNumber}>
                {String(track.track_number ?? position + 1).padStart(2, "0")}
              </div>
            )}
          </div>
          {track.available && (
            <button
              className={clsx(rowStyles.rowPlayButton, loading && styles.alwaysShown)}
              title={playing ? "Pause" : localSongId !== undefined ? "Play" : "Play preview"}
              aria-label={playing ? "Pause" : localSongId !== undefined ? "Play" : "Play preview"}
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
        {editing ? (
          <input
            type="text"
            className={styles.inlineTrackInput}
            value={track.title}
            onClick={(e) => e.stopPropagation()}
            onDoubleClick={(e) => e.stopPropagation()}
            onChange={(e) => onChangeTitle?.(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" || e.key === "Escape") {
                e.currentTarget.blur();
              }
            }}
            aria-label={`Track ${track.track_number ?? position + 1} title`}
          />
        ) : (
          <div className={styles.trackTitleWrap}>
            <span className={styles.trackTitleText}>{decodeHtmlEntities(track.title)}</span>
            {track.artists.length > 0 && (
              <span className={styles.trackArtist}>
                {track.artists.join(", ")}
              </span>
            )}
          </div>
        )}
      </div>
      <div className={rowStyles.songRight}>
        {!track.available && track.note && (
          <div className={clsx(styles.note, "truncate")}>{track.note}</div>
        )}
        {track.candidates && track.candidates.length > 0 ? (
          <CandidateSelector
            candidates={track.candidates}
            selectedIdx={track.selected_candidate_idx ?? 0}
            onSelect={onSelectCandidate}
          />
        ) : (
          resolving &&
          track.available &&
          estimate && (
            <span
              className={clsx(styles.estimateBadge, styles[estimate.tier])}
              title="Checking exact multi-source audio quality…"
            >
              <span className={styles.miniSpinner} aria-hidden="true" />
              <span>{estimate.label}</span>
            </span>
          )
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
