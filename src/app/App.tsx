import { createHashRouter, RouterProvider } from "react-router-dom";
import { QueryClient, QueryClientProvider, useQuery } from "@tanstack/react-query";
import { api } from "@/services/api";
import { ToastProvider } from "@/components/toast";
import { TooltipProvider } from "@/components/ui/primitives";
import { FileActionsProvider } from "@/components/files/FileActions";
import { JobsProvider } from "@/hooks/useJobs";
import { useApplyTheme } from "@/hooks/useTheme";
import { AppShell } from "./AppShell";
import { OverviewPage } from "@/features/dashboard/OverviewPage";
import { ExplorerPage } from "@/features/explorer/ExplorerPage";
import { OrganizationPage } from "@/features/organization/OrganizationPage";
import { DuplicatesPage } from "@/features/duplicates/DuplicatesPage";
import { LargeFilesPage } from "@/features/large-files/LargeFilesPage";
import { RecentPage } from "@/features/recent/RecentPage";
import { CategoriesPage } from "@/features/categories/CategoriesPage";
import { HistoryPage } from "@/features/history/HistoryPage";
import { SettingsPage } from "@/features/settings/SettingsPage";
import { SearchPage } from "@/features/search/SearchPage";
import { EmptyState, LoadingState } from "@/components/common";
import { FileQuestion } from "lucide-react";

export const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      staleTime: 15_000,
      retry: (count, error) => count < 1 && (error as { kind?: string }).kind !== "unavailable",
      refetchOnWindowFocus: false,
    },
  },
});

// Hash routing: the app is served from a custom protocol with no server-side routes.
const router = createHashRouter([
  {
    path: "/",
    element: <AppShell />,
    children: [
      { index: true, element: <OverviewPage /> },
      { path: "explorer", element: <ExplorerPage /> },
      { path: "organize", element: <OrganizationPage /> },
      { path: "duplicates", element: <DuplicatesPage /> },
      {
        path: "storage",
        // Charts are the heaviest dependency; load them only when needed.
        lazy: async () => ({
          Component: (await import("@/features/storage/StoragePage")).StoragePage,
        }),
        hydrateFallbackElement: <LoadingState />,
      },
      { path: "large", element: <LargeFilesPage /> },
      { path: "recent", element: <RecentPage /> },
      { path: "categories", element: <CategoriesPage /> },
      { path: "history", element: <HistoryPage /> },
      { path: "settings", element: <SettingsPage /> },
      { path: "search", element: <SearchPage /> },
      { path: "*", element: <EmptyState icon={FileQuestion} title="Page not found" /> },
    ],
  },
]);

function ThemeBridge() {
  const settings = useQuery({ queryKey: ["settings"], queryFn: api.settings });
  useApplyTheme(settings.data?.theme);
  return null;
}

export function App() {
  return (
    <QueryClientProvider client={queryClient}>
      <ToastProvider>
        <TooltipProvider>
          <JobsProvider>
            <FileActionsProvider>
              <ThemeBridge />
              <RouterProvider router={router} />
            </FileActionsProvider>
          </JobsProvider>
        </TooltipProvider>
      </ToastProvider>
    </QueryClientProvider>
  );
}
