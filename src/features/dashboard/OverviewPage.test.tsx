import { describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import type { Overview } from "@/types";
import { ToastProvider } from "@/components/toast";
import { TooltipProvider } from "@/components/ui/primitives";

const emptyOverview: Overview = {
  totalFiles: 0,
  totalSize: 0,
  totalDirectories: 0,
  locationCount: 0,
  hashedFiles: 0,
  duplicateGroups: 0,
  duplicateWaste: 0,
  pendingRecommendations: 0,
  incompleteLocations: 0,
  categories: [],
  largestFiles: [],
  recentFiles: [],
  recentScans: [],
};

const api = vi.hoisted(() => ({
  overview: vi.fn(),
  locations: vi.fn(),
  settings: vi.fn(),
}));

vi.mock("@/services/api", () => ({
  api,
  errorMessage: (e: unknown) => String(e),
  isTauri: () => false,
  subscribe: async () => () => {},
  events: {},
}));

import { OverviewPage } from "./OverviewPage";
import { FileActionsProvider } from "@/components/files/FileActions";

function renderPage() {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={qc}>
      <ToastProvider>
        <TooltipProvider>
          <FileActionsProvider>
            <MemoryRouter>
              <OverviewPage />
            </MemoryRouter>
          </FileActionsProvider>
        </TooltipProvider>
      </ToastProvider>
    </QueryClientProvider>,
  );
}

describe("OverviewPage", () => {
  it("onboards with a folder picker when nothing is indexed", async () => {
    api.overview.mockResolvedValue(emptyOverview);
    api.locations.mockResolvedValue([]);
    api.settings.mockResolvedValue(null);
    renderPage();
    expect(await screen.findByText("Choose a folder to get started")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Add a folder/ })).toBeInTheDocument();
  });

  it("shows real indexed statistics", async () => {
    api.overview.mockResolvedValue({
      ...emptyOverview,
      totalFiles: 1234,
      totalSize: 5 * 1024 ** 3,
      totalDirectories: 56,
      locationCount: 1,
      categories: [{ category: "videos", files: 3, size: 4 * 1024 ** 3 }],
    });
    api.locations.mockResolvedValue([]);
    renderPage();
    expect(await screen.findByText("5.0 GB")).toBeInTheDocument();
    expect(screen.getByText((1234).toLocaleString())).toBeInTheDocument();
    expect(screen.getByText("Videos")).toBeInTheDocument();
  });

  it("surfaces backend errors instead of fake data", async () => {
    api.overview.mockRejectedValue(new Error("database is locked"));
    api.locations.mockResolvedValue([]);
    renderPage();
    expect(await screen.findByText(/database is locked/)).toBeInTheDocument();
  });
});
