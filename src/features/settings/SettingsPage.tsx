import * as React from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  CircleAlert,
  CircleCheck,
  Database,
  FolderPlus,
  HardDrive,
  Monitor,
  Moon,
  Plus,
  RefreshCw,
  ShieldCheck,
  Sun,
  Trash2,
  X,
} from "lucide-react";
import { api, errorMessage } from "@/services/api";
import type { Category, IntegrityReport, Location, Settings } from "@/types";
import { CATEGORIES } from "@/lib/categories";
import { formatBytes, formatCount, formatRelative } from "@/lib/format";
import { cn } from "@/lib/utils";
import { Button } from "@/components/ui/button";
import { ConfirmDialog } from "@/components/ui/overlays";
import { Badge, Card, Input, Select, Switch } from "@/components/ui/primitives";
import { ErrorState, InlineNotice, LoadingState, Page, PageHeader } from "@/components/common";
import { useToast } from "@/components/toast";
import { useJobs } from "@/hooks/useJobs";
import { useLocationActions } from "@/hooks/useLocationActions";

function Section({
  id,
  title,
  description,
  children,
}: {
  id: string;
  title: string;
  description?: string;
  children: React.ReactNode;
}) {
  return (
    <section id={id} className="scroll-mt-6">
      <Card>
        <div className="border-b border-border px-5 py-3.5">
          <h2 className="text-[14px] font-semibold">{title}</h2>
          {description && <p className="mt-0.5 text-[12.5px] text-muted">{description}</p>}
        </div>
        <div className="divide-y divide-border">{children}</div>
      </Card>
    </section>
  );
}

function Field({
  label,
  hint,
  children,
  htmlFor,
}: {
  label: string;
  hint?: React.ReactNode;
  children: React.ReactNode;
  htmlFor?: string;
}) {
  return (
    <div className="flex items-center justify-between gap-6 px-5 py-3">
      <div className="min-w-0">
        <label htmlFor={htmlFor} className="text-[13px]">
          {label}
        </label>
        {hint && <div className="mt-0.5 text-[12px] text-muted">{hint}</div>}
      </div>
      <div className="shrink-0">{children}</div>
    </div>
  );
}

function ListEditor({
  items,
  placeholder,
  onChange,
  validate,
  mono,
}: {
  items: string[];
  placeholder: string;
  onChange: (items: string[]) => void;
  validate?: (v: string) => string | null;
  mono?: boolean;
}) {
  const [value, setValue] = React.useState("");
  const error = value.trim() ? (validate?.(value.trim()) ?? null) : null;
  const add = () => {
    const v = value.trim();
    if (!v || error || items.includes(v)) return;
    onChange([...items, v]);
    setValue("");
  };
  return (
    <div className="px-5 py-3">
      {items.length > 0 && (
        <ul className="mb-2 flex flex-wrap gap-1.5">
          {items.map((it) => (
            <li
              key={it}
              className={cn(
                "flex items-center gap-1 rounded-md border border-border bg-surface-2 py-0.5 pr-0.5 pl-2 text-[12.5px]",
                mono && "font-mono text-[12px]",
              )}
            >
              <span className="selectable max-w-[420px] truncate" title={it}>
                {it}
              </span>
              <button
                className="rounded p-0.5 text-subtle hover:bg-surface-3 hover:text-fg"
                aria-label={`Remove ${it}`}
                onClick={() => onChange(items.filter((x) => x !== it))}
              >
                <X className="size-3.5" />
              </button>
            </li>
          ))}
        </ul>
      )}
      <form
        className="flex gap-2"
        onSubmit={(e) => {
          e.preventDefault();
          add();
        }}
      >
        <Input
          value={value}
          onChange={(e) => setValue(e.target.value)}
          placeholder={placeholder}
          aria-invalid={!!error}
          className={mono ? "font-mono text-[12px]" : undefined}
        />
        <Button type="submit" disabled={!value.trim() || !!error}>
          <Plus /> Add
        </Button>
      </form>
      {error && <p className="mt-1.5 text-[12px] text-danger">{error}</p>}
    </div>
  );
}

function LocationsSection() {
  const qc = useQueryClient();
  const toast = useToast();
  const jobs = useJobs();
  const { add, scan } = useLocationActions();
  const locations = useQuery({ queryKey: ["locations"], queryFn: api.locations });
  const [removing, setRemoving] = React.useState<Location | null>(null);
  const remove = useMutation({
    mutationFn: (id: number) => api.removeLocation(id),
    onSuccess: () => {
      setRemoving(null);
      void qc.invalidateQueries();
      toast.success("Location removed", "Its index data was deleted. No files were changed.");
    },
    onError: (e) => toast.error("Couldn’t remove the location", errorMessage(e)),
  });
  return (
    <Section
      id="locations"
      title="Locations"
      description="Folders SORTD may read and organize. Access is granted only through the system folder picker."
    >
      {locations.data?.map((l) => (
        <div key={l.id} className="flex items-center gap-3 px-5 py-3">
          <HardDrive className="size-4 shrink-0 text-muted" strokeWidth={1.75} />
          <div className="min-w-0 flex-1">
            <div className="selectable truncate text-[13px]" title={l.path}>
              {l.path}
            </div>
            <div className="text-[12px] text-muted">
              {l.kind === "destination"
                ? "Destination only (not indexed)"
                : l.lastScanAt
                  ? `${formatCount(l.fileCount)} files · ${formatBytes(l.totalSize)} · scanned ${formatRelative(l.lastScanAt)}`
                  : "Not scanned yet"}
            </div>
          </div>
          {!l.exists && <Badge tone="danger">Missing</Badge>}
          {l.kind === "destination" && <Badge tone="violet">Destination</Badge>}
          {l.kind === "scan" && (
            <Button
              size="sm"
              variant="ghost"
              disabled={jobs.scanRunning || !l.exists}
              onClick={() => scan.mutate([l.id])}
            >
              <RefreshCw /> Rescan
            </Button>
          )}
          <Button
            size="sm"
            variant="danger-ghost"
            onClick={() => setRemoving(l)}
            disabled={jobs.scanRunning}
          >
            Remove
          </Button>
        </div>
      ))}
      <div className="flex gap-2 px-5 py-3">
        <Button variant="primary" onClick={() => add.mutate()} loading={add.isPending}>
          <FolderPlus /> Add folder to index
        </Button>
        <Button
          onClick={() =>
            api
              .addLocation("destination")
              .then((l) => {
                if (l) toast.success("Destination added", l.path);
                return qc.invalidateQueries();
              })
              .catch((e) => toast.error("Couldn’t add the destination", errorMessage(e)))
          }
        >
          Add destination only
        </Button>
      </div>
      <ConfirmDialog
        open={!!removing}
        onOpenChange={(o) => !o && setRemoving(null)}
        title="Remove this location?"
        description={
          <>
            SORTD will forget{" "}
            <span className="selectable font-medium text-fg">{removing?.path}</span> and delete its
            index data. Your files are not touched.
          </>
        }
        confirmLabel="Remove location"
        destructive
        loading={remove.isPending}
        onConfirm={() => removing && remove.mutate(removing.id)}
      />
    </Section>
  );
}

function DatabaseSection() {
  const qc = useQueryClient();
  const toast = useToast();
  const info = useQuery({ queryKey: ["app-info"], queryFn: api.appInfo });
  const [report, setReport] = React.useState<IntegrityReport | null>(null);
  const [confirmReset, setConfirmReset] = React.useState(false);
  const check = useMutation({
    mutationFn: api.checkDatabase,
    onSuccess: setReport,
    onError: (e) => toast.error("Check failed", errorMessage(e)),
  });
  const optimize = useMutation({
    mutationFn: api.optimizeDatabase,
    onSuccess: () => {
      toast.success("Database optimized");
      void qc.invalidateQueries({ queryKey: ["app-info"] });
    },
    onError: (e) => toast.error("Optimization failed", errorMessage(e)),
  });
  const reset = useMutation({
    mutationFn: api.resetIndex,
    onSuccess: () => {
      setConfirmReset(false);
      void qc.invalidateQueries();
      toast.success("Index reset", "Rescan your locations to rebuild it.");
    },
    onError: (e) => toast.error("Reset failed", errorMessage(e)),
  });
  return (
    <Section
      id="database"
      title="Database"
      description="SORTD’s index is a local SQLite database. It describes your files; it never contains their contents."
    >
      <Field
        label="Location"
        hint={
          <span className="selectable font-mono text-[11.5px]">
            {info.data?.databasePath ?? "…"}
          </span>
        }
      >
        <span className="tabular text-[12.5px] text-muted">
          {info.data ? formatBytes(info.data.databaseSize) : ""}
        </span>
      </Field>
      <div className="flex flex-wrap gap-2 px-5 py-3">
        <Button onClick={() => check.mutate()} loading={check.isPending}>
          <ShieldCheck /> Check integrity
        </Button>
        <Button onClick={() => optimize.mutate()} loading={optimize.isPending}>
          <Database /> Optimize
        </Button>
        <Button variant="danger-ghost" className="ml-auto" onClick={() => setConfirmReset(true)}>
          <Trash2 /> Reset index…
        </Button>
      </div>
      {report && (
        <div className="px-5 py-3">
          {report.ok ? (
            <InlineNotice tone="accent" icon={CircleCheck}>
              No problems found.
            </InlineNotice>
          ) : (
            <InlineNotice tone="danger" icon={CircleAlert}>
              <div className="font-medium">{report.guidance}</div>
              <ul className="mt-1 list-disc pl-4">
                {report.problems.slice(0, 10).map((p) => (
                  <li key={p}>{p}</li>
                ))}
              </ul>
            </InlineNotice>
          )}
        </div>
      )}
      <ConfirmDialog
        open={confirmReset}
        onOpenChange={setConfirmReset}
        title="Reset the index?"
        description="Indexed files, folders, scan history, duplicate hashes, suggestions and search history are cleared. Locations, settings and the operation history are kept. No files on disk are changed."
        confirmLabel="Reset index"
        destructive
        loading={reset.isPending}
        onConfirm={() => reset.mutate()}
      />
    </Section>
  );
}

export function SettingsPage() {
  const qc = useQueryClient();
  const toast = useToast();
  const settings = useQuery({ queryKey: ["settings"], queryFn: api.settings });
  const info = useQuery({ queryKey: ["app-info"], queryFn: api.appInfo });
  const save = useMutation({
    mutationFn: (s: Settings) => api.updateSettings(s),
    onMutate: (s) => {
      const prev = qc.getQueryData<Settings>(["settings"]);
      qc.setQueryData(["settings"], s);
      return { prev };
    },
    onError: (e, _s, ctx) => {
      if (ctx?.prev) qc.setQueryData(["settings"], ctx.prev);
      toast.error("Couldn’t save the setting", errorMessage(e));
    },
    onSuccess: (s) => qc.setQueryData(["settings"], s),
  });

  if (settings.isLoading) return <LoadingState />;
  if (settings.isError || !settings.data)
    return <ErrorState error={settings.error} onRetry={() => settings.refetch()} />;
  const s = settings.data;
  const patch = (p: Partial<Settings>) => save.mutate({ ...s, ...p });

  const nav = [
    ["appearance", "Appearance"],
    ["locations", "Locations"],
    ["scanning", "Scanning"],
    ["duplicates", "Duplicates"],
    ["categorization", "Categorization"],
    ["safety", "Safety"],
    ["privacy", "Privacy"],
    ["database", "Database"],
    ["about", "About"],
  ];

  return (
    <Page>
      <PageHeader title="Settings" description="Changes are saved automatically." />
      <div className="grid gap-6 lg:grid-cols-[170px_1fr]">
        <nav
          className="sticky top-0 hidden h-fit flex-col gap-0.5 lg:flex"
          aria-label="Settings sections"
        >
          {nav.map(([id, label]) => (
            <a
              key={id}
              href={`#${id}`}
              className="rounded-md px-2.5 py-1.5 text-[13px] text-muted hover:bg-surface-2 hover:text-fg"
            >
              {label}
            </a>
          ))}
        </nav>
        <div className="flex min-w-0 flex-col gap-5">
          <Section id="appearance" title="Appearance">
            <Field label="Theme" hint="System follows your operating system’s light or dark mode.">
              <div
                className="flex rounded-lg border border-border bg-surface-2 p-0.5"
                role="radiogroup"
                aria-label="Theme"
              >
                {(
                  [
                    ["system", "System", Monitor],
                    ["dark", "Dark", Moon],
                    ["light", "Light", Sun],
                  ] as const
                ).map(([v, label, Icon]) => (
                  <button
                    key={v}
                    role="radio"
                    aria-checked={s.theme === v}
                    onClick={() => patch({ theme: v })}
                    className={cn(
                      "flex h-7 items-center gap-1.5 rounded-md px-2.5 text-[12.5px] text-muted",
                      s.theme === v && "bg-surface text-fg shadow-sm",
                    )}
                  >
                    <Icon className="size-3.5" /> {label}
                  </button>
                ))}
              </div>
            </Field>
          </Section>

          <LocationsSection />

          <Section
            id="scanning"
            title="Scanning"
            description="Scans only read metadata. Symbolic links are recorded but never followed."
          >
            <Field
              label="Skip system locations"
              hint="Operating-system folders (e.g. Windows, Program Files, AppData, /System, /proc) unless you picked them directly."
              htmlFor="defex"
            >
              <Switch
                id="defex"
                checked={s.useDefaultExclusions}
                onCheckedChange={(v) => patch({ useDefaultExclusions: v })}
              />
            </Field>
            <Field label="Include hidden files" htmlFor="hidden">
              <Switch
                id="hidden"
                checked={s.includeHidden}
                onCheckedChange={(v) => patch({ includeHidden: v })}
              />
            </Field>
            <Field
              label="Cross into other drives and mounts"
              hint="Off: a scan stays on the filesystem of the folder you picked."
              htmlFor="xfs"
            >
              <Switch
                id="xfs"
                checked={s.crossFilesystems}
                onCheckedChange={(v) => patch({ crossFilesystems: v })}
              />
            </Field>
            <Field label="Rescan all locations when SORTD starts" htmlFor="startup">
              <Switch
                id="startup"
                checked={s.scanOnStartup}
                onCheckedChange={(v) => patch({ scanOnStartup: v })}
              />
            </Field>
            <div className="px-5 pt-3 text-[13px]">Excluded folders</div>
            <ListEditor
              items={s.excludedPaths}
              mono
              placeholder={
                info.data?.platform === "windows"
                  ? "C:\\Users\\you\\Videos\\Raw"
                  : "/home/you/VirtualMachines"
              }
              onChange={(excludedPaths) => patch({ excludedPaths })}
              validate={(v) =>
                /^([a-zA-Z]:[\\/]|\\\\|\/)/.test(v) ? null : "Enter an absolute path"
              }
            />
            <div className="px-5 pt-3 text-[13px]">Excluded folder names (any depth)</div>
            <ListEditor
              items={s.excludedNames}
              placeholder="node_modules"
              onChange={(excludedNames) => patch({ excludedNames })}
              validate={(v) => (/[\\/]/.test(v) ? "A name, not a path" : null)}
            />
          </Section>

          <Section
            id="duplicates"
            title="Duplicates"
            description="Hashing reads file contents locally to compare them. Cloud placeholders are never downloaded."
          >
            <Field label="Find duplicates after each scan" htmlFor="dupauto">
              <Switch
                id="dupauto"
                checked={s.detectDuplicatesAfterScan}
                onCheckedChange={(v) => patch({ detectDuplicatesAfterScan: v })}
              />
            </Field>
            <Field
              label="Ignore files smaller than"
              hint={`Currently ${formatBytes(s.duplicateMinSize)}`}
            >
              <Select
                value={s.duplicateMinSize}
                onChange={(e) => patch({ duplicateMinSize: Number(e.target.value) })}
                aria-label="Minimum duplicate size"
              >
                {[1, 1024, 64 * 1024, 1024 * 1024, 10 * 1024 * 1024].map((v) => (
                  <option key={v} value={v}>
                    {v === 1 ? "Any size" : formatBytes(v, 0)}
                  </option>
                ))}
              </Select>
            </Field>
            <Field
              label="Hashing threads"
              hint="More threads finish faster on SSDs; fewer are gentler on hard drives."
            >
              <Select
                value={s.hashConcurrency}
                onChange={(e) => patch({ hashConcurrency: Number(e.target.value) })}
                aria-label="Hashing threads"
              >
                {[1, 2, 3, 4, 6, 8, 12, 16].map((n) => (
                  <option key={n} value={n}>
                    {n}
                  </option>
                ))}
              </Select>
            </Field>
          </Section>

          <Section
            id="categorization"
            title="Categorization"
            description="Extensions are treated as hints. Your rules take priority over built-in ones."
          >
            <Field
              label="Verify file types from content"
              hint="Reads the first 8 KB of a file when you open its details to confirm its type. Files are never executed."
              htmlFor="verify"
            >
              <Switch
                id="verify"
                checked={s.verifyContentType}
                onCheckedChange={(v) => patch({ verifyContentType: v })}
              />
            </Field>
            <RulesEditor settings={s} onChange={(categoryRules) => patch({ categoryRules })} />
          </Section>

          <Section id="safety" title="Safety">
            <Field
              label="Preview single-file operations"
              hint="Bulk operations are always previewed. Moving to trash always asks first."
              htmlFor="confirm1"
            >
              <Switch
                id="confirm1"
                checked={s.confirmSingleOperations}
                onCheckedChange={(v) => patch({ confirmSingleOperations: v })}
              />
            </Field>
            <Field
              label="When a name is already taken"
              hint="Default for new previews; existing files are never overwritten either way."
            >
              <Select
                value={s.defaultConflictPolicy}
                onChange={(e) =>
                  patch({
                    defaultConflictPolicy: e.target.value as Settings["defaultConflictPolicy"],
                  })
                }
                aria-label="Conflict policy"
              >
                <option value="skip">Skip the item</option>
                <option value="rename">Keep both (rename)</option>
              </Select>
            </Field>
            <div className="px-5 py-3 text-[12.5px] text-muted">
              SORTD never deletes files permanently; “delete” always means the system trash.
              Executables and scripts are never opened.
            </div>
          </Section>

          <Section id="privacy" title="Privacy">
            <Field
              label="Remember search history"
              hint="Stored only in the local index. Turning this off erases it."
              htmlFor="hist"
            >
              <Switch
                id="hist"
                checked={s.recordSearchHistory}
                onCheckedChange={(v) => patch({ recordSearchHistory: v })}
              />
            </Field>
            <div className="px-5 py-3">
              <ul className="space-y-1.5 text-[12.5px] text-muted">
                {[
                  "No account, no telemetry, no analytics.",
                  "No network requests: file names, paths and contents never leave this computer.",
                  "The webview can only call SORTD’s own commands; it has no direct filesystem or shell access.",
                  "Activity logs record actions and paths, never file contents.",
                ].map((t) => (
                  <li key={t} className="flex gap-2">
                    <ShieldCheck className="mt-0.5 size-3.5 shrink-0 text-accent" /> {t}
                  </li>
                ))}
              </ul>
            </div>
          </Section>

          <DatabaseSection />

          <Section id="about" title="About">
            <Field label="Version">
              <span className="tabular text-[12.5px] text-muted">
                {info.data ? `SORTD ${info.data.version}` : "…"}
              </span>
            </Field>
            <Field label="Platform">
              <span className="text-[12.5px] text-muted">
                {info.data
                  ? `${info.data.platform} · ${info.data.arch} · schema v${info.data.schemaVersion}`
                  : "…"}
              </span>
            </Field>
            <Field label="License">
              <span className="text-[12.5px] text-muted">MIT · © muffle</span>
            </Field>
            <Field label="Source code">
              <span className="selectable font-mono text-[12px] text-muted">
                github.com/eezy18x/sortd
              </span>
            </Field>
          </Section>
        </div>
      </div>
    </Page>
  );
}

function RulesEditor({
  settings,
  onChange,
}: {
  settings: Settings;
  onChange: (rules: Settings["categoryRules"]) => void;
}) {
  const [ext, setExt] = React.useState("");
  const [cat, setCat] = React.useState<Category>("documents");
  const clean = ext.trim().replace(/^\./, "").toLowerCase();
  const invalid = clean !== "" && !/^[\p{L}\p{N}_-]{1,32}$/u.test(clean);
  const exists = settings.categoryRules.some((r) => r.extension === clean);
  return (
    <div className="px-5 py-3">
      <div className="mb-2 text-[13px]">Custom rules</div>
      {settings.categoryRules.length > 0 && (
        <ul className="mb-2 flex flex-wrap gap-1.5">
          {settings.categoryRules.map((r) => (
            <li
              key={r.extension}
              className="flex items-center gap-1.5 rounded-md border border-border bg-surface-2 py-0.5 pr-0.5 pl-2 text-[12.5px]"
            >
              <code className="font-mono text-[12px]">.{r.extension}</code> →{" "}
              {CATEGORIES.find((c) => c.id === r.category)?.label}
              <button
                className="rounded p-0.5 text-subtle hover:bg-surface-3 hover:text-fg"
                aria-label={`Remove rule for .${r.extension}`}
                onClick={() =>
                  onChange(settings.categoryRules.filter((x) => x.extension !== r.extension))
                }
              >
                <X className="size-3.5" />
              </button>
            </li>
          ))}
        </ul>
      )}
      <form
        className="flex gap-2"
        onSubmit={(e) => {
          e.preventDefault();
          if (!clean || invalid || exists) return;
          onChange([...settings.categoryRules, { extension: clean, category: cat }]);
          setExt("");
        }}
      >
        <Input
          value={ext}
          onChange={(e) => setExt(e.target.value)}
          placeholder="Extension, e.g. .log"
          aria-label="Extension"
          aria-invalid={invalid}
          className="w-48"
        />
        <Select
          value={cat}
          onChange={(e) => setCat(e.target.value as Category)}
          aria-label="Category"
        >
          {CATEGORIES.map((c) => (
            <option key={c.id} value={c.id}>
              {c.label}
            </option>
          ))}
        </Select>
        <Button type="submit" disabled={!clean || invalid || exists}>
          <Plus /> Add rule
        </Button>
      </form>
      {(invalid || exists) && (
        <p className="mt-1.5 text-[12px] text-danger">
          {exists
            ? "A rule for this extension already exists."
            : "Use letters, digits, - or _ only."}
        </p>
      )}
      <p className="mt-2 text-[12px] text-subtle">Rules apply on the next scan.</p>
    </div>
  );
}
