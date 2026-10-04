import { getCurrentWindow } from "@tauri-apps/api/window";
import { info } from "@tauri-apps/plugin-log";
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App";
import { lockDocumentScroll } from "./lib/documentScroll";
import "./index.css";
import { applyOsAttribute } from "./lib/osAttribute";
import { PromptPopup } from "./ui/PromptPopup";

/** TUR-59: the "Record this meeting?" popup window (Windows, Linux) gets only its prompt. */
const PROMPT_WINDOW = "prompt";

function windowLabel(): string | null {
  try {
    return getCurrentWindow().label;
  } catch {
    // A plain browser or a test: no Tauri window, so the main app.
    return null;
  }
}
const isPromptWindow = windowLabel() === PROMPT_WINDOW;

const container = document.getElementById("root");
if (!container) {
  throw new Error("index.html is missing its #root element");
}

// TUR-15: only the panes scroll, never the page.
lockDocumentScroll();

// TUR-57: CSS keys the per-OS chrome off <html data-os>.
applyOsAttribute();

createRoot(container).render(<StrictMode>{isPromptWindow ? <PromptPopup /> : <App />}</StrictMode>);

// SPEC §2.2: frontend logs go into the same file as the Rust ones. This line
// also doubles as proof that the webview loaded and the IPC bridge is up —
// if the CSP or the bundle were broken, it would never be written.
info("app shell mounted").catch(() => {
  // Running in a plain browser (vite dev without Tauri, or a test). There is no
  // log plugin to talk to, and that is not an error worth showing anyone.
});
