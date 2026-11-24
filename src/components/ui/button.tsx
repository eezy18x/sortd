import * as React from "react";
import { Slot } from "radix-ui";
import { cva, type VariantProps } from "class-variance-authority";
import { LoaderCircle } from "lucide-react";
import { cn } from "@/lib/utils";

const buttonVariants = cva(
  "inline-flex shrink-0 items-center justify-center gap-1.5 whitespace-nowrap rounded-md font-medium transition-[background-color,border-color,color,box-shadow] duration-150 disabled:pointer-events-none disabled:opacity-45 [&_svg]:size-4 [&_svg]:shrink-0 cursor-default",
  {
    variants: {
      variant: {
        primary: "bg-accent text-accent-fg hover:brightness-110 active:brightness-95 shadow-sm",
        secondary:
          "bg-surface-2 text-fg border border-border hover:bg-surface-3 hover:border-border-strong",
        ghost: "text-muted hover:text-fg hover:bg-surface-2",
        outline: "border border-border text-fg hover:bg-surface-2",
        danger: "bg-danger text-white hover:brightness-110 active:brightness-95 shadow-sm",
        "danger-ghost": "text-danger hover:bg-danger-soft",
      },
      size: {
        sm: "h-7 px-2.5 text-[12.5px]",
        md: "h-8 px-3 text-[13px]",
        lg: "h-9 px-4 text-sm",
        icon: "size-8",
        "icon-sm": "size-7 [&_svg]:size-3.5",
      },
    },
    defaultVariants: { variant: "secondary", size: "md" },
  },
);

export interface ButtonProps
  extends React.ButtonHTMLAttributes<HTMLButtonElement>, VariantProps<typeof buttonVariants> {
  asChild?: boolean;
  loading?: boolean;
}

export const Button = React.forwardRef<HTMLButtonElement, ButtonProps>(
  ({ className, variant, size, asChild, loading, disabled, children, ...props }, ref) => {
    const Comp = asChild ? Slot.Root : "button";
    return (
      <Comp
        ref={ref}
        className={cn(buttonVariants({ variant, size }), className)}
        disabled={disabled || loading}
        aria-busy={loading || undefined}
        {...props}
      >
        {asChild ? (
          children
        ) : (
          <>
            {loading && <LoaderCircle className="animate-spin" aria-hidden />}
            {children}
          </>
        )}
      </Comp>
    );
  },
);
Button.displayName = "Button";
