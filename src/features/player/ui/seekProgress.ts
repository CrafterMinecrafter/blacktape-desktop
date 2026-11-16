export const calculateDisplayProgress = (
  progress: number,
  dragProgress: number | null,
): number => {
  const p = dragProgress ?? progress;
  if (!Number.isFinite(p)) return 0;
  return Math.min(1, Math.max(0, p));
};

export const progressToSliderValue = (progress: number): number => {
  if (!Number.isFinite(progress)) return 0;
  return Math.min(1000, Math.max(0, Math.round(progress * 1000)));
};

export const sliderValueToProgress = (value: number): number => {
  if (!Number.isFinite(value)) return 0;
  return Math.min(1, Math.max(0, value / 1000));
};
