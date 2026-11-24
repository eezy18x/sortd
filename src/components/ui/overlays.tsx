// Dialog, confirmation dialog, side sheet and dropdown menu built on Radix
// primitives (focus trapping, Escape handling and ARIA come from Radix).

import * as React from "react";
import {
  AlertDialog as AlertPrimitive,
  Dialog as DialogPrimitive,
  DropdownMenu as MenuPrimitive,
} from "radix-ui";
import { X } from "lucide-react";
import { cn } from "@/lib/utils";
import { Button } from "./button";

export const Dialog = DialogPrimitive.Root;

export function DialogContent({
  title,
  description,
  children,
  footer,
  className,
  onInteractOutside,
}: {
  title: React.ReactNode;
  description?: React.ReactNode;
  children?: React.ReactNode;
  footer?: React.ReactNode;
  className?: string;
  onInteractOutside?: (e: Event) => void;
}) {
  return (
    <DialogPrimitive.Portal>
      <DialogPrimitive.Overlay className="fixed inset-0 z-40 animate-fade-in bg-black/45 backdrop-blur-[2px]" />
      <DialogPrimitive.Content
        onInteractOutside={onInteractOutside}
        className={cn(
          "fixed top-1/2 left-1/2 z-50 flex max-h-[86vh] w-[min(560px,92vw)] -translate-x-1/2 -translate-y-1/2 animate-pop-in flex-col rounded-xl border border-border bg-surface shadow-elevated focus:outline-none",
          className,
        )}
      >
        <div className="flex items-start justify-between gap-4 border-b border-border px-5 py-4">
          <div className="min-w-0">
            <DialogPrimitive.Title className="text-[15px] font-semibold">
              {title}
            </DialogPrimitive.Title>
            {description ? (
              <DialogPrimitive.Description className="mt-1 text-[12.5px] text-muted">
                {description}
              </DialogPrimitive.Description>
            ) : (
              <DialogPrimitive.Description className="sr-only">
                {String(title)}
              </DialogPrimitive.Description>
            )}
          </div>
          <DialogPrimitive.Close asChild>
            <Button variant="ghost" size="icon-sm" aria-label="Close">
              <X />
            </Button>
          </DialogPrimitive.Close>
        </div>
        <div className="min-h-0 flex-1 overflow-auto">{children}</div>
        {footer && (
          <div className="flex items-center justify-end gap-2 border-t border-border px-5 py-3">
            {footer}
          </div>
        )}
      </DialogPrimitive.Content>
    </DialogPrimitive.Portal>
  );
}

export const DialogClose = DialogPrimitive.Close;

/** Confirmation for sensitive actions. The confirm button is never the default focus for destructive actions. */
export function ConfirmDialog({
  open,
  onOpenChange,
  title,
  description,
  confirmLabel,
  destructive,
  loading,
  onConfirm,
  children,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  title: string;
  description: React.ReactNode;
  confirmLabel: string;
  destructive?: boolean;
  loading?: boolean;
  onConfirm: () => void;
  children?: React.ReactNode;
}) {
  return (
    <AlertPrimitive.Root open={open} onOpenChange={onOpenChange}>
      <AlertPrimitive.Portal>
        <AlertPrimitive.Overlay className="fixed inset-0 z-40 animate-fade-in bg-black/45 backdrop-blur-[2px]" />
        <AlertPrimitive.Content className="fixed top-1/2 left-1/2 z-50 w-[min(440px,92vw)] -translate-x-1/2 -translate-y-1/2 animate-pop-in rounded-xl border border-border bg-surface p-5 shadow-elevated">
          <AlertPrimitive.Title className="text-[15px] font-semibold">{title}</AlertPrimitive.Title>
          <AlertPrimitive.Description asChild>
            <div className="mt-2 text-[13px] text-muted">{description}</div>
          </AlertPrimitive.Description>
          {children}
          <div className="mt-5 flex justify-end gap-2">
            <AlertPrimitive.Cancel asChild>
              <Button variant="secondary" autoFocus={destructive}>
                Cancel
              </Button>
            </AlertPrimitive.Cancel>
            <Button
              variant={destructive ? "danger" : "primary"}
              loading={loading}
              onClick={(e) => {
                e.preventDefault();
                onConfirm();
              }}
            >
              {confirmLabel}
            </Button>
          </div>
        </AlertPrimitive.Content>
      </AlertPrimitive.Portal>
    </AlertPrimitive.Root>
  );
}

/** Right-hand side panel for details. */
export function Sheet({
  open,
  onOpenChange,
  title,
  children,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  title: string;
  children: React.ReactNode;
}) {
  return (
    <DialogPrimitive.Root open={open} onOpenChange={onOpenChange}>
      <DialogPrimitive.Portal>
        <DialogPrimitive.Overlay className="fixed inset-0 z-40 animate-fade-in bg-black/30" />
        <DialogPrimitive.Content className="fixed inset-y-0 right-0 z-50 flex w-[min(440px,92vw)] animate-slide-in flex-col border-l border-border bg-surface shadow-elevated focus:outline-none">
          <div className="flex items-center justify-between border-b border-border px-5 py-3">
            <DialogPrimitive.Title className="text-[14px] font-semibold">
              {title}
            </DialogPrimitive.Title>
            <DialogPrimitive.Description className="sr-only">{title}</DialogPrimitive.Description>
            <DialogPrimitive.Close asChild>
              <Button variant="ghost" size="icon-sm" aria-label="Close">
                <X />
              </Button>
            </DialogPrimitive.Close>
          </div>
          <div className="min-h-0 flex-1 overflow-auto">{children}</div>
        </DialogPrimitive.Content>
      </DialogPrimitive.Portal>
    </DialogPrimitive.Root>
  );
}

export const Menu = MenuPrimitive.Root;
export const MenuTrigger = MenuPrimitive.Trigger;

export function MenuContent({
  children,
  align = "end",
}: {
  children: React.ReactNode;
  align?: "start" | "end";
}) {
  return (
    <MenuPrimitive.Portal>
      <MenuPrimitive.Content
        align={align}
        sideOffset={4}
        className="z-50 min-w-48 animate-fade-in rounded-lg border border-border bg-surface p-1 shadow-elevated"
      >
        {children}
      </MenuPrimitive.Content>
    </MenuPrimitive.Portal>
  );
}

export function MenuItem({
  children,
  onSelect,
  danger,
  disabled,
  icon,
}: {
  children: React.ReactNode;
  onSelect: () => void;
  danger?: boolean;
  disabled?: boolean;
  icon?: React.ReactNode;
}) {
  return (
    <MenuPrimitive.Item
      disabled={disabled}
      onSelect={onSelect}
      className={cn(
        "flex h-8 cursor-default items-center gap-2 rounded-md px-2 text-[13px] outline-none data-[disabled]:opacity-40 data-[highlighted]:bg-surface-2 [&_svg]:size-4 [&_svg]:text-muted",
        danger && "text-danger [&_svg]:text-danger data-[highlighted]:bg-danger-soft",
      )}
    >
      {icon}
      {children}
    </MenuPrimitive.Item>
  );
}

export function MenuSeparator() {
  return <MenuPrimitive.Separator className="my-1 h-px bg-border" />;
}
