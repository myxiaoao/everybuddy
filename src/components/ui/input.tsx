import * as React from "react";
import { cn } from "@/lib/utils";

function Input({ className, type, ...props }: React.ComponentProps<"input">) {
  return (
    <input
      type={type}
      data-slot="input"
      className={cn(
        "h-10 w-full min-w-0 rounded-[6px] border border-[var(--color-border-strong)] bg-[var(--color-bg-subtle)] px-3 py-1 text-[length:var(--text-body)] leading-[var(--leading-ui)] text-[var(--color-text-primary)] outline-none placeholder:text-[var(--color-text-tertiary)] focus-visible:ring-2 focus-visible:ring-[var(--color-focus)] disabled:cursor-not-allowed disabled:opacity-50",
        className,
      )}
      {...props}
    />
  );
}

export { Input };
