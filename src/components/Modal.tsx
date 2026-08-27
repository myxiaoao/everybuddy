import { useState, type ReactNode } from "react";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { cn } from "@/lib/utils";
import { ErrorNotice, type ErrorNoticeContent } from "./ErrorNotice";

interface ModalProps {
  open: boolean;
  title: string;
  description?: string;
  children: ReactNode;
  footer?: ReactNode;
  errorNotice?: ErrorNoticeContent;
  size?: "small" | "medium" | "large";
  closeLabel: string;
  onClose: () => void;
}

const widths = {
  small: "max-w-[450px]",
  medium: "max-w-[520px]",
  large: "max-w-[720px]",
};

let lastFocusOutsideDialog: HTMLElement | null = null;

function focusWhenAvailable(target: HTMLElement) {
  const tryFocus = () => {
    if (!target.isConnected) return true;
    if (target.matches(":disabled")) return false;
    const openDialog = document.querySelector(
      '[role="dialog"][data-state="open"]',
    );
    if (openDialog && !openDialog.contains(target)) return true;
    target.focus();
    return true;
  };

  if (tryFocus()) return;

  // The trigger can stay disabled briefly while an operation finishes.
  const observer = new MutationObserver(() => {
    if (!tryFocus()) return;
    observer.disconnect();
  });
  observer.observe(target, {
    attributes: true,
    attributeFilter: ["disabled"],
  });
  window.setTimeout(() => observer.disconnect(), 5_000);
}

export function Modal({
  open,
  title,
  description,
  children,
  footer,
  errorNotice,
  size = "medium",
  closeLabel,
  onClose,
}: ModalProps) {
  const [focusTargets] = useState(() => {
    const active =
      typeof document !== "undefined" &&
      document.activeElement instanceof HTMLElement
        ? document.activeElement
        : null;
    const primary = active === document.body ? null : active;
    const primaryDialog = primary?.closest<HTMLElement>('[role="dialog"]');
    if (primary && !primaryDialog) {
      lastFocusOutsideDialog = primary;
    }
    return { primary, primaryDialog, fallback: lastFocusOutsideDialog };
  });
  const restoreFocus = () => {
    if (!focusTargets.primary && !focusTargets.fallback) return false;

    window.setTimeout(() => {
      window.setTimeout(() => {
        const primaryIsAvailable =
          focusTargets.primary?.isConnected &&
          (!focusTargets.primaryDialog ||
            (focusTargets.primaryDialog.isConnected &&
              focusTargets.primaryDialog.getAttribute("data-state") ===
                "open"));
        const target = primaryIsAvailable
          ? focusTargets.primary
          : focusTargets.fallback?.isConnected
            ? focusTargets.fallback
            : null;
        if (!target) return;
        focusWhenAvailable(target);
      }, 0);
    }, 0);
    return true;
  };

  return (
    <Dialog
      open={open}
      onOpenChange={(nextOpen) => {
        if (!nextOpen) onClose();
      }}
    >
      <DialogContent
        className={cn(widths[size], "modal-shell gap-0")}
        closeLabel={closeLabel}
        onCloseAutoFocus={(event) => {
          if (!restoreFocus()) return;
          event.preventDefault();
        }}
      >
        <DialogHeader>
          <DialogTitle>{title}</DialogTitle>
          {description ? (
            <DialogDescription>{description}</DialogDescription>
          ) : null}
        </DialogHeader>
        <div className="modal__body">{children}</div>
        {errorNotice ? <ErrorNotice {...errorNotice} contained /> : null}
        {footer ? <DialogFooter>{footer}</DialogFooter> : null}
      </DialogContent>
    </Dialog>
  );
}
