import { describe, expect, it } from "vitest";
import {
  calculateDisplayProgress,
  progressToSliderValue,
  sliderValueToProgress,
} from "./seekProgress.ts";

describe("calculateDisplayProgress", () => {
  it("uses store progress when user is not dragging", () => {
    expect(calculateDisplayProgress(0.25, null)).toBe(0.25);
    expect(calculateDisplayProgress(0.5, null)).toBe(0.5);
  });

  it("prioritizes dragProgress over store progress while dragging", () => {
    // When dragging to 0.75, even if audio progress is at 0.1, display should stay at 0.75
    expect(calculateDisplayProgress(0.1, 0.75)).toBe(0.75);
    expect(calculateDisplayProgress(0.3, 0.75)).toBe(0.75);
  });

  it("clamps progress between 0 and 1", () => {
    expect(calculateDisplayProgress(-0.5, null)).toBe(0);
    expect(calculateDisplayProgress(1.5, null)).toBe(1);
    expect(calculateDisplayProgress(0.5, -0.2)).toBe(0);
    expect(calculateDisplayProgress(0.5, 1.2)).toBe(1);
  });

  it("handles non-finite values safely", () => {
    expect(calculateDisplayProgress(NaN, null)).toBe(0);
    expect(calculateDisplayProgress(Infinity, null)).toBe(0);
    expect(calculateDisplayProgress(0.5, NaN)).toBe(0);
  });
});

describe("progressToSliderValue", () => {
  it("converts fraction 0..1 to integer 0..1000", () => {
    expect(progressToSliderValue(0)).toBe(0);
    expect(progressToSliderValue(0.5)).toBe(500);
    expect(progressToSliderValue(1)).toBe(1000);
    expect(progressToSliderValue(0.1234)).toBe(123);
  });

  it("clamps out-of-range and non-finite values", () => {
    expect(progressToSliderValue(-0.1)).toBe(0);
    expect(progressToSliderValue(1.2)).toBe(1000);
    expect(progressToSliderValue(NaN)).toBe(0);
  });
});

describe("sliderValueToProgress", () => {
  it("converts slider integer 0..1000 to fraction 0..1", () => {
    expect(sliderValueToProgress(0)).toBe(0);
    expect(sliderValueToProgress(500)).toBe(0.5);
    expect(sliderValueToProgress(1000)).toBe(1);
  });

  it("clamps out-of-range and non-finite values", () => {
    expect(sliderValueToProgress(-50)).toBe(0);
    expect(sliderValueToProgress(1500)).toBe(1);
    expect(sliderValueToProgress(NaN)).toBe(0);
  });
});
