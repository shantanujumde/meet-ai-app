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

import { useEffect } from "react";
import { HashRouter, Navigate, Route, Routes, useLocation, useNavigate } from "react-router";
import { Meetings } from "@/routes/Meetings";
import { Onboarding } from "@/routes/Onboarding";
import { Review } from "@/routes/Review";
import { Settings } from "@/routes/Settings";
import { useAppStore } from "@/state/app";
import { useRecordingStore, watchRecordingState } from "@/state/recording";
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
    void loadPermission();
    void loadOnboarding();
    // Mirrors Rust's recording state, including changes this window did not
    // cause — ⌘⇧R firing while the app is in the background, or the menu-bar
    // item being used.
    return watchRecordingState();
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

  useEffect(() => {
    if (onboardingLoading || onboarding === null) return;
    if (onboarding.completedAt !== null) return;
    if (location.pathname.startsWith("/onboarding")) return;
    navigate("/onboarding", { replace: true });
  }, [onboarding, onboardingLoading, location.pathname, navigate]);

  return null;
}
