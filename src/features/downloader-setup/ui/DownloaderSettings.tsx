import { useEffect, useState } from "react";
import {
  getDefaultDownloadQuality,
  setDefaultDownloadQuality,
  type DownloadQualityPreset,
} from "@/shared/lib/audio.ts";
import {
  anyBusy,
  anyFailed,
  TOOL_LABELS,
  TOOL_NAMES,
  TOOL_PURPOSE,
  toolDetails,
  toolStatusText,
} from "../model/toolsStatus.ts";
import { useToolsStore } from "../model/toolsStore.ts";
import { useToolsStatus } from "./useToolsStatus.ts";
import styles from "./downloader-setup.module.css";

interface Props {
  rowClassName: string;
  buttonClassName: string;
  mutedClassName: string;
  selectClassName?: string;
}

/** Settings rows for Default Download Quality and yt-dlp, deno and ffmpeg setup/updates. */
export const DownloaderSettings = ({
  rowClassName,
  buttonClassName,
  mutedClassName,
  selectClassName,
}: Props) => {
  const status = useToolsStatus();
  const check = useToolsStore((s) => s.check);
  const busy = anyBusy(status);
  const [defaultQuality, setDefaultQuality] = useState<DownloadQualityPreset>("best");

  useEffect(() => {
    getDefaultDownloadQuality()
      .then(setDefaultQuality)
      .catch((err) => console.error("Failed to load default download quality:", err));
  }, []);

  const handleQualityChange = async (next: DownloadQualityPreset) => {
    const prev = defaultQuality;
    setDefaultQuality(next);
    try {
      await setDefaultDownloadQuality(next);
    } catch (err) {
      console.error("Failed to save default download quality:", err);
      setDefaultQuality(prev);
    }
  };

  return (
    <>
      <div className={rowClassName}>
        <label htmlFor="default-download-quality-select" className={styles.toolName}>
          <span>Default Download Quality</span>
          <span className={mutedClassName}>
            Used when clicking Download directly without opening the quality picker.
          </span>
        </label>
        <select
          id="default-download-quality-select"
          className={selectClassName}
          value={defaultQuality}
          onChange={(e) => void handleQualityChange(e.target.value as DownloadQualityPreset)}
        >
          <option value="best">Automatic Best Quality (Lossless / Opus 160k)</option>
          <option value="high">High Quality (Opus 160k / AAC 256k)</option>
          <option value="standard">Standard Quality (MP3 128k)</option>
        </select>
      </div>
      {TOOL_NAMES.map((name) => {
        const tool = status?.[name];
        const details = tool ? toolDetails(tool) : "";
        return (
          <div key={name} className={rowClassName}>
            <span className={styles.toolName}>
              <span>
                {TOOL_LABELS[name]}
                {details && <span className={mutedClassName}> {details}</span>}
              </span>
              <span className={mutedClassName}>{TOOL_PURPOSE[name]}</span>
            </span>
            <span
              className={tool?.phase === "failed" ? styles.toolFailed : `${mutedClassName} ${styles.toolStatus}`}
              title={tool?.error ?? undefined}
            >
              {tool ? toolStatusText(tool) : "Loading…"}
            </span>
          </div>
        );
      })}
      <div className={rowClassName}>
        <span className={mutedClassName}>
          yt-dlp is checked for updates daily, Deno and FFmpeg weekly. If Deno or FFmpeg is already installed on your system, Blacktape uses it.
        </span>
        <button
          className={buttonClassName}
          onClick={() => check().catch(console.error)}
          disabled={busy || !status}
        >
          {anyFailed(status) ? "Retry" : "Check for updates"}
        </button>
      </div>
    </>
  );
};
