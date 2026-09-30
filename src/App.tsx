/**
 * The app shell and its routes.
 *
 * `HashRouter`, not `BrowserRouter`: in a bundled Tauri app the frontend is
 * served from a custom protocol with no server to rewrite deep links, so a
 * history-based route survives navigation but not a reload. A hash route works
 * identically in `tauri dev` and in the shipped bundle.
 *
 * Four routes, which is what SPEC §2.1 budgeted for.
 */

import { useEffect, useRef } from "react";
import { HashRouter, Navigate, Route, Routes, useLocation, useNavigate } from "react-router";
import { Meetings } from "@/routes/Meetings";
import { Onboarding } from "@/routes/Onboarding";
import { Review } from "@/routes/Review";
import { Settings } from "@/routes/Settings";
import { useAppStore, watchPermissionStatus } from "@/state/app";
import { useRecordingStore, watchRecordingState } from "@/state/recording";
import { watchLiveTranscript } from "@/state/transcript";
import { isRevisit } from "@/ui/permissionRoute";
import { Shell } from "@/ui/Shell";

export function App() {
  return (
    <HashRouter>
      <Bootstrap />
      <Routes>
        <Route element={<Shell />}>
          <Route index element={<Navigate to="/meetings" replace />} />
          <Route path="/meetings" element={<Meetings />} />
          <Route path="/meetings/:id" element={<Review />} />
          <Route path="/settings" element={<Settings />} />
          <Route path="/onboarding" element={<Onboarding />} />
          <Route path="/onboarding/:step" element={<Onboarding />} />
          {/* A hash that matches nothing is not worth an error page in a
              four-route app. */}
          <Route path="*" element={<Navigate to="/meetings" replace />} />
        </Route>
      </Routes>
    </HashRouter>
  );
}

/**
 * Load what the shell needs, and send a first-time user to onboarding.
 *
 * Rendered inside the router so it can navigate, and rendering nothing so it
 * cannot affect layout.
 */
function Bootstrap() {
  const navigate = useNavigate();
  const location = useLocation();

  const loadMeetings = useAppStore((state) => state.loadMeetings);
  const loadPermission = useAppStore((state) => state.loadPermission);
  const loadOnboarding = useAppStore((state) => state.loadOnboarding);
  const onboarding = useAppStore((state) => state.onboarding);
  const onboardingLoading = useAppStore((state) => state.onboardingLoading);

  useEffect(() => {
    void loadMeetings();
    // Silent: the full check plays a chime, and SPEC A7 keeps that to setup
    // and the start of a recording, not every launch.
    void loadPermission({ silent: true });
    void loadOnboarding();
    // Mirrors Rust's recording state, including changes this window did not
    // cause — ⌘⇧R firing while the app is in the background, or the menu-bar
    // item being used.
    const stopRecording = watchRecordingState();
    // Watched here rather than from the live pane, so lines keep landing while
    // the user is on another screen, and the pane has them when they come back.
    const stopTranscript = watchLiveTranscript();
    // The full check every recording start runs, which is the only one that
    // hears system audio after launch.
    const stopPermission = watchPermissionStatus();
    return () => {
      stopRecording();
      stopTranscript();
      stopPermission();
    };
  }, [loadMeetings, loadPermission, loadOnboarding]);

  // Starting a recording creates the meeting folder and stopping finishes it,
  // so either end of one changes the list.
  //
  // Subscribing to the store rather than depending on the phase in a render:
  // the phase is a *trigger* here, not a value this component displays, and
  // writing it as an effect dependency makes it look like the latter.
  useEffect(() => {
    return useRecordingStore.subscribe((state, previous) => {
      if (state.status.phase !== previous.status.phase) void loadMeetings();
    });
  }, [loadMeetings]);

  // Refs, not effect state: `navigate` changes identity with every location
  // change, so the effect below re-subscribes often, and the meeting already
  // opened must survive that or leaving it would be undone on the next event.
  const pathnameRef = useRef(location.pathname);
  pathnameRef.current = location.pathname;
  const openedMeetingId = useRef<string | null>(null);

  // A recording that starts — from the button, the menu bar, or ⌘⇧R with the
  // window hidden — opens its meeting, so the live transcript is on screen
  // without the user hunting for the "● Recording" row. Once per meeting id,
  // not per state event: a user who walks away mid-meeting stays where they
  // went, and only the next recording's id moves them again.
  //
  // The id only appears once Rust reaches `recording` (it is null through
  // `starting`), so a new id is the transition. A window opened mid-meeting
  // sees the id for the first time too, and opening onto the running meeting
  // is the useful answer there, so that is deliberately not special-cased.
  //
  // Rust does not gate ⌘⇧R on onboarding, so a recording can start during
  // the wizard; it still counts as seen, but must not pull the user out.
  useEffect(() => {
    return useRecordingStore.subscribe(({ status }) => {
      const id = status.meetingId;
      if (status.phase === "idle" || id === null || id === openedMeetingId.current) return;
      openedMeetingId.current = id;
      const inOnboarding =
        pathnameRef.current.startsWith("/onboarding") ||
        useAppStore.getState().onboarding?.completedAt === null;
      if (inOnboarding) return;
      navigate(`/meetings/${encodeURIComponent(id)}`);
    });
  }, [navigate]);

  useEffect(() => {
    if (onboardingLoading || onboarding === null) return;
    const onOnboardingRoute = location.pathname.startsWith("/onboarding");
    if (onboarding.completedAt === null) {
      // Not finished yet: keep the user inside the wizard.
      if (!onOnboardingRoute) navigate("/onboarding", { replace: true });
      return;
    }
    // Already finished: a wizard URL left over from a previous, unfinished
    // session (the single-instance window was simply refocused, never
    // re-loaded) must not trap an otherwise-done user on setup forever. A trip
    // the user asked for — the shell's "Fix this" banner — is let through.
    if (onOnboardingRoute && !isRevisit(location.state)) navigate("/meetings", { replace: true });
  }, [onboarding, onboardingLoading, location.pathname, location.state, navigate]);

  return null;
}
