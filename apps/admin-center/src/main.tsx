import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { HashRouter } from "react-router-dom";
import "./index.css";
import App from "./App";
import { isDesktop } from "./lib/api";

// In the desktop app, open external links in the system browser.
if (isDesktop) {
  document.addEventListener("click", (e) => {
    const a = (e.target as HTMLElement).closest?.("a[href^='http']") as HTMLAnchorElement | null;
    if (!a || a.target !== "_blank") return;
    e.preventDefault();
    import("@tauri-apps/plugin-opener")
      .then((m) => m.openUrl(a.href))
      .catch((err) => { console.error("Couldn't open link", err); window.open(a.href, "_blank", "noopener"); });
  });
}

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <HashRouter>
      <App />
    </HashRouter>
  </StrictMode>,
);
