/**
 * "Show the recording window over other apps" (TUR-146):
 * `audio.show_recording_overlay` in `config.jsonc`, on by default. The
 * switch itself is {@link SettingSwitch}; the overlay follows it at once.
 */

import { PictureInPicture2 } from "lucide-react";
import { setShowRecordingOverlay, showRecordingOverlay } from "@/ipc/client";
import { SettingSwitch } from "./SettingSwitch";

export const OVERLAY_SETTING_LABEL = "Show the recording window over other apps";

export function OverlaySetting() {
  return (
    <SettingSwitch
      icon={PictureInPicture2}
      label={OVERLAY_SETTING_LABEL}
      detail="While recording, a small window floats on top with the timer, the latest words, Pause and Stop."
      load={showRecordingOverlay}
      save={setShowRecordingOverlay}
    />
  );
}
