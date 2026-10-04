import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, test, vi } from "vitest";
import type { EngineChoices, ModelView } from "@/ipc/types";
import { ipc } from "@/test/ipcMock";
import { EngineSummary, NEXT_RECORDING_NOTE } from "./EngineSummary";
import { WHISPER_ONLY_NOTE } from "./ModelList";

/**
 * The speech card against a stubbed backend: the engine picker (TUR-75), the
 * model rows as a picker, and what each model is good for (TUR-79). Which
 * choices are disabled, and why, is the backend's answer; these tests check
 * the card shows it and sends the right save.
 */

vi.mock("@/ipc/client", async (importOriginal) =>
  (await import("@/test/ipcMock")).mockClient(await importOriginal()),
);

const { engineChoices, modelCatalogue, setTranscription } = ipc;

function choices(overrides: Partial<EngineChoices> = {}): EngineChoices {
  return {
    engine: "auto",
    model: "large-v3-turbo-q5_0",
    auto: "apple-speech",
    apple: { available: true, reason: null },
    whisper: { available: true, reason: null },
    languages: ["en-US"],
    ...overrides,
  };
}

function small(overrides: Partial<ModelView> = {}): ModelView {
  return {
    id: "small.en-q5_1",
    filename: "ggml-small.en-q5_1.bin",
    bytes: 190_098_681,
    installed: true,
    displayName: "Small (English only)",
    goodFor:
      "Lightweight and fast. Good for clear English calls; less accurate with accents, cross-talk or jargon.",
    tags: ["fast", "light", "english-only"],
    recommended: null,
    ...overrides,
  };
}

function large(overrides: Partial<ModelView> = {}): ModelView {
  return {
    id: "large-v3-turbo-q5_0",
    filename: "ggml-large-v3-turbo-q5_0.bin",
    bytes: 574_041_195,
    installed: false,
    displayName: "Large turbo (multilingual)",
    goodFor:
      "Most accurate. Understands ~100 languages and mixed-language calls. Slower and uses more memory.",
    tags: ["most-accurate", "multilingual", "slower"],
    recommended:
      "This Mac has Apple silicon and 32 GB of memory, enough for the most accurate model.",
    ...overrides,
  };
}

/** Render, and wait for the probe and the catalogue to answer. */
async function renderCard() {
  render(<EngineSummary />);
  await waitFor(() => expect(screen.queryByText("Checking…")).toBeNull());
  await waitFor(() => expect(screen.queryByText("Reading the model list…")).toBeNull());
}

function engine(name: RegExp): HTMLElement {
  return screen.getByRole("radio", { name });
}

/** The row around a model, found by its plain name. */
function modelRow(name: string): HTMLElement {
  const row = screen.getByText(name).closest(".flex-col.items-stretch");
  if (!(row instanceof HTMLElement)) throw new Error(`no row for ${name}`);
  return row;
}

describe("EngineSummary: the engine picker", () => {
  test("draws the three choices at once and holds them until the probe answers", async () => {
    engineChoices.mockReturnValue(new Promise(() => {}));
    render(<EngineSummary />);

    expect(engine(/Automatic \(recommended\)/)).toBeDisabled();
    expect(engine(/Apple \(built in\)/)).toBeDisabled();
    expect(engine(/Whisper/)).toBeDisabled();
    expect(screen.getByText("Checking…")).toBeInTheDocument();
  });

  test("shows the saved choice and what Automatic does on this Mac", async () => {
    engineChoices.mockResolvedValue(choices());
    await renderCard();

    expect(engine(/Automatic/)).toBeChecked();
    expect(screen.getByText("Uses Apple's engine on this Mac")).toBeInTheDocument();
    expect(screen.getByText(NEXT_RECORDING_NOTE)).toBeInTheDocument();
  });

  test("Automatic says when it lands on Whisper, or that nothing is ready", async () => {
    engineChoices.mockResolvedValue(
      choices({ auto: "whisper", apple: { available: false, reason: "Needs macOS 26 or later." } }),
    );
    const { unmount } = render(<EngineSummary />);
    expect(await screen.findByText(/Uses Whisper on this Mac/)).toBeInTheDocument();
    unmount();

    engineChoices.mockResolvedValue(choices({ auto: null }));
    render(<EngineSummary />);
    expect(await screen.findByText(/Nothing is ready yet/)).toBeInTheDocument();
  });

  test("picking Apple saves apple-speech with the saved model", async () => {
    const user = userEvent.setup();
    engineChoices.mockResolvedValue(choices());
    await renderCard();

    await user.click(screen.getByText("Apple (built in)"));
    expect(setTranscription).toHaveBeenCalledWith("apple-speech", "large-v3-turbo-q5_0");
    await waitFor(() => expect(engine(/Apple \(built in\)/)).toBeChecked());
  });

  test("picking Automatic saves auto", async () => {
    const user = userEvent.setup();
    engineChoices.mockResolvedValue(choices({ engine: "apple-speech" }));
    await renderCard();

    await user.click(screen.getByText("Automatic (recommended)"));
    expect(setTranscription).toHaveBeenCalledWith("auto", "large-v3-turbo-q5_0");
  });

  test("picking Whisper also picks a downloaded model when the saved one is not here", async () => {
    const user = userEvent.setup();
    engineChoices.mockResolvedValue(choices());
    modelCatalogue.mockResolvedValue([small(), large()]);
    await renderCard();

    await user.click(screen.getByText("Whisper"));
    expect(setTranscription).toHaveBeenCalledWith("whisper", "small.en-q5_1");
  });

  test("picking Whisper before the model list loads still saves, with the saved model", async () => {
    const user = userEvent.setup();
    engineChoices.mockResolvedValue(choices());
    modelCatalogue.mockReturnValue(new Promise(() => {}));
    render(<EngineSummary />);
    await waitFor(() => expect(screen.queryByText("Checking…")).toBeNull());

    await user.click(screen.getByText("Whisper"));
    expect(setTranscription).toHaveBeenCalledWith("whisper", "large-v3-turbo-q5_0");
  });

  test("Apple is disabled with the backend's reason", async () => {
    engineChoices.mockResolvedValue(
      choices({ auto: null, apple: { available: false, reason: "Needs macOS 26 or later." } }),
    );
    await renderCard();

    expect(engine(/Apple \(built in\)/)).toBeDisabled();
    expect(screen.getByText("Needs macOS 26 or later.")).toBeInTheDocument();
  });

  test("Whisper is disabled until a model is downloaded, and says so", async () => {
    const user = userEvent.setup();
    engineChoices.mockResolvedValue(
      choices({ whisper: { available: false, reason: "Download a model first." } }),
    );
    modelCatalogue.mockResolvedValue([small({ installed: false }), large()]);
    await renderCard();

    expect(engine(/Whisper/)).toBeDisabled();
    expect(screen.getByText("Download a model first.")).toBeInTheDocument();
    await user.click(screen.getByText("Whisper"));
    expect(setTranscription).not.toHaveBeenCalled();
  });

  test("a refused save puts the radios back and shows why", async () => {
    const user = userEvent.setup();
    engineChoices.mockResolvedValue(choices());
    setTranscription.mockRejectedValue({
      domain: "app",
      kind: "engine-unavailable",
      message: "Needs macOS 26 or later.",
    });
    await renderCard();

    await user.click(screen.getByText("Apple (built in)"));
    await waitFor(() => expect(engine(/Automatic/)).toBeChecked());
    expect(await screen.findByText(/Needs macOS 26 or later/)).toBeInTheDocument();
  });

  test("Apple's row names the languages it has installed, in words", async () => {
    engineChoices.mockResolvedValue(choices({ languages: ["en-US", "fr-FR"] }));
    await renderCard();

    expect(
      screen.getByText(/Built into macOS 26\. Fast, private, no download\. Languages: .*English/),
    ).toBeInTheDocument();
    expect(screen.getByText(/French/)).toBeInTheDocument();
  });

  test("the speech helper path is folded under Details", async () => {
    const user = userEvent.setup();
    ipc.engineEnvironment.mockResolvedValue({
      sidecar: "/Applications/meet-ai.app/Contents/MacOS/meet-stt",
      whisperModel: null,
      locale: "en-US",
      modelId: "large-v3-turbo-q5_0",
      modelsDir: null,
    });
    await renderCard();

    const path = await screen.findByText("/Applications/meet-ai.app/Contents/MacOS/meet-stt");
    expect(path).not.toBeVisible();
    await user.click(screen.getByText("Details"));
    expect(path).toBeVisible();
  });
});

describe("EngineSummary: the model rows", () => {
  test("each row says what the model is good for, in plain words", async () => {
    modelCatalogue.mockResolvedValue([small(), large()]);
    await renderCard();

    const row = modelRow("Small (English only)");
    expect(within(row).getByText(/Lightweight and fast/)).toBeInTheDocument();
    for (const tag of ["Fast", "Light", "English only"]) {
      expect(within(row).getByText(tag)).toBeInTheDocument();
    }
    // The id stays in the muted detail line, after the size.
    expect(within(row).getByText(/190 MB · small\.en-q5_1/)).toBeInTheDocument();
    // …and never in the main line.
    expect(screen.getByText("Small (English only)").textContent).not.toMatch(/q5|ggml|RTF/);

    const big = modelRow("Large turbo (multilingual)");
    for (const tag of ["Most accurate", "Multilingual", "Slower"]) {
      expect(within(big).getByText(tag)).toBeInTheDocument();
    }
  });

  test("the recommended model is marked, with its reason", async () => {
    modelCatalogue.mockResolvedValue([small(), large()]);
    await renderCard();

    const row = modelRow("Large turbo (multilingual)");
    expect(within(row).getByText("Recommended")).toBeInTheDocument();
    expect(within(row).getByText(/Apple silicon and 32 GB/)).toBeInTheDocument();
    expect(within(modelRow("Small (English only)")).queryByText("Recommended")).toBeNull();
  });

  test("picking a downloaded model saves it, keeping the engine", async () => {
    const user = userEvent.setup();
    engineChoices.mockResolvedValue(choices({ engine: "whisper", model: "large-v3-turbo-q5_0" }));
    modelCatalogue.mockResolvedValue([small(), large({ installed: true })]);
    await renderCard();

    await user.click(screen.getByText("Small (English only)"));
    expect(setTranscription).toHaveBeenCalledWith("whisper", "small.en-q5_1");
    await waitFor(() =>
      expect(screen.getByRole("radio", { name: /Small \(English only\)/ })).toBeChecked(),
    );
  });

  test("a model that is not downloaded cannot be picked, only downloaded", async () => {
    modelCatalogue.mockResolvedValue([small(), large()]);
    await renderCard();

    expect(screen.queryByRole("radio", { name: /Large turbo/ })).toBeNull();
    expect(
      within(modelRow("Large turbo (multilingual)")).getByRole("button", { name: "Download" }),
    ).toBeInTheDocument();
  });

  test("with Whisper running, the saved model is marked In use and there is no note", async () => {
    engineChoices.mockResolvedValue(choices({ engine: "whisper", model: "small.en-q5_1" }));
    modelCatalogue.mockResolvedValue([small(), large()]);
    await renderCard();

    expect(within(modelRow("Small (English only)")).getByText("In use")).toBeInTheDocument();
    expect(within(modelRow("Large turbo (multilingual)")).queryByText("In use")).toBeNull();
    expect(screen.queryByText(WHISPER_ONLY_NOTE)).toBeNull();
  });

  test("when Automatic lands on Whisper, its model is in use too", async () => {
    engineChoices.mockResolvedValue(
      choices({ engine: "auto", auto: "whisper", model: "small.en-q5_1" }),
    );
    modelCatalogue.mockResolvedValue([small()]);
    await renderCard();

    expect(within(modelRow("Small (English only)")).getByText("In use")).toBeInTheDocument();
  });

  test("with another engine, the list says it is only used with Whisper", async () => {
    engineChoices.mockResolvedValue(choices({ engine: "apple-speech", model: "small.en-q5_1" }));
    modelCatalogue.mockResolvedValue([small(), large()]);
    await renderCard();

    expect(screen.getByText(WHISPER_ONLY_NOTE)).toBeInTheDocument();
    expect(screen.queryByText("In use")).toBeNull();
    // Still the saved pick, ready for when Whisper is chosen.
    expect(screen.getByRole("radio", { name: /Small \(English only\)/ })).toBeChecked();
  });
});
