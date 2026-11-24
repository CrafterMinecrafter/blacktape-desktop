import { useState } from "react";
import clsx from "clsx";
import { MdClose } from "react-icons/md";
import type { AlbumPreview } from "@/shared/lib/remote/albumPreview.ts";
import modalStyles from "@/features/library/ui/find-missing-modal.module.css";
import styles from "./metadata-edit-modal.module.css";

export interface MetadataEditModalProps {
  preview: AlbumPreview;
  isOpen: boolean;
  onClose: () => void;
  onSave: (updated: {
    title: string;
    artist: string;
    year: number | null;
    tracks: Array<{ index: number; title: string }>;
  }) => void;
}

export function applyMetadataPatch(
  preview: AlbumPreview,
  patch: {
    title?: string;
    artist?: string;
    year?: number | null;
    tracks?: Array<{ index: number; title: string }>;
  },
): AlbumPreview {
  const updatedTracks = preview.tracks.map((t) => {
    const override = patch.tracks?.find((p) => p.index === t.index);
    return override ? { ...t, title: override.title } : t;
  });

  return {
    ...preview,
    title: patch.title !== undefined ? patch.title : preview.title,
    artist: patch.artist !== undefined ? patch.artist : preview.artist,
    year: patch.year !== undefined ? patch.year : preview.year,
    tracks: updatedTracks,
  };
}

export const MetadataEditModal = ({
  preview,
  isOpen,
  onClose,
  onSave,
}: MetadataEditModalProps) => {
  const [title, setTitle] = useState(preview.title);
  const [artist, setArtist] = useState(preview.artist);
  const [yearStr, setYearStr] = useState(preview.year ? String(preview.year) : "");
  const [tracks, setTracks] = useState(
    preview.tracks.map((t) => ({ index: t.index, title: t.title })),
  );

  if (!isOpen) return null;

  const handleSave = () => {
    const year = yearStr.trim() ? parseInt(yearStr.trim(), 10) : null;
    onSave({
      title: title.trim(),
      artist: artist.trim(),
      year: isNaN(year as number) ? null : year,
      tracks,
    });
    onClose();
  };

  return (
    <div className={modalStyles.backdrop} onClick={onClose} role="dialog" aria-modal="true">
      <div
        className={clsx(modalStyles.modal, styles.editModal)}
        onClick={(e) => e.stopPropagation()}
      >
        <div className={styles.header}>
          <h2 className={styles.heading}>Edit Album Metadata</h2>
          <button type="button" className={styles.closeBtn} onClick={onClose} aria-label="Close">
            <MdClose size={20} />
          </button>
        </div>

        <div className={styles.body}>
          <div className={styles.fieldGroup}>
            <label className={styles.label}>
              Album Title
              <input
                type="text"
                className={styles.input}
                value={title}
                onChange={(e) => setTitle(e.target.value)}
              />
            </label>
            <label className={styles.label}>
              Artist
              <input
                type="text"
                className={styles.input}
                value={artist}
                onChange={(e) => setArtist(e.target.value)}
              />
            </label>
            <label className={styles.label}>
              Year
              <input
                type="number"
                className={styles.input}
                value={yearStr}
                onChange={(e) => setYearStr(e.target.value)}
                placeholder="e.g. 1995"
              />
            </label>
          </div>

          <h3 className={styles.subheading}>Track Titles</h3>
          <div className={styles.trackList}>
            {tracks.map((t, idx) => (
              <div key={t.index} className={styles.trackRow}>
                <span className={styles.trackNum}>{String(idx + 1).padStart(2, "0")}</span>
                <input
                  type="text"
                  className={clsx(styles.input, styles.trackInput)}
                  value={t.title}
                  onChange={(e) => {
                    const newTracks = [...tracks];
                    newTracks[idx] = { ...t, title: e.target.value };
                    setTracks(newTracks);
                  }}
                />
              </div>
            ))}
          </div>
        </div>

        <div className={styles.footer}>
          <button type="button" className={styles.cancelBtn} onClick={onClose}>
            Cancel
          </button>
          <button type="button" className={styles.saveBtn} onClick={handleSave}>
            Save Changes
          </button>
        </div>
      </div>
    </div>
  );
};
