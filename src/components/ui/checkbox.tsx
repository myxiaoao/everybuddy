import * as React from "react";
import * as CheckboxPrimitive from "@radix-ui/react-checkbox";
import { Check, Minus } from "lucide-react";
import { cn } from "@/lib/utils";

function Checkbox({
  className,
  checked,
  ...props
}: React.ComponentProps<typeof CheckboxPrimitive.Root>) {
  return (
    <CheckboxPrimitive.Root
      data-slot="checkbox"
      checked={checked}
      className={cn(
        "peer group grid size-10 shrink-0 place-items-center rounded-[6px] outline-none focus-visible:ring-2 focus-visible:ring-[var(--color-focus)] disabled:cursor-not-allowed disabled:opacity-50",
        className,
      )}
      {...props}
    >
      <span
        aria-hidden="true"
        className="grid size-4 place-items-center rounded-[3px] border border-[var(--color-control-border)] bg-[var(--color-bg-surface)] group-data-[state=checked]:border-[var(--color-accent-solid)] group-data-[state=checked]:bg-[var(--color-accent-solid)] group-data-[state=checked]:text-[var(--primary-foreground)] group-data-[state=indeterminate]:border-[var(--color-accent-solid)] group-data-[state=indeterminate]:bg-[var(--color-accent-solid)] group-data-[state=indeterminate]:text-[var(--primary-foreground)]"
      >
        <CheckboxPrimitive.Indicator
          data-slot="checkbox-indicator"
          className="grid place-items-center text-current"
        >
          {checked === "indeterminate" ? (
            <Minus className="size-3.5" />
          ) : (
            <Check className="size-3.5" />
          )}
        </CheckboxPrimitive.Indicator>
      </span>
    </CheckboxPrimitive.Root>
  );
}

export { Checkbox };
