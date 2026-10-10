/**
 * Audio retention (TUR-45, TUR-85): what Settings says about how long
 * recorded audio is kept.
 *
 * Re-exported from `./client`; import from there.
 */

import { writable } from "@/lib/writable";
import {
  commands,
  DEFAULT_AUDIO_RETENTION,
  type meet_ai_lib_retention_AudioRetentionSetting,
} from "./bindings";
import { call, hasBackend } from "./client";

/**
 * `audio.retention_days` as the retention job reads it (TUR-85): `running`
 * with the days (`-1` keeps audio forever, `0` deletes it once the transcript
 * is done), or `paused` when `config.jsonc` could not be read or parsed, or its
 * `audio` section is not valid. Nothing is deleted while paused.
 */
export type AudioRetention = meet_ai_lib_retention_AudioRetentionSetting;

export async function audioRetentionDays(): Promise<AudioRetention> {
  // SPEC §3.5's default for `audio.retention_days`, from Rust (TUR-173).
  if (!hasBackend()) return writable(DEFAULT_AUDIO_RETENTION);
  return call(() => commands.audioRetentionDays());
}
