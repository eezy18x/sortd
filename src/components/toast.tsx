import * as React from "react";
import { CircleAlert, CircleCheck, Info, X } from "lucide-react";
import { cn } from "@/lib/utils";

type ToastTone = "success" | "error" | "info";

interface ToastItem {
  id: number;
  tone: ToastTone;
  title: string;
  description?: string;
}

interface ToastApi {
  success: (title: string, description?: string) => void;
  error: (title: string, description?: string) => void;
  info: (title: string, description?: string) => void;
}

const ToastContext = React.createContext<ToastApi | null>(null);

export function ToastProvider({ children }: { children: React.ReactNode }) {
  const [items, setItems] = React.useState<ToastItem[]>([]);
  const next = React.useRef(1);

  const dismiss = React.useCallback(
    (id: number) => setItems((xs) => xs.filter((x) => x.id !== id)),
    [],
  );
  const push = React.useCallback(
    (tone: ToastTone, title: string, description?: string) => {
      const id = next.current++;
      setItems((xs) => [...xs.slice(-3), { id, tone, title, description }]);
      window.setTimeout(() => dismiss(id), tone === "error" ? 9000 : 4500);
    },
    [dismiss],
  );
  const api = React.useMemo<ToastApi>(
    () => ({
      success: (t, d) => push("success", t, d),
      error: (t, d) => push("error", t, d),
      info: (t, d) => push("info", t, d),
    }),
    [push],
  );

  return (
    <ToastContext.Provider value={api}>
      {children}
      <div
        className="pointer-events-none fixed right-4 bottom-4 z-[60] flex w-[360px] flex-col gap-2"
        aria-live="polite"
      >
        {items.map((t) => {
          const Icon = t.tone === "success" ? CircleCheck : t.tone === "error" ? CircleAlert : Info;
          return (
            <div
              key={t.id}
              role={t.tone === "error" ? "alert" : "status"}
              className="pointer-events-auto flex animate-pop-in items-start gap-3 rounded-lg border border-border bg-surface px-3.5 py-3 shadow-elevated"
            >
              <Icon
                className={cn(
                  "mt-0.5 size-4 shrink-0",
                  t.tone === "success" && "text-success",
                  t.tone === "error" && "text-danger",
                  t.tone === "info" && "text-accent",
                )}
              />
              <div className="min-w-0 flex-1">
                <div className="text-[13px] font-medium">{t.title}</div>
                {t.description && (
                  <div className="selectable mt-0.5 text-[12.5px] break-words text-muted">
                    {t.description}
                  </div>
                )}
              </div>
              <button
                className="text-subtle hover:text-fg"
                onClick={() => dismiss(t.id)}
                aria-label="Dismiss notification"
              >
                <X className="size-3.5" />
              </button>
            </div>
          );
        })}
      </div>
    </ToastContext.Provider>
  );
}

export function useToast(): ToastApi {
  const ctx = React.useContext(ToastContext);
  if (!ctx) throw new Error("useToast must be used inside ToastProvider");
  return ctx;
}
