import type { MouseEvent } from "react";

export interface DismissOnPressProps {
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
 * Nothing is returned while no modal is open, so the control works as usual.
 */
export function dismissOnPress(
  modalOpen: boolean,
  closeModal: () => void,
): DismissOnPressProps {
  if (!modalOpen) return {};
  return {
    onMouseDownCapture: (e) => {
      swallow(e);
      closeModal();
    },
    onClickCapture: swallow,
  };
}
