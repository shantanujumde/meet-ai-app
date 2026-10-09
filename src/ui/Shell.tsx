/**
 * The window chrome: titlebar, sidebar, and the content column the routes
 * render into.
 *
 * The titlebar is a drag region with a 78px inset for the traffic lights.
 * TUR-102 made it slim and calm: back and forward arrows, the window title,
 * and the record control, on the canvas colour. The sidebar under it is
 * glass; the titlebar and the content column stay opaque, because body text
 * does not live on glass.
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

import { ChevronLeft, ChevronRight } from "lucide-react";
import { useRef } from "react";
import { Outlet, useLocation, useNavigate, useParams } from "react-router";
import type { PrivacyPane } from "@/ipc/types";
import { osText } from "@/lib/osText";
import { openPermissionScreen } from "@/lib/permissionRoute";
import { systemAudioOffText } from "@/lib/recordingPermission";
import { isOnboardingPath } from "@/lib/routes";
import { useAppStore } from "@/state/app";
import { useRecordingStore } from "@/state/recording";
import appIcon from "../../design-system/meet-ai/brand/meet-ai-appicon-16-fullcolor.svg";
import { HeadphoneBanner } from "./HeadphoneBanner";
import { Button, IconButton } from "./primitives";
import { RecordControl } from "./RecordControl";
import { Sidebar } from "./Sidebar";
import { useHistoryArrows } from "./useHistoryArrows";
import { useResetScrollOnRouteChange } from "./useResetScrollOnRouteChange";

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
  const togglePause = useRecordingStore((state) => state.togglePause);
  const error = useRecordingStore((state) => state.error);

  // Onboarding owns the whole window: a half-set-up app should not look
  // browsable, and there is nothing in the sidebar to browse to yet.
  const focused = isOnboardingPath(location.pathname);

  // Every route renders into the one content pane, so its scroll offset would
  // otherwise carry from one route into the next (TUR-82).
  const pane = useRef<HTMLElement>(null);
  useResetScrollOnRouteChange(pane);
  const { canBack, canForward } = useHistoryArrows();

  return (
    <div className={focused ? "shell shell--focused" : "shell"}>
      <header className="titlebar" data-tauri-drag-region>
        {!focused ? (
          <span className="flex items-center gap-1">
            <IconButton
              icon={ChevronLeft}
              label="Back"
              className="size-(--titlebar-button)"
              disabled={!canBack}
              onClick={() => navigate(-1)}
            />
            <IconButton
              icon={ChevronRight}
              label="Forward"
              className="size-(--titlebar-button)"
              disabled={!canForward}
              onClick={() => navigate(1)}
            />
          </span>
        ) : null}
        <h1 className="titlebar__title" data-tauri-drag-region>
          {/* The 16px app icon, drawn for exactly this size (brand README §4).
              Decorative: the word beside it is the name. */}
          <img src={appIcon} alt="" width={16} height={16} draggable={false} />
          meet-ai
        </h1>
        <span className="titlebar__spacer" data-tauri-drag-region />
        {!focused ? (
          <RecordControl
            status={status}
            permission={permission}
            busy={busy}
            onToggle={() => void toggle()}
            onTogglePause={() => void togglePause()}
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

      <main className="content" ref={pane}>
        {/* Two banners, and deliberately in this order: a refused recording is
            something the user just did, and outranks a standing warning. */}
        {error ? (
          <div className="banner banner--danger" role="alert">
            <p className="banner__text">{error.message}</p>
            <Button size="small" onClick={() => useRecordingStore.getState().clearError()}>
              Dismiss
            </Button>
          </div>
        ) : null}

        {!focused && permission?.state === "denied" ? (
          <div className="banner" role="status">
            <p className="banner__text">
              {deniedBannerText(permission.denied, status.phase === "recording")}
            </p>
            <Button size="small" onClick={() => openPermissionScreen(navigate)}>
              Fix this
            </Button>
          </div>
        ) : null}

        {/* TUR-65: quiet, below the two above, and only while recording. */}
        <HeadphoneBanner status={status} />

        <Outlet />
      </main>
    </div>
  );
}

const PANE_LABEL: Record<PrivacyPane, string> = {
  microphone: "Microphone",
  "audio-capture": "System Audio Recording",
  calendars: "Calendars",
};

/** Name the switch that is off, so the banner says where to go. */
function deniedBannerText(denied: PrivacyPane[], recording: boolean): string {
  const names = denied.map((pane) => PANE_LABEL[pane]);
  // TUR-87: only system audio off still records the microphone.
  if (denied.length === 1 && denied[0] === "audio-capture") {
    return systemAudioOffText(recording);
  }
  if (names.length === 0) {
    return `meet-ai cannot record ${osText("thisComputer")}'s audio yet, so recording is off.`;
  }
  const verb = names.length === 1 ? "is" : "are";
  return `${names.join(" and ")} ${verb} turned off for meet-ai in ${osText("settings")}, so recording is off.`;
}
