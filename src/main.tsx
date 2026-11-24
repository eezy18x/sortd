import React from "react";
import ReactDOM from "react-dom/client";
import "./styles/globals.css";
import { App } from "./app/App";

// Apply the last known theme before first paint to avoid a flash.
(() => {
  const pref = localStorage.getItem("sortd.theme") ?? "system";
  const dark =
    pref === "dark" ||
    (pref === "system" && window.matchMedia("(prefers-color-scheme: dark)").matches);
  document.documentElement.dataset.theme = dark ? "dark" : "light";
})();

// Block the default browser context menu outside editable fields: SORTD
// provides its own actions, and "Reload"/"Inspect" are not meaningful here.
if (!import.meta.env.DEV) {
  window.addEventListener("contextmenu", (e) => {
    const t = e.target as HTMLElement;
    if (!t.closest("input, textarea, .selectable")) e.preventDefault();
  });
}

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);

// Fade out the opening screen (defined in index.html) once the app has
// rendered and the logo animation has had time to finish.
const SPLASH_MIN_MS = 1500;
const splash = document.getElementById("splash");
if (splash) {
  const reduceMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
  const wait = reduceMotion ? 0 : Math.max(0, SPLASH_MIN_MS - performance.now());
  window.setTimeout(() => {
    requestAnimationFrame(() => {
      splash.classList.add("done");
      splash.addEventListener("transitionend", () => splash.remove(), { once: true });
      window.setTimeout(() => splash.remove(), 600); // fallback if no transition fires
    });
  }, wait);
}
