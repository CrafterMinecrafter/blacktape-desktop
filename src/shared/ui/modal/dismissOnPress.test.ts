import { describe, expect, it, vi } from "vitest";
import { dismissOnPress } from "@/shared/ui/modal/dismissOnPress.ts";

const fakeEvent = () => ({ preventDefault: vi.fn(), stopPropagation: vi.fn() });

describe("dismissOnPress", () => {
  it("returns no handlers while no modal is open", () => {
    expect(dismissOnPress(false, vi.fn())).toEqual({});
  });

  it("closes the modal on mouse down and keeps the press from reaching the control", () => {
    const close = vi.fn();
    const event = fakeEvent();
    dismissOnPress(true, close).onMouseDownCapture?.(event as never);
    expect(close).toHaveBeenCalledTimes(1);
    expect(event.preventDefault).toHaveBeenCalled();
    expect(event.stopPropagation).toHaveBeenCalled();
  });

  it("swallows the click that follows without closing anything again", () => {
    const close = vi.fn();
    const event = fakeEvent();
    dismissOnPress(true, close).onClickCapture?.(event as never);
    expect(close).not.toHaveBeenCalled();
    expect(event.preventDefault).toHaveBeenCalled();
    expect(event.stopPropagation).toHaveBeenCalled();
  });
});
