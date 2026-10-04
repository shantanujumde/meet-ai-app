import { render, screen } from "@testing-library/react";
import { describe, expect, test, vi } from "vitest";
import { ipc } from "@/test/ipcMock";
import { AboutSettings } from "./AboutSettings";

/** Settings, About (TUR-62): the credit Parakeet's CC-BY-4.0 licence asks for. */

vi.mock("@/ipc/client", async (importOriginal) =>
  (await import("@/test/ipcMock")).mockClient(await importOriginal()),
);

const { modelCredits } = ipc;

describe("AboutSettings", () => {
  test("credits each model with its licence and a link to it", async () => {
    modelCredits.mockResolvedValue([
      {
        text: "Parakeet speech model: parakeet-tdt-0.6b-v3 by NVIDIA, used under CC-BY-4.0.",
        url: "https://huggingface.co/nvidia/parakeet-tdt-0.6b-v3",
      },
    ]);
    render(<AboutSettings />);

    expect(await screen.findByRole("heading", { name: "About" })).toBeInTheDocument();
    expect(screen.getByText(/by NVIDIA, used under CC-BY-4\.0/)).toBeInTheDocument();
    expect(
      screen.getByRole("link", { name: "https://huggingface.co/nvidia/parakeet-tdt-0.6b-v3" }),
    ).toHaveAttribute("href", "https://huggingface.co/nvidia/parakeet-tdt-0.6b-v3");
  });

  test("draws nothing when there is nothing to credit", async () => {
    modelCredits.mockResolvedValue([]);
    const { container } = render(<AboutSettings />);
    await Promise.resolve();
    expect(container).toBeEmptyDOMElement();
  });
});
