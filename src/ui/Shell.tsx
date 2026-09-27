/**
 * The window chrome: titlebar, sidebar, and the content column the routes
 * render into.
 *
 * The titlebar is a drag region with a 78px inset for the traffic lights.
 * Everything interactive inside it re-declares `no-drag`, or it stops
 * responding to clicks — a bug that looks like a dead button rather than like a
 * drag-region mistake.
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
      <header className="titlebar">
        <h1 className="titlebar__title">meet-ai</h1>
        <span className="titlebar__spacer" />
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
              onClick={() => navigate("/onboarding/permission")}
            >
              Fix this
            </button>
          </div>
        ) : null}

        {/* Recording with no capture behind it is the single most misleading
            state this app could have, so it is said out loud rather than
            inferred from a stub flag nobody can see. */}
        {status.phase === "recording" && status.stub ? (
          <div className="banner" role="status">
            <p className="banner__text">
              This is the recording controls working end to end. The audio capture itself is not
              wired up yet, so no sound is being saved.
            </p>
          </div>
        ) : null}

        <Outlet />
      </main>
    </div>
  );
}
