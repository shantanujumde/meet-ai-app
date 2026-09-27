import { info } from "@tauri-apps/plugin-log";
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App";
import "./index.css";

const container = document.getElementById("root");
if (!container) {
  throw new Error("index.html is missing its #root element");
}

createRoot(container).render(
  <StrictMode>
    <App />
  </StrictMode>,
);

// SPEC §2.2: frontend logs go into the same file as the Rust ones. This line
// also doubles as proof that the webview loaded and the IPC bridge is up —
// if the CSP or the bundle were broken, it would never be written.
info("app shell mounted").catch(() => {
  // Running in a plain browser (vite dev without Tauri, or a test). There is no
  // log plugin to talk to, and that is not an error worth showing anyone.
});
