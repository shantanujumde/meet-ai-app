import { describe, expect, test, vi } from "vitest";
import type { LiveLine, LiveSpeaker, LiveTranscriptSnapshot, TranscriptUpdate } from "@/ipc/types";
import { applySnapshot, applyUpdate, EMPTY_LIVE, type LiveTranscript } from "./transcript";

/**
 * The folding rules as a property: however the window receives a meeting's
 * events — reordered, repeated, some missed before it subscribed and made up
 * for by a snapshot that lands at any moment — it ends up showing exactly what
 * Rust holds. Settled lines in `seq` order, and per speaker the guess that is
 * still open, if any.
 */

vi.mock("@/ipc/client", () => ({}));

/** Deterministic PRNG (mulberry32), so a failure names a reproducible seed. */
function rng(seed: number) {
  let state = seed >>> 0;
  return (below: number) => {
    state = (state + 0x6d2b79f5) >>> 0;
    let t = state;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return (((t ^ (t >>> 14)) >>> 0) % below) as number;
  };
}

/**
 * What two engines emit over one meeting, in the order Rust saw it: one
 * shared `seq`, and per speaker the tail contract (guesses, then a final or a
 * drop). Blank guesses and blank finals are thrown in, since engines do that.
 */
function meeting(next: (below: number) => number): TranscriptUpdate[] {
  let seq = 0;
  const events: TranscriptUpdate[] = [];
  const count = 5 + next(60);
  for (let i = 0; i < count; i++) {
    const speaker: LiveSpeaker = next(2) === 0 ? "you" : "others";
    const roll = next(10);
    const base = { seq: seq++, speaker, start_sec: i };
    if (roll < 5) {
      events.push({ kind: "volatile", ...base, text: next(8) === 0 ? " " : `guess ${base.seq}` });
    } else if (roll < 8) {
      events.push({ kind: "final", ...base, text: next(10) === 0 ? "" : `line ${base.seq}` });
    } else {
      events.push({ kind: "dropped", speaker, seq: base.seq });
    }
  }
  return events;
}

/** Rust's own board after `events`, which is what a snapshot returns. */
function board(events: TranscriptUpdate[]): LiveTranscriptSnapshot {
  const finals: LiveLine[] = [];
  const tails: Record<LiveSpeaker, LiveLine | null> = { you: null, others: null };
  for (const event of events) {
    if (event.kind === "volatile") {
      const { kind: _, ...line } = event;
      tails[event.speaker] = line.text.trim() === "" ? null : line;
    } else {
      tails[event.speaker] = null;
      if (event.kind === "final" && event.text.trim() !== "") {
        const { kind: _, ...line } = event;
        finals.push(line);
      }
    }
  }
  return {
    status: { state: "running", engine: "apple-speech", detail: null },
    finals,
    volatile: [tails.you, tails.others].filter((line): line is LiveLine => line !== null),
  };
}

function shuffle<T>(items: T[], next: (below: number) => number): T[] {
  const out = [...items];
  for (let i = out.length - 1; i > 0; i--) {
    const j = next(i + 1);
    [out[i], out[j]] = [out[j] as T, out[i] as T];
  }
  return out;
}

function expectShows(state: LiveTranscript, truth: LiveTranscriptSnapshot, seed: number) {
  expect(
    state.finals.map((line) => line.seq),
    `seed ${seed}: settled lines`,
  ).toEqual(truth.finals.map((line) => line.seq));
  for (const speaker of ["you", "others"] as const) {
    const open = truth.volatile.find((line) => line.speaker === speaker) ?? null;
    expect(state.volatile[speaker]?.seq ?? null, `seed ${seed}: ${speaker}'s guess`).toBe(
      open?.seq ?? null,
    );
  }
}

describe("the live pane converges", () => {
  test("from any delivery order, with repeats", () => {
    for (let seed = 1; seed <= 500; seed++) {
      const next = rng(seed);
      const events = meeting(next);
      const delivered = shuffle(
        [...events, ...events.filter(() => next(4) === 0)], // some arrive twice
        next,
      );
      const state = delivered.reduce(applyUpdate, EMPTY_LIVE);
      expectShows(state, board(events), seed);
    }
  });

  test("when the window opened mid-meeting and a snapshot fills in what it missed", () => {
    for (let seed = 1; seed <= 500; seed++) {
      const next = rng(seed);
      const events = meeting(next);
      // Rust took the snapshot after `taken` events; the window had already
      // subscribed at `subscribed` <= `taken`, so it hears everything from
      // there on, in any order, with the snapshot landing anywhere among them.
      const taken = next(events.length + 1);
      const subscribed = next(taken + 1);
      const snapshot = board(events.slice(0, taken));
      const heard = shuffle(events.slice(subscribed), next);
      const at = next(heard.length + 1);

      let state = heard.slice(0, at).reduce(applyUpdate, EMPTY_LIVE);
      state = applySnapshot(state, snapshot);
      state = heard.slice(at).reduce(applyUpdate, state);
      expectShows(state, board(events), seed);
    }
  });
});
