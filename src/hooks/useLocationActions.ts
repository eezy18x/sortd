import { useMutation, useQueryClient } from "@tanstack/react-query";
import { api, errorMessage } from "@/services/api";
import { useToast } from "@/components/toast";

/** Adding a folder (via the native picker) and starting scans. */
export function useLocationActions() {
  const qc = useQueryClient();
  const toast = useToast();

  const scan = useMutation({
    mutationFn: (ids?: number[]) => api.startScan(ids),
    onError: (e) => toast.error("Couldn’t start the scan", errorMessage(e)),
  });

  const add = useMutation({
    mutationFn: () => api.addLocation("scan"),
    onSuccess: (loc) => {
      if (!loc) return; // Picker cancelled.
      void qc.invalidateQueries();
      toast.success("Folder added", loc.path);
      scan.mutate([loc.id]);
    },
    onError: (e) => toast.error("Couldn’t add the folder", errorMessage(e)),
  });

  return { add, scan };
}
