import * as React from "react";
import { Slot } from "@radix-ui/react-slot";
import { cva, type VariantProps } from "class-variance-authority";
import { cn } from "@/lib/utils";

const buttonVariants = cva(
  "inline-flex min-h-10 shrink-0 items-center justify-center gap-2 whitespace-nowrap rounded-[6px] text-[length:var(--text-label)] leading-[var(--leading-ui)] font-semibold outline-none focus-visible:ring-2 focus-visible:ring-[var(--color-focus)] focus-visible:ring-offset-2 focus-visible:ring-offset-[var(--color-bg-surface)] disabled:pointer-events-none disabled:opacity-50 motion-safe:transition-[transform,opacity] motion-safe:duration-[150ms] motion-safe:active:scale-[0.96] [&_svg]:pointer-events-none [&_svg]:shrink-0",
  {
    variants: {
      variant: {
        default: "border border-transparent bg-[var(--color-accent-solid)] px-3.5 text-[var(--primary-foreground)] shadow-sm hover:bg-[var(--color-accent-hover)]",
        secondary: "border border-[var(--color-border-strong)] bg-[var(--color-bg-subtle)] px-3.5 text-[var(--color-text-primary)] hover:bg-[var(--color-bg-hover)]",
        ghost: "bg-transparent px-2.5 text-[var(--color-text-secondary)] hover:bg-[var(--color-bg-hover)] hover:text-[var(--color-text-primary)]",
        destructive: "bg-[var(--color-danger-soft)] px-3.5 text-[var(--color-danger-text)] hover:opacity-85",
      },
      size: {
        default: "h-10",
        sm: "h-9 min-h-9 px-2.5",
        icon: "size-10 min-h-10 p-0",
        "icon-sm": "size-8 min-h-8 p-0",
      },
    },
    defaultVariants: { variant: "default", size: "default" },
  },
);

function Button({ className, variant, size, asChild = false, ...props }: React.ComponentProps<"button"> & VariantProps<typeof buttonVariants> & { asChild?: boolean }) {
  const Comp = asChild ? Slot : "button";
  return <Comp data-slot="button" className={cn(buttonVariants({ variant, size, className }))} {...props} />;
}

export { Button };
