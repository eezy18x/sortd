import * as React from "react";
import { CircleAlert, LoaderCircle, type LucideIcon } from "lucide-react";
import { cn } from "@/lib/utils";
import { errorMessage } from "@/services/api";
import { Button } from "./ui/button";

export function Page({ children, className }: { children: React.ReactNode; className?: string }) {
  return (
    <div
      className={cn("mx-auto flex w-full max-w-[1280px] flex-col gap-5 px-7 pt-6 pb-10", className)}
    >
      {children}
    </div>
  );
}

export function PageHeader({
  title,
  description,
  actions,
}: {
  title: string;
  description?: React.ReactNode;
  actions?: React.ReactNode;
}) {
  return (
    <header className="flex flex-wrap items-end justify-between gap-4">
      <div className="min-w-0">
        <h1 className="text-[20px] font-semibold tracking-[-0.01em]">{title}</h1>
        {description && <p className="mt-1 max-w-2xl text-[13px] text-muted">{description}</p>}
      </div>
      {actions && <div className="flex flex-wrap items-center gap-2">{actions}</div>}
    </header>
  );
}

export function EmptyState({
  icon: Icon,
  title,
  description,
  action,
  className,
}: {
  icon: LucideIcon;
  title: string;
  description?: React.ReactNode;
  action?: React.ReactNode;
  className?: string;
}) {
  return (
    <div
      className={cn("flex flex-col items-center justify-center px-6 py-14 text-center", className)}
    >
      <div className="mb-4 flex size-11 items-center justify-center rounded-xl border border-border bg-surface-2 text-muted">
        <Icon className="size-5" strokeWidth={1.75} />
      </div>
      <h3 className="text-[14px] font-semibold">{title}</h3>
      {description && <p className="mt-1.5 max-w-md text-[13px] text-muted">{description}</p>}
      {action && <div className="mt-5 flex gap-2">{action}</div>}
    </div>
  );
}

export function ErrorState({
  error,
  onRetry,
  className,
}: {
  error: unknown;
  onRetry?: () => void;
  className?: string;
}) {
  return (
    <div
      role="alert"
      className={cn("flex flex-col items-center justify-center px-6 py-12 text-center", className)}
    >
      <div className="mb-3 flex size-10 items-center justify-center rounded-xl bg-danger-soft text-danger">
        <CircleAlert className="size-5" />
      </div>
      <h3 className="text-[14px] font-semibold">Couldn’t load this</h3>
      <p className="selectable mt-1 max-w-md text-[13px] text-muted">{errorMessage(error)}</p>
      {onRetry && (
        <Button className="mt-4" size="sm" onClick={onRetry}>
          Try again
        </Button>
      )}
    </div>
  );
}

export function LoadingState({
  label = "Loading…",
  className,
}: {
  label?: string;
  className?: string;
}) {
  return (
    <div
      className={cn(
        "flex items-center justify-center gap-2 py-14 text-[13px] text-muted",
        className,
      )}
      role="status"
    >
      <LoaderCircle className="size-4 animate-spin" />
      {label}
    </div>
  );
}

export function InlineNotice({
  tone = "neutral",
  icon: Icon,
  children,
  className,
}: {
  tone?: "neutral" | "warning" | "danger" | "accent";
  icon?: LucideIcon;
  children: React.ReactNode;
  className?: string;
}) {
  const tones = {
    neutral: "bg-surface-2 text-muted border-border",
    warning: "bg-warning-soft text-warning border-transparent",
    danger: "bg-danger-soft text-danger border-transparent",
    accent: "bg-accent-soft text-accent border-transparent",
  };
  return (
    <div
      className={cn(
        "flex items-start gap-2.5 rounded-lg border px-3 py-2.5 text-[12.5px]",
        tones[tone],
        className,
      )}
    >
      {Icon && <Icon className="mt-px size-4 shrink-0" />}
      <div className="min-w-0 flex-1">{children}</div>
    </div>
  );
}

export function StatTile({
  label,
  value,
  hint,
  icon: Icon,
  onClick,
}: {
  label: string;
  value: React.ReactNode;
  hint?: React.ReactNode;
  icon?: LucideIcon;
  onClick?: () => void;
}) {
  const Comp = onClick ? "button" : "div";
  return (
    <Comp
      onClick={onClick}
      className={cn(
        "flex min-w-0 flex-col items-start rounded-xl border border-border bg-surface px-4 py-3.5 text-left",
        onClick && "cursor-default transition-colors hover:border-border-strong hover:bg-surface-2",
      )}
    >
      <div className="flex w-full items-center justify-between gap-2 text-[12px] text-muted">
        <span>{label}</span>
        {Icon && <Icon className="size-4 text-subtle" strokeWidth={1.75} />}
      </div>
      <div className="tabular mt-1.5 text-[22px] font-semibold tracking-[-0.02em] text-fg">
        {value}
      </div>
      {hint && <div className="mt-0.5 truncate text-[12px] text-subtle">{hint}</div>}
    </Comp>
  );
}

/** A horizontal magnitude bar (single hue; magnitude is the only encoding). */
export function SizeBar({ fraction, className }: { fraction: number; className?: string }) {
  const pct = Math.max(0, Math.min(1, fraction)) * 100;
  return (
    <div className={cn("h-1.5 w-full rounded-full bg-surface-3", className)} aria-hidden>
      <div
        className="h-full rounded-full bg-accent/80"
        style={{ width: `${Math.max(pct, pct > 0 ? 1.5 : 0)}%` }}
      />
    </div>
  );
}
