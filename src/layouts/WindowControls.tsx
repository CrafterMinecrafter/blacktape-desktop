import { useEffect, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import styles from "./window-controls.module.css";
import MinimizeIcon from "@/assets/icons/minimize.svg?react";
import MaximizeIcon from "@/assets/icons/maximize.svg?react";
import RestoreIcon from "@/assets/icons/restore.svg?react";
import CloseIcon from "@/assets/icons/close.svg?react";
import { maximizeButtonLabel } from "@/layouts/maximizeButton.ts";

const appWindow = getCurrentWindow();

// Follows the window's maximized state, also when it changes outside these buttons
// (double-click on the header, Win+Up, dragging to the screen edge).
function useMaximized(): boolean {
  const [maximized, setMaximized] = useState(false);

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    const update = () =>
      appWindow
        .isMaximized()
        .then((value) => {
          if (!disposed) setMaximized(value);
        })
        .catch(console.error);

    update();
    appWindow
      .onResized(update)
      .then((fn) => {
        if (disposed) fn();
        else unlisten = fn;
      })
      .catch(console.error);

    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  return maximized;
}

export const WindowControls = () => {
  const maximized = useMaximized();
  const label = maximizeButtonLabel(maximized);

  return (
    <div className={styles.controls}>
      <button
        onClick={() => appWindow.minimize()}
        className={styles.button}
        aria-label="Minimize"
      >
        <MinimizeIcon width={12} height={12} />
      </button>
      <button
        onClick={() => appWindow.toggleMaximize()}
        className={styles.button}
        aria-label={label}
        title={label}
      >
        {maximized ? (
          <RestoreIcon width={12} height={12} />
        ) : (
          <MaximizeIcon width={12} height={12} />
        )}
      </button>
      <button
        onClick={() => appWindow.close()}
        className={`${styles.button} ${styles.close}`}
        aria-label="Close"
      >
        <CloseIcon width={12} height={12} />
      </button>
    </div>
  );
};
