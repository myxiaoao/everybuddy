import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";

export interface ErrorNoticeContent {
  title: string;
  message: string;
  recovery: string;
  dismissLabel: string;
  onDismiss: () => void;
}

export function ErrorNotice({
  title,
  message,
  recovery,
  dismissLabel,
  onDismiss,
  contained = false,
}: ErrorNoticeContent & { contained?: boolean }) {
  return (
    <div
      className={cn("error-toast", contained && "error-toast--contained")}
      role="alert"
      aria-atomic="true"
    >
      <strong>{title}</strong>
      <span>{message}</span>
      <small>{recovery}</small>
      <Button variant="ghost" type="button" onClick={onDismiss}>
        {dismissLabel}
      </Button>
    </div>
  );
}
