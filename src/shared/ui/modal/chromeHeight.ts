import { useEffect } from "react";

export const CHROME_HEIGHT_VAR = "--chrome-height";

/** Height of the window's header in px: the main window's header and the Settings window's differ. */
export const MAIN_HEADER_HEIGHT = 48;
export const SETTINGS_HEADER_HEIGHT = 40;

interface StyleTarget {
  style: {
    setProperty(name: string, value: string): void;
    removeProperty(name: string): unknown;
  };
}

/** Publishes the header height as a CSS variable, so the header, the modal backdrop and the dimming
 *  over the header all use one number. Returns a function that removes it again. */
export function applyChromeHeight(root: StyleTarget, px: number): () => void {
  root.style.setProperty(CHROME_HEIGHT_VAR, `${px}px`);
  return () => {
    root.style.removeProperty(CHROME_HEIGHT_VAR);
  };
}

export function useChromeHeight(px: number): void {
  useEffect(() => applyChromeHeight(document.documentElement, px), [px]);
}
