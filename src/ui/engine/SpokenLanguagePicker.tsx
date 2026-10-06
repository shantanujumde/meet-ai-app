/**
 * The spoken-language picker: which language whisper is told the audio is in
 * (`transcription.language`).
 *
 * Automatic works the language out line by line. That works for the languages
 * whisper recognises well, and not for some it transcribes: on a real Marathi
 * call, whisper heard the Marathi as English or Hindi. Picking the language is
 * the fix for those. Hinglish writes Hindi and English mixed in English
 * letters. The order (Hinglish, then by name) is Rust's.
 */

import { Languages } from "lucide-react";
import { useId } from "react";
import type { EngineChoices } from "@/ipc/types";
import { SETTINGS_SELECT, SettingsRow } from "@/ui/settings/SettingsSection";

export const AUTO_LANGUAGE = "auto";

export const SPOKEN_LANGUAGE_DETAIL =
  "Whisper only. Automatic works out the language line by line. If lines come out in the wrong language, pick the one people speak. Hinglish keeps Hindi and English mixed, in English letters.";

export function SpokenLanguagePicker({
  choices,
  onPick,
}: {
  choices: EngineChoices | null;
  onPick: (language: string) => void;
}) {
  const id = useId();
  const options = choices?.spokenLanguages ?? [];

  return (
    <SettingsRow
      icon={Languages}
      name={<label htmlFor={id}>Spoken language</label>}
      detail={SPOKEN_LANGUAGE_DETAIL}
      control={
        <select
          id={id}
          className={SETTINGS_SELECT}
          value={choices?.spokenLanguage ?? AUTO_LANGUAGE}
          disabled={choices === null}
          onChange={(event) => onPick(event.target.value)}
        >
          <option value={AUTO_LANGUAGE}>Automatic</option>
          {options.map((option) => (
            <option key={option.code} value={option.code}>
              {option.name}
            </option>
          ))}
        </select>
      }
    />
  );
}
