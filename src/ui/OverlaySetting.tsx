/**
 * "Show the recording window over other apps" (TUR-146):
 * `audio.show_recording_overlay` in `config.jsonc`, on by default. The
 * switch itself is {@link SettingSwitch}; the overlay follows it at once.
 */

import { PictureInPicture2 } from "lucide-react";
import { type SettingsSnapshot, setShowRecordingOverlay, showRecordingOverlay } from "@/ipc/client";
import { SettingSwitch } from "./SettingSwitch";
import { useSnapshotLoad } from "./settings/snapshot";

export const OVERLAY_SETTING_LABEL = "Show the recording window over other apps";

const pick = (snapshot: SettingsSnapshot) => snapshot.showRecordingOverlay;

export function OverlaySetting() {
  const load = useSnapshotLoad(pick, showRecordingOverlay);
  return (
    <SettingSwitch
      icon={PictureInPicture2}
      label={OVERLAY_SETTING_LABEL}
      detail="While recording, a small window floats on top with the timer, the latest words, Pause and Stop."
      load={load}
      save={setShowRecordingOverlay}
    />
  );
}
