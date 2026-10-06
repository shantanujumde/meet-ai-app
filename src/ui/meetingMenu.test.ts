import { describe, expect, test } from "vitest";
import { meetingSummary, transcriptLine } from "@/test/fixtures";
import {
  deleteQuestion,
  type MeetingMenuContext,
  meetingMenuItems,
  transcriptText,
} from "./meetingMenu";

const READY: MeetingMenuContext = {
  isRecording: false,
  harnessIsNone: false,
  runStatus: { state: "idle" },
};

/** The actions, group by group. */
function actions(...args: Parameters<typeof meetingMenuItems>) {
  return meetingMenuItems(...args).map((group) => group.map((entry) => entry.action));
}

describe("meetingMenuItems (TUR-116)", () => {
  test("an ordinary meeting: four groups, Delete last and in red", () => {
    const groups = meetingMenuItems(meetingSummary({ hasNotes: true }), READY);
    expect(groups.map((group) => group.map((entry) => entry.label))).toEqual([
      ["Open", "Show in Finder", "Copy folder path"],
      ["Write notes now", "Turn notes off for this meeting"],
      ["Copy transcript", "Copy notes"],
      ["Rename…", "Delete…"],
    ]);
    expect(groups[3]?.[1]).toMatchObject({ action: "delete", danger: true });
  });

  test("the meeting being recorded offers only Open and Show in Finder", () => {
    expect(actions(meetingSummary(), { ...READY, isRecording: true })).toEqual([
      ["open", "reveal"],
    ]);
    // The list's own word for it counts too.
    expect(actions(meetingSummary({ recordingState: "recording" }), READY)).toEqual([
      ["open", "reveal"],
    ]);
  });

  test("notes already written: Write notes again", () => {
    const groups = meetingMenuItems(meetingSummary({ hasAnalysis: true }), READY);
    expect(groups[1]?.[0]?.label).toBe("Write notes again");
  });

  test("a run going: Stop writing notes instead", () => {
    expect(actions(meetingSummary(), { ...READY, runStatus: { state: "running" } })[1]).toEqual([
      "stop-notes",
      "notes-off",
    ]);
  });

  test("no agent set up: Copy prompt instead of Write notes", () => {
    expect(actions(meetingSummary(), { ...READY, harnessIsNone: true })[1]).toEqual([
      "copy-prompt",
      "notes-off",
    ]);
  });

  test("before the agent answer arrives, neither is offered yet", () => {
    expect(actions(meetingSummary(), { ...READY, harnessIsNone: null })[1]).toEqual(["notes-off"]);
  });

  test("notes off: only Turn notes on, nothing that would send it", () => {
    expect(actions(meetingSummary({ notesOff: true }), READY)[1]).toEqual(["notes-on"]);
  });

  test("items that do not apply are left out, not greyed", () => {
    const groups = actions(meetingSummary({ lineCount: 0, hasNotes: false }), READY);
    // Nothing to write notes from and nothing to copy: that whole group goes.
    expect(groups).toEqual([
      ["open", "reveal", "copy-path"],
      ["notes-off"],
      ["rename", "delete"],
    ]);
  });
});

describe("deleteQuestion", () => {
  test("says what goes to the Trash", () => {
    expect(deleteQuestion("Standup", false)).toEqual({
      title: "Delete “Standup”?",
      body: "Its transcript, notes and audio will be moved to the Trash.",
    });
  });

  test("says shared tickets stay when there are any", () => {
    expect(deleteQuestion("Standup", true).body).toBe(
      "Its transcript, notes and audio will be moved to the Trash. Tickets made from it stay in Tickets.",
    );
  });
});

test("transcriptText writes the transcript.md line format", () => {
  expect(
    transcriptText([
      transcriptLine({ time: "00:00:04", speaker: "Others", text: "Morning." }),
      transcriptLine({ time: "00:00:11", speaker: "You", text: "Hi." }),
    ]),
  ).toBe("[00:00:04] Others: Morning.\n[00:00:11] You: Hi.");
});
