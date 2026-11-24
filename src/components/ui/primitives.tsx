// Small presentational primitives: Card, Badge, Input, Select, Skeleton,
// Progress, Kbd, Separator, Checkbox, Switch, Tooltip.

import * as React from "react";
import {
  Checkbox as CheckboxPrimitive,
  Switch as SwitchPrimitive,
  Tooltip as TooltipPrimitive,
} from "radix-ui";
import { Check, Minus } from "lucide-react";
import { cn } from "@/lib/utils";

export function Card({ className, ...props }: React.HTMLAttributes<HTMLDivElement>) {
  return <div className={cn("rounded-xl border border-border bg-surface", className)} {...props} />;
}

export function CardHeader({
  title,
  description,
  actions,
  className,
}: {
  title: React.ReactNode;
  description?: React.ReactNode;
  actions?: React.ReactNode;
  className?: string;
}) {
  return (
    <div className={cn("flex items-start justify-between gap-4 px-5 pt-4 pb-3", className)}>
      <div className="min-w-0">
        <h3 className="text-[13.5px] font-semibold text-fg">{title}</h3>
        {description && <p className="mt-0.5 text-[12.5px] text-muted">{description}</p>}
      </div>
      {actions && <div className="flex shrink-0 items-center gap-1.5">{actions}</div>}
    </div>
  );
}

type Tone = "neutral" | "accent" | "violet" | "success" | "warning" | "danger";

const toneClass: Record<Tone, string> = {
  neutral: "bg-surface-2 text-muted border-border",
  accent: "bg-accent-soft text-accent border-transparent",
  violet: "bg-violet-soft text-violet border-transparent",
  success: "bg-success-soft text-success border-transparent",
  warning: "bg-warning-soft text-warning border-transparent",
  danger: "bg-danger-soft text-danger border-transparent",
};

export function Badge({
  tone = "neutral",
  className,
  ...props
}: React.HTMLAttributes<HTMLSpanElement> & { tone?: Tone }) {
  return (
    <span
      className={cn(
        "inline-flex h-5 items-center gap-1 rounded-md border px-1.5 text-[11.5px] font-medium whitespace-nowrap [&_svg]:size-3",
        toneClass[tone],
        className,
      )}
      {...props}
    />
  );
}

export const Input = React.forwardRef<
  HTMLInputElement,
  React.InputHTMLAttributes<HTMLInputElement>
>(({ className, ...props }, ref) => (
  <input
    ref={ref}
    className={cn(
      "h-8 w-full rounded-md border border-border bg-surface px-2.5 text-[13px] text-fg placeholder:text-subtle transition-colors hover:border-border-strong focus-visible:border-accent focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent/25 disabled:opacity-50",
      className,
    )}
    {...props}
  />
));
Input.displayName = "Input";

export const Select = React.forwardRef<
  HTMLSelectElement,
  React.SelectHTMLAttributes<HTMLSelectElement>
>(({ className, children, ...props }, ref) => (
  <select
    ref={ref}
    className={cn(
      "h-8 rounded-md border border-border bg-surface pl-2.5 pr-7 text-[13px] text-fg transition-colors hover:border-border-strong focus-visible:border-accent focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent/25 disabled:opacity-50",
      className,
    )}
    {...props}
  >
    {children}
  </select>
));
Select.displayName = "Select";

export function Skeleton({ className }: { className?: string }) {
  return <div className={cn("animate-pulse rounded-md bg-surface-2", className)} aria-hidden />;
}

export function Progress({
  value,
  className,
  indeterminate,
  label,
}: {
  value?: number;
  className?: string;
  indeterminate?: boolean;
  label?: string;
}) {
  const pct = Math.max(0, Math.min(1, value ?? 0)) * 100;
  return (
    <div
      role="progressbar"
      aria-label={label}
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={indeterminate ? undefined : Math.round(pct)}
      className={cn("relative h-1.5 w-full overflow-hidden rounded-full bg-surface-3", className)}
    >
      {indeterminate ? (
        <div className="absolute inset-y-0 w-1/3 animate-[indeterminate_1.2s_ease-in-out_infinite] rounded-full bg-accent" />
      ) : (
        <div
          className="h-full rounded-full bg-accent transition-[width] duration-300"
          style={{ width: `${pct}%` }}
        />
      )}
    </div>
  );
}

export function Kbd({ children }: { children: React.ReactNode }) {
  return (
    <kbd className="inline-flex h-5 min-w-5 items-center justify-center rounded border border-border bg-surface-2 px-1 font-sans text-[11px] text-muted">
      {children}
    </kbd>
  );
}

export function Separator({ className }: { className?: string }) {
  return <div role="separator" className={cn("h-px w-full bg-border", className)} />;
}

export function Checkbox({
  checked,
  onCheckedChange,
  className,
  ...props
}: {
  checked: boolean | "indeterminate";
  onCheckedChange: (checked: boolean) => void;
  className?: string;
  "aria-label"?: string;
  disabled?: boolean;
}) {
  return (
    <CheckboxPrimitive.Root
      checked={checked}
      onCheckedChange={(v) => onCheckedChange(v === true)}
      className={cn(
        "flex size-4 shrink-0 items-center justify-center rounded-[4px] border border-border-strong bg-surface transition-colors data-[state=checked]:border-accent data-[state=checked]:bg-accent data-[state=indeterminate]:border-accent data-[state=indeterminate]:bg-accent disabled:opacity-40",
        className,
      )}
      {...props}
    >
      <CheckboxPrimitive.Indicator className="text-accent-fg">
        {checked === "indeterminate" ? (
          <Minus className="size-3" strokeWidth={3} />
        ) : (
          <Check className="size-3" strokeWidth={3} />
        )}
      </CheckboxPrimitive.Indicator>
    </CheckboxPrimitive.Root>
  );
}

export function Switch({
  checked,
  onCheckedChange,
  disabled,
  id,
  ...props
}: {
  checked: boolean;
  onCheckedChange: (v: boolean) => void;
  disabled?: boolean;
  id?: string;
  "aria-label"?: string;
}) {
  return (
    <SwitchPrimitive.Root
      id={id}
      checked={checked}
      onCheckedChange={onCheckedChange}
      disabled={disabled}
      className="relative inline-flex h-5 w-9 shrink-0 items-center rounded-full border border-transparent bg-surface-3 transition-colors data-[state=checked]:bg-accent disabled:opacity-40"
      {...props}
    >
      <SwitchPrimitive.Thumb className="block size-4 translate-x-0.5 rounded-full bg-white shadow transition-transform data-[state=checked]:translate-x-[18px]" />
    </SwitchPrimitive.Root>
  );
}

export const TooltipProvider = TooltipPrimitive.Provider;

export function Tooltip({
  content,
  children,
  side = "top",
}: {
  content: React.ReactNode;
  children: React.ReactNode;
  side?: "top" | "bottom" | "left" | "right";
}) {
  if (!content) return <>{children}</>;
  return (
    <TooltipPrimitive.Root delayDuration={350}>
      <TooltipPrimitive.Trigger asChild>{children}</TooltipPrimitive.Trigger>
      <TooltipPrimitive.Portal>
        <TooltipPrimitive.Content
          side={side}
          sideOffset={6}
          className="z-50 max-w-sm animate-fade-in rounded-md border border-border bg-surface-3 px-2 py-1 text-[12px] text-fg shadow-elevated"
        >
          {content}
        </TooltipPrimitive.Content>
      </TooltipPrimitive.Portal>
    </TooltipPrimitive.Root>
  );
}
