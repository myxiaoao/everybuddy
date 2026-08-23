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
        "peer size-4 shrink-0 rounded-[3px] border border-[var(--color-border-strong)] bg-[var(--color-bg-surface)] outline-none focus-visible:ring-2 focus-visible:ring-[var(--color-focus)] disabled:cursor-not-allowed disabled:opacity-50 data-[state=checked]:border-[var(--color-accent-solid)] data-[state=checked]:bg-[var(--color-accent-solid)] data-[state=checked]:text-[var(--primary-foreground)] data-[state=indeterminate]:border-[var(--color-accent-solid)] data-[state=indeterminate]:bg-[var(--color-accent-solid)] data-[state=indeterminate]:text-[var(--primary-foreground)]",
        className,
      )}
      {...props}
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
    </CheckboxPrimitive.Root>
  );
}

export { Checkbox };
