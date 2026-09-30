/**
 * The window chrome: titlebar, sidebar, and the content column the routes
 * render into.
 *
 * The titlebar is a drag region with a 78px inset for the traffic lights.
 *
 * The CSS `-webkit-app-region: drag` in app.css is not enough on Tauri's
 * macOS WKWebView (TUR-83: the window could not be dragged at all once
 * onboarding finished and the record control mounted). Real dragging goes
 * through Tauri's `data-tauri-drag-region` attribute, wired to the native
 * `start_dragging` command and gated by the `core:window:allow-start-dragging`
 * permission in capabilities/default.json. The attribute only applies to the
 * element it is on, not its descendants, so it is repeated on every plain
 * element in the row; `RecordControl`'s button is deliberately left without
 * it so clicks keep working.
 */

import { Outlet, useLocation, useNavigate, useParams } from "react-router";
import { useAppStore } from "@/state/app";
import { useRecordingStore } from "@/state/recording";
import { RecordControl } from "./RecordControl";
import { Sidebar } from "./Sidebar";

export function Shell() {
  const location = useLocation();
  const navigate = useNavigate();
  const params = useParams<{ id?: string }>();

  const meetings = useAppStore((state) => state.meetings);
  const meetingsLoading = useAppStore((state) => state.meetingsLoading);
  const permission = useAppStore((state) => state.permission);

  const status = useRecordingStore((state) => state.status);
  const busy = useRecordingStore((state) => state.busy);
  const toggle = useRecordingStore((state) => state.toggle);
  const error = useRecordingStore((state) => state.error);

  // Onboarding owns the whole window: a half-set-up app should not look
  // browsable, and there is nothing in the sidebar to browse to yet.
  const focused = location.pathname.startsWith("/onboarding");

  return (
    <div className={focused ? "shell shell--focused" : "shell"}>
      <header className="titlebar" data-tauri-drag-region>
        <h1 className="titlebar__title" data-tauri-drag-region>
          meet-ai
        </h1>
        <span className="titlebar__spacer" data-tauri-drag-region />
        {!focused ? (
          <RecordControl
            status={status}
            permission={permission}
            busy={busy}
            onToggle={() => void toggle()}
          />
        ) : null}
      </header>

      {!focused ? (
        <Sidebar
          list={meetings}
          loading={meetingsLoading}
          recording={status}
          selectedId={params.id}
        />
      ) : null}

      <main className="content">
        {/* Two banners, and deliberately in this order: a refused recording is
            something the user just did, and outranks a standing warning. */}
        {error ? (
          <div className="banner banner--danger" role="alert">
            <p className="banner__text">{error.message}</p>
            <button
              type="button"
              className="btn btn--small"
              onClick={() => useRecordingStore.getState().clearError()}
            >
              Dismiss
            </button>
          </div>
        ) : null}

        {!focused && permission?.state === "denied" ? (
          <div className="banner" role="status">
            <p className="banner__text">
              meet-ai cannot record this Mac's audio yet, so recording is turned off.
            </p>
            <button
              type="button"
              className="btn btn--small"
              // `revisit` tells Bootstrap this is a deliberate trip, not a
              // leftover setup URL it should bounce back to the list.
              onClick={() => navigate("/onboarding/permission", { state: { revisit: true } })}
            >
              Fix this
            </button>
          </div>
        ) : null}

        <Outlet />
      </main>
    </div>
  );
}
