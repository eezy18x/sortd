import * as React from "react";

export type ThemePreference = "system" | "dark" | "light";

const STORAGE_KEY = "sortd.theme";

export function resolveTheme(pref: ThemePreference, systemDark: boolean): "dark" | "light" {
  if (pref === "system") return systemDark ? "dark" : "light";
  return pref;
}

/** Applies the theme to <html data-theme> and follows the OS when "system". */
export function useApplyTheme(pref: ThemePreference | undefined) {
  React.useEffect(() => {
    const effective =
      pref ?? (localStorage.getItem(STORAGE_KEY) as ThemePreference | null) ?? "system";
    const mq = window.matchMedia("(prefers-color-scheme: dark)");
    const apply = () => {
      document.documentElement.dataset.theme = resolveTheme(effective, mq.matches);
    };
    apply();
    if (pref) localStorage.setItem(STORAGE_KEY, pref);
    if (effective !== "system") return;
    mq.addEventListener("change", apply);
    return () => mq.removeEventListener("change", apply);
  }, [pref]);
}

export function useDebouncedValue<T>(value: T, ms = 200): T {
  const [v, setV] = React.useState(value);
  React.useEffect(() => {
    const t = window.setTimeout(() => setV(value), ms);
    return () => window.clearTimeout(t);
  }, [value, ms]);
  return v;
}
