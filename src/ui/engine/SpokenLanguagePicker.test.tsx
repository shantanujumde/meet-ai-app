import { render, screen } from "@testing-library/react";
import { describe, expect, test, vi } from "vitest";
import type { EngineChoices } from "@/ipc/types";
import {
  LANGUAGE_IGNORED,
  SPOKEN_LANGUAGE_DETAIL,
  SpokenLanguagePicker,
} from "./SpokenLanguagePicker";

/**
 * The picker's state (TUR-157): enabled only when the backend says the engine
 * that will run uses the spoken language, and otherwise disabled with its
 * reason as the grey line. The saved language is still shown, not reset.
 */

function choices(overrides: Partial<EngineChoices> = {}): EngineChoices {
  return {
    engine: "whisper",
    model: "large-v3-turbo-q5_0",
    auto: "apple-speech",
    apple: { available: true, reason: null },
    whisper: { available: true, reason: null },
    parakeet: { available: false, reason: "Download the Parakeet model first." },
    parakeetModel: {
      id: "parakeet-tdt-0.6b-v3",
      displayName: "Parakeet (25 European languages)",
      goodFor: "Fast on a computer without a graphics card, so live captions keep up.",
      bytes: 670_479_942,
      installed: false,
      runtimeReady: true,
      languages: ["English", "German"],
    },
    languages: ["en-US"],
    spokenLanguage: "mr",
    spokenLanguages: [
      { code: "en", name: "English" },
      { code: "mr", name: "Marathi" },
    ],
    configProblem: null,
    selectionError: null,
    honoursLanguage: true,
    languageIgnoredReason: null,
    ...overrides,
  };
}

function picker(): HTMLSelectElement {
  const select = screen.getByRole("combobox", { name: "Spoken language" });
  if (!(select instanceof HTMLSelectElement)) throw new Error("not a select");
  return select;
}

describe("SpokenLanguagePicker", () => {
  test("is enabled with its usual line when the engine uses the language", () => {
    render(<SpokenLanguagePicker choices={choices()} onPick={vi.fn()} />);
    expect(picker()).toBeEnabled();
    expect(screen.getByText(SPOKEN_LANGUAGE_DETAIL)).toBeInTheDocument();
  });

  test("is disabled and says why when the model only transcribes English", () => {
    const reason =
      "This model only transcribes English. Pick a multilingual model to choose the language.";
    render(
      <SpokenLanguagePicker
        choices={choices({
          model: "small.en-q5_1",
          honoursLanguage: false,
          languageIgnoredReason: reason,
        })}
        onPick={vi.fn()}
      />,
    );
    expect(picker()).toBeDisabled();
    expect(screen.getByText(reason)).toBeInTheDocument();
    expect(screen.queryByText(SPOKEN_LANGUAGE_DETAIL)).toBeNull();
    // The saved language stays as it is.
    expect(picker()).toHaveValue("mr");
  });

  test("falls back to a plain line when the backend gives no reason", () => {
    render(
      <SpokenLanguagePicker
        choices={choices({ honoursLanguage: false, languageIgnoredReason: null })}
        onPick={vi.fn()}
      />,
    );
    expect(picker()).toBeDisabled();
    expect(screen.getByText(LANGUAGE_IGNORED)).toBeInTheDocument();
  });

  test("is disabled while the choices are loading", () => {
    render(<SpokenLanguagePicker choices={null} onPick={vi.fn()} />);
    expect(picker()).toBeDisabled();
    expect(screen.getByText(SPOKEN_LANGUAGE_DETAIL)).toBeInTheDocument();
  });
});
