import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import type { Batch } from "@/types";
import { ToastProvider } from "@/components/toast";
import { TooltipProvider } from "@/components/ui/primitives";

const planned: Batch = {
  summary: {
    id: 7,
    label: "Move “a.txt”",
    origin: "manual",
    status: "planned",
    createdAt: 0,
    executedAt: null,
    finishedAt: null,
    conflictPolicy: "skip",
    allowCrossVolume: false,
    total: 2,
    succeeded: 0,
    failed: 0,
    skipped: 1,
    canUndo: false,
  },
  items: [
    {
      id: 1,
      seq: 0,
      op: "move",
      source: "/r/a.txt",
      destination: "/r/Docs/a.txt",
      status: "pending",
      issue: null,
      error: null,
      crossVolume: false,
      isDirectory: false,
      size: 10,
      verified: false,
    },
    {
      id: 2,
      seq: 1,
      op: "move",
      source: "/r/b.txt",
      destination: null,
      status: "skipped",
      issue: "/r/Docs/b.txt already exists",
      error: null,
      crossVolume: false,
      isDirectory: false,
      size: 5,
      verified: false,
    },
  ],
  pending: 1,
  conflicts: 1,
  crossVolumeItems: 0,
  totalBytes: 10,
};

const executed: Batch = {
  ...planned,
  summary: { ...planned.summary, status: "completed", succeeded: 1, canUndo: true },
  items: [{ ...planned.items[0]!, status: "done", verified: true }, planned.items[1]!],
  pending: 0,
};

const api = vi.hoisted(() => ({
  planOperations: vi.fn(),
  executeOperations: vi.fn(),
  discardPlan: vi.fn(),
  dryRun: vi.fn(),
  undoBatch: vi.fn(),
}));

vi.mock("@/services/api", () => ({
  api,
  errorMessage: (e: unknown) => String(e),
  isTauri: () => false,
  subscribe: async () => () => {},
  events: {},
}));

import { OperationPreview } from "./OperationPreview";

function renderPreview(onClose = vi.fn()) {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={qc}>
      <ToastProvider>
        <TooltipProvider>
          <OperationPreview
            request={{
              label: "Move “a.txt”",
              origin: "manual",
              operations: [{ op: "move", source: "/r/a.txt", destinationDir: "/r/Docs" }],
            }}
            onClose={onClose}
          />
        </TooltipProvider>
      </ToastProvider>
    </QueryClientProvider>,
  );
  return { onClose };
}

describe("OperationPreview", () => {
  beforeEach(() => {
    api.planOperations.mockResolvedValue(planned);
    api.executeOperations.mockResolvedValue(executed);
    api.discardPlan.mockResolvedValue(undefined);
    api.dryRun.mockResolvedValue([{ itemId: 1, ok: true, message: null }]);
  });
  afterEach(() => vi.clearAllMocks());

  it("plans once and never executes without explicit confirmation", async () => {
    renderPreview();
    expect(await screen.findByText("a.txt")).toBeInTheDocument();
    expect(screen.getByText(/already exists/)).toBeInTheDocument();
    expect(api.planOperations).toHaveBeenCalledTimes(1);
    expect(api.executeOperations).not.toHaveBeenCalled();
  });

  it("dry run checks without executing", async () => {
    renderPreview();
    await screen.findByText("a.txt");
    await userEvent.click(screen.getByRole("button", { name: "Dry run" }));
    await waitFor(() => expect(api.dryRun).toHaveBeenCalledWith(7));
    expect(api.executeOperations).not.toHaveBeenCalled();
    expect(await screen.findByText("Ready")).toBeInTheDocument();
  });

  it("executes the planned batch on confirm and offers undo", async () => {
    renderPreview();
    await screen.findByText("a.txt");
    await userEvent.click(screen.getByRole("button", { name: "Run 1 operation" }));
    await waitFor(() => expect(api.executeOperations).toHaveBeenCalledWith(7));
    expect(await screen.findByText(/Done · verified/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Undo/ })).toBeInTheDocument();
  });

  it("discards the plan when cancelled", async () => {
    const { onClose } = renderPreview();
    await screen.findByText("a.txt");
    await userEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(api.discardPlan).toHaveBeenCalledWith(7);
    expect(onClose).toHaveBeenCalled();
    expect(api.executeOperations).not.toHaveBeenCalled();
  });

  it("re-plans with the rename policy when chosen", async () => {
    renderPreview();
    await screen.findByText("a.txt");
    await userEvent.selectOptions(screen.getByRole("combobox"), "rename");
    await waitFor(() => expect(api.planOperations).toHaveBeenCalledTimes(2));
    expect(api.planOperations.mock.calls[1]![0]).toMatchObject({ conflictPolicy: "rename" });
    expect(api.discardPlan).toHaveBeenCalledWith(7);
  });
});
