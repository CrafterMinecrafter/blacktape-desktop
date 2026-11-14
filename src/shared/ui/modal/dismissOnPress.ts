import type { MouseEvent } from "react";

export interface DismissOnPressProps {
  /** Set while a modal covers the control, so its hover and pointer styles can be switched off. */
  "data-covered"?: "";
  onMouseDownCapture?: (e: MouseEvent) => void;
  onClickCapture?: (e: MouseEvent) => void;
}

const swallow = (e: Pick<MouseEvent, "preventDefault" | "stopPropagation">) => {
  e.preventDefault();
  e.stopPropagation();
};

/**
 * Props for a control that stays visible above an open modal (the header's search and menu). A press on it closes
 * the modal, like a press on the backdrop, and does nothing else: no focus, no menu, no click on the control.
 * It is also marked `data-covered`, so the control can drop its hover effects meanwhile.
 * Nothing is returned while no modal is open, so the control works as usual.
 */
export function dismissOnPress(
  modalOpen: boolean,
  closeModal: () => void,
): DismissOnPressProps {
  if (!modalOpen) return {};
  return {
    "data-covered": "",
    onMouseDownCapture: (e) => {
      swallow(e);
      closeModal();
    },
    onClickCapture: swallow,
  };
}
