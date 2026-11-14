import { useEffect } from "react";
import { useModalStore } from "@/shared/store/modalStore";
import styles from "./modal.module.css";

export const ModalHost = () => {
  const { isOpen, content, closeModal } = useModalStore();

  useEffect(() => {
    if (!isOpen) return;

    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        closeModal();
      }
    };

    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [isOpen, closeModal]);

  if (!isOpen) return null;

  return (
    <>
      {/* The backdrop starts below the header, so the header stays usable: it can still drag the window and its
          buttons work. This layer only dims it and lets every click through. */}
      <div className={styles.headerDim} aria-hidden />
      <div className={styles.backdrop} onMouseDown={closeModal}>
        <div
          className={styles.modalContainer}
          onMouseDown={(e) => e.stopPropagation()}
          onClick={(e) => e.stopPropagation()}
        >
          {content}
        </div>
      </div>
    </>
  );
};
