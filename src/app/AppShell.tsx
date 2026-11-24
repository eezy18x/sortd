import * as React from "react";
import { NavLink, Outlet, useLocation, useNavigate } from "react-router-dom";
import { useQuery } from "@tanstack/react-query";
import {
  Clock,
  Copy,
  FileBox,
  FolderTree,
  HardDrive,
  History,
  LayoutDashboard,
  LoaderCircle,
  Search,
  Settings,
  Shapes,
  Sparkles,
  Square,
  type LucideIcon,
} from "lucide-react";
import { api, errorMessage } from "@/services/api";
import { useJobs } from "@/hooks/useJobs";
import { formatBytes, formatCount } from "@/lib/format";
import { cn } from "@/lib/utils";
import { Button } from "@/components/ui/button";
import { Kbd, Tooltip } from "@/components/ui/primitives";
import { useToast } from "@/components/toast";

interface NavItem {
  to: string;
  label: string;
  icon: LucideIcon;
  badge?: number;
}

function NavGroup({ items }: { items: NavItem[] }) {
  return (
    <div className="flex flex-col gap-0.5">
      {items.map((it) => (
        <NavLink
          key={it.to}
          to={it.to}
          end={it.to === "/"}
          className={({ isActive }) =>
            cn(
              "group flex h-8 items-center gap-2.5 rounded-md px-2.5 text-[13px] text-muted transition-colors hover:bg-surface-2 hover:text-fg",
              isActive && "bg-surface-2 text-fg",
            )
          }
        >
          {({ isActive }) => (
            <>
              <it.icon
                className={cn(
                  "size-4 shrink-0",
                  isActive ? "text-accent" : "text-subtle group-hover:text-muted",
                )}
                strokeWidth={1.75}
              />
              <span className="truncate">{it.label}</span>
              {!!it.badge && (
                <span className="tabular ml-auto rounded-full bg-accent-soft px-1.5 text-[11px] font-medium text-accent">
                  {it.badge > 999 ? "999+" : it.badge}
                </span>
              )}
            </>
          )}
        </NavLink>
      ))}
    </div>
  );
}

function Sidebar() {
  const overview = useQuery({ queryKey: ["overview"], queryFn: api.overview });
  const primary: NavItem[] = [
    { to: "/", label: "Overview", icon: LayoutDashboard },
    { to: "/explorer", label: "File Explorer", icon: FolderTree },
    {
      to: "/organize",
      label: "Smart Organization",
      icon: Sparkles,
      badge: overview.data?.pendingRecommendations,
    },
    { to: "/duplicates", label: "Duplicates", icon: Copy, badge: overview.data?.duplicateGroups },
  ];
  const insight: NavItem[] = [
    { to: "/storage", label: "Storage Analyzer", icon: HardDrive },
    { to: "/large", label: "Large Files", icon: FileBox },
    { to: "/recent", label: "Recent Files", icon: Clock },
    { to: "/categories", label: "Categories", icon: Shapes },
  ];
  const system: NavItem[] = [
    { to: "/history", label: "Activity History", icon: History },
    { to: "/settings", label: "Settings", icon: Settings },
  ];
  return (
    <nav
      aria-label="Main"
      className="flex w-[228px] shrink-0 flex-col border-r border-border bg-surface/60"
    >
      <div className="flex h-[52px] items-center gap-2.5 px-4">
        <img src="/logo.svg" alt="" className="size-6" />
        <span className="text-[14px] font-semibold tracking-[0.08em]">SORTD</span>
      </div>
      <div className="flex flex-1 flex-col gap-5 overflow-auto px-2.5 pt-2 pb-3">
        <NavGroup items={primary} />
        <div>
          <div className="px-2.5 pb-1.5 text-[11px] font-medium tracking-wide text-subtle uppercase">
            Insights
          </div>
          <NavGroup items={insight} />
        </div>
        <div className="mt-auto">
          <NavGroup items={system} />
        </div>
      </div>
      {overview.data && overview.data.totalFiles > 0 && (
        <div className="border-t border-border px-4 py-3 text-[11.5px] text-subtle">
          <span className="tabular">{formatCount(overview.data.totalFiles)}</span> files ·{" "}
          <span className="tabular">{formatBytes(overview.data.totalSize)}</span> indexed
        </div>
      )}
    </nav>
  );
}

function JobIndicator() {
  const jobs = useJobs();
  const toast = useToast();
  if (jobs.scanRunning && jobs.scan) {
    const s = jobs.scan;
    return (
      <div
        className="flex items-center gap-2 rounded-lg border border-border bg-surface-2 py-1 pr-1 pl-2.5 text-[12px]"
        role="status"
      >
        <LoaderCircle className="size-3.5 animate-spin text-accent" />
        <span className="text-muted">
          {s.phase === "finalizing" ? "Finalizing index…" : "Scanning"}
        </span>
        <span className="tabular font-medium">{formatCount(s.filesSeen)} files</span>
        <Tooltip content="Cancel scan">
          <Button
            variant="ghost"
            size="icon-sm"
            aria-label="Cancel scan"
            onClick={() =>
              api.cancelScan().catch((e) => toast.error("Couldn’t cancel", errorMessage(e)))
            }
          >
            <Square className="size-3" />
          </Button>
        </Tooltip>
      </div>
    );
  }
  if (jobs.duplicatesRunning && jobs.duplicates) {
    const d = jobs.duplicates;
    return (
      <div
        className="flex items-center gap-2 rounded-lg border border-border bg-surface-2 py-1 pr-1 pl-2.5 text-[12px]"
        role="status"
      >
        <LoaderCircle className="size-3.5 animate-spin text-violet" />
        <span className="text-muted">Finding duplicates</span>
        <span className="tabular font-medium">
          {d.toHash ? `${formatCount(d.hashed)} / ${formatCount(d.toHash)}` : "…"}
        </span>
        <Tooltip content="Stop">
          <Button
            variant="ghost"
            size="icon-sm"
            aria-label="Stop duplicate detection"
            onClick={() => void api.cancelDuplicateScan()}
          >
            <Square className="size-3" />
          </Button>
        </Tooltip>
      </div>
    );
  }
  return null;
}

function HeaderSearch() {
  const navigate = useNavigate();
  const location = useLocation();
  const ref = React.useRef<HTMLInputElement>(null);
  const [value, setValue] = React.useState("");

  React.useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "k") {
        e.preventDefault();
        ref.current?.focus();
        ref.current?.select();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  // Keep the header box in sync when arriving at /search?q=… from elsewhere.
  const urlQuery =
    location.pathname === "/search" ? (new URLSearchParams(location.search).get("q") ?? "") : null;
  const [syncedQuery, setSyncedQuery] = React.useState(urlQuery);
  if (urlQuery !== syncedQuery) {
    setSyncedQuery(urlQuery);
    if (urlQuery !== null) setValue(urlQuery);
  }

  const isMac = typeof navigator !== "undefined" && /Mac/i.test(navigator.platform);
  return (
    <form
      role="search"
      className="relative w-[min(440px,40vw)]"
      onSubmit={(e) => {
        e.preventDefault();
        navigate(`/search?q=${encodeURIComponent(value.trim())}`);
      }}
    >
      <Search className="pointer-events-none absolute top-1/2 left-2.5 size-4 -translate-y-1/2 text-subtle" />
      <input
        ref={ref}
        value={value}
        onChange={(e) => setValue(e.target.value)}
        placeholder="Search files…"
        aria-label="Search indexed files"
        className="h-8 w-full rounded-lg border border-border bg-surface-2 pr-16 pl-8 text-[13px] placeholder:text-subtle hover:border-border-strong focus-visible:border-accent focus-visible:bg-surface focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent/25"
      />
      <span className="pointer-events-none absolute top-1/2 right-2 flex -translate-y-1/2 gap-0.5">
        <Kbd>{isMac ? "⌘" : "Ctrl"}</Kbd>
        <Kbd>K</Kbd>
      </span>
    </form>
  );
}

export function AppShell() {
  return (
    <div className="flex h-full">
      <Sidebar />
      <div className="flex min-w-0 flex-1 flex-col">
        <header className="flex h-[52px] shrink-0 items-center justify-between gap-4 border-b border-border px-6">
          <HeaderSearch />
          <JobIndicator />
        </header>
        <main className="min-h-0 flex-1 overflow-auto" id="main">
          <Outlet />
        </main>
      </div>
    </div>
  );
}
