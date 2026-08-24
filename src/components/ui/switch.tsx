import * as React from "react";
import * as SwitchPrimitive from "@radix-ui/react-switch";
import { cn } from "@/lib/utils";

function Switch({
  className,
  ...props
}: React.ComponentProps<typeof SwitchPrimitive.Root>) {
  return (
    <SwitchPrimitive.Root
      data-slot="switch"
      className={cn(
        "peer inline-flex h-5 w-[34px] shrink-0 cursor-pointer items-center rounded-full bg-[var(--color-border-strong)] outline-none focus-visible:ring-2 focus-visible:ring-[var(--color-focus)] disabled:cursor-not-allowed disabled:opacity-50 data-[state=checked]:bg-[var(--color-accent-solid)]",
        className,
      )}
      {...props}
    >
      <SwitchPrimitive.Thumb
        data-slot="switch-thumb"
        className="pointer-events-none block size-3.5 translate-x-[3px] rounded-full bg-[var(--color-bg-surface)] shadow-sm motion-safe:transition-transform motion-safe:duration-[180ms] data-[state=checked]:translate-x-[17px]"
      />
    </SwitchPrimitive.Root>
  );
}

export { Switch };
