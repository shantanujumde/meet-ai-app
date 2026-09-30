import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, test } from "vitest";
import type { LiveLine } from "@/ipc/types";
import { EMPTY_LIVE, type LiveTranscript as LiveState } from "@/state/transcript";
import { isAtBottom, LiveTranscript } from "./LiveTranscript";

/**
 * The live pane, rendered from plain state — no store, no IPC.
 *
 * jsdom does no layout, so the scroll tests give the scroller its dimensions
 * by hand. That is enough to test the decision the pane makes (follow, or stay
 * put), which is the part that has to be right.
 */

const line = (seq: number, speaker: LiveLine["speaker"], text: string): LiveLine => ({
  seq,
  speaker,
  start_sec: seq * 5,
  text,
});

function stateWith(partial: Partial<LiveState>): LiveState {
  return {
    ...EMPTY_LIVE,
    status: { state: "running", engine: "apple-speech", detail: null },
    ...partial,
  };
}

/** Give the scroller a fake geometry, with a writable scrollTop. */
function fakeGeometry(
  element: HTMLElement,
  geometry: { scrollHeight: number; clientHeight: number },
) {
  let top = 0;
  Object.defineProperty(element, "scrollHeight", {
    configurable: true,
    get: () => geometry.scrollHeight,
  });
  Object.defineProperty(element, "clientHeight", {
    configurable: true,
    get: () => geometry.clientHeight,
  });
  Object.defineProperty(element, "scrollTop", {
    configurable: true,
    get: () => top,
    set: (value: number) => {
      top = Math.min(value, geometry.scrollHeight - geometry.clientHeight);
    },
  });
}

describe("isAtBottom", () => {
  test("at the bottom, and within the slack of it, counts", () => {
    expect(isAtBottom({ scrollTop: 600, scrollHeight: 1000, clientHeight: 400 })).toBe(true);
    expect(isAtBottom({ scrollTop: 590, scrollHeight: 1000, clientHeight: 400 })).toBe(true);
  });

  test("scrolled up to read does not", () => {
    expect(isAtBottom({ scrollTop: 300, scrollHeight: 1000, clientHeight: 400 })).toBe(false);
  });

  test("content that does not fill the pane is always at the bottom", () => {
    expect(isAtBottom({ scrollTop: 0, scrollHeight: 200, clientHeight: 400 })).toBe(true);
  });
});

describe("LiveTranscript", () => {
  test("a settled line and a guess render differently, with speaker and timestamp", () => {
    render(
      <LiveTranscript
        live={stateWith({
          finals: [line(1, "others", "Morning everyone.")],
          volatile: { you: line(2, "you", "Sessions are still"), others: null },
        })}
      />,
    );

    const settled = screen.getByText("Morning everyone.").closest("li");
    const guess = screen.getByText("Sessions are still").closest("li");
    expect(settled).not.toHaveAttribute("data-volatile");
    expect(guess).toHaveAttribute("data-volatile");
    expect(settled?.textContent).toMatch(/00:00:05/);
    expect(settled?.textContent).toMatch(/Others:/);
    expect(guess?.textContent).toMatch(/You \(still speaking\):/);
  });

  test("settled lines are in a polite log, and guesses are outside it", () => {
    render(
      <LiveTranscript
        live={stateWith({
          finals: [line(1, "you", "Settled.")],
          volatile: { you: null, others: line(2, "others", "Still going") },
        })}
      />,
    );
    const log = screen.getByRole("log", { name: /live transcript/i });
    expect(log).toHaveAttribute("aria-live", "polite");
    expect(log).toHaveTextContent("Settled.");
    expect(log).not.toHaveTextContent("Still going");
  });

  test("a failed engine says recording continues, with the reason in plain words", () => {
    render(
      <LiveTranscript
        live={stateWith({
          status: {
            state: "failed",
            engine: "whisper",
            detail: "The speech engine stopped responding.",
          },
          finals: [line(1, "you", "Before it stopped.")],
        })}
      />,
    );
    const notice = screen.getByRole("alert");
    expect(notice).toHaveTextContent(/transcription stopped/i);
    expect(notice).toHaveTextContent(/recording is still going/i);
    expect(notice).toHaveTextContent("The speech engine stopped responding.");
    // What was already transcribed stays on screen.
    expect(screen.getByText("Before it stopped.")).toBeInTheDocument();
  });

  test("no failure, no notice", () => {
    render(<LiveTranscript live={stateWith({})} />);
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    expect(screen.getByText(/lines appear here/i)).toBeInTheDocument();
  });

  test("at the bottom, a new line is followed", () => {
    const geometry = { scrollHeight: 1000, clientHeight: 400 };
    const { rerender } = render(
      <LiveTranscript live={stateWith({ finals: [line(1, "you", "a")] })} />,
    );
    const scroller = screen.getByRole("log").parentElement as HTMLElement;
    fakeGeometry(scroller, geometry);
    scroller.scrollTop = 600;
    fireEvent.scroll(scroller);

    geometry.scrollHeight = 1100;
    rerender(
      <LiveTranscript
        live={stateWith({ finals: [line(1, "you", "a"), line(2, "others", "b")] })}
      />,
    );

    expect(scroller.scrollTop).toBe(700);
    expect(screen.queryByRole("button", { name: /new lines/i })).not.toBeInTheDocument();
  });

  test("scrolled up to read, a new line does not move the reader, and offers a way down", () => {
    const geometry = { scrollHeight: 1000, clientHeight: 400 };
    const { rerender } = render(
      <LiveTranscript live={stateWith({ finals: [line(1, "you", "a")] })} />,
    );
    const scroller = screen.getByRole("log").parentElement as HTMLElement;
    fakeGeometry(scroller, geometry);
    scroller.scrollTop = 200;
    fireEvent.scroll(scroller);

    geometry.scrollHeight = 1100;
    rerender(
      <LiveTranscript
        live={stateWith({ finals: [line(1, "you", "a"), line(2, "others", "b")] })}
      />,
    );

    expect(scroller.scrollTop).toBe(200);
    const jump = screen.getByRole("button", { name: /new lines below/i });

    fireEvent.click(jump);
    expect(scroller.scrollTop).toBe(700);
    expect(screen.queryByRole("button", { name: /new lines/i })).not.toBeInTheDocument();
  });

  test("scrolling back down by hand clears the button and resumes following", () => {
    const geometry = { scrollHeight: 1000, clientHeight: 400 };
    const one = [line(1, "you", "a")];
    const two = [...one, line(2, "others", "b")];
    const three = [...two, line(3, "you", "c")];
    const { rerender } = render(<LiveTranscript live={stateWith({ finals: one })} />);
    const scroller = screen.getByRole("log").parentElement as HTMLElement;
    fakeGeometry(scroller, geometry);
    scroller.scrollTop = 100;
    fireEvent.scroll(scroller);

    geometry.scrollHeight = 1100;
    rerender(<LiveTranscript live={stateWith({ finals: two })} />);
    expect(screen.getByRole("button", { name: /new lines below/i })).toBeInTheDocument();

    scroller.scrollTop = 700; // the reader scrolls to the bottom themselves
    fireEvent.scroll(scroller);
    expect(screen.queryByRole("button", { name: /new lines/i })).not.toBeInTheDocument();

    geometry.scrollHeight = 1200;
    rerender(<LiveTranscript live={stateWith({ finals: three })} />);
    expect(scroller.scrollTop).toBe(800);
    expect(screen.queryByRole("button", { name: /new lines/i })).not.toBeInTheDocument();
  });

  test("scrolled up, a guess changing is not a new line and raises no button", () => {
    const geometry = { scrollHeight: 1000, clientHeight: 400 };
    const finals = [line(1, "you", "a")];
    const { rerender } = render(
      <LiveTranscript
        live={stateWith({ finals, volatile: { you: null, others: line(2, "others", "b") } })}
      />,
    );
    const scroller = screen.getByRole("log").parentElement as HTMLElement;
    fakeGeometry(scroller, geometry);
    scroller.scrollTop = 200;
    fireEvent.scroll(scroller);

    rerender(
      <LiveTranscript
        live={stateWith({ finals, volatile: { you: null, others: line(3, "others", "b c") } })}
      />,
    );

    expect(scroller.scrollTop).toBe(200);
    expect(screen.queryByRole("button", { name: /new lines/i })).not.toBeInTheDocument();
  });
});
