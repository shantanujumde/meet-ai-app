/**
 * What a meeting's ⋯ menu and right-click menu offer (TUR-116), and doing it.
 *
 * {@link meetingMenuItems} is the one list both menus draw, so they cannot
 * drift: it picks which actions apply to this meeting right now and in what
 * groups. Actions that do not apply are left out rather than greyed, so the
 * menu only ever lists what can be done. {@link useMeetingMenu} turns that
 * list into menu items that act, and asks Rust for the two things the list
 * depends on (whether an agent is set up, whether a notes run is going) each
 * time a menu opens, since either can change while the window is open.
 *
 * Every action is one the app already has somewhere else, with the same
 * command behind it: the meeting header's Show in Finder and Copy folder
 * path, the notes panel's start and stop, the "Make notes for this meeting"
 * switch, Copy prompt, renaming. Delete is the one new command.
 */

import { ask, message } from "@tauri-apps/plugin-dialog";
import { useCallback, useEffect, useRef, useState } from "react";
import { useLocation, useNavigate } from "react-router";
import {
  cancelNotesRun,
  copyPromptFallback,
  deleteMeeting,
  listTickets,
  notesRunStatus,
  readMeeting,
  revealMeeting,
  setMeetingNotes,
  startNotesRun,
  wrapUpPrompt,
} from "@/ipc/client";
import type { MeetingSummary, NotesRunState, TranscriptLine } from "@/ipc/types";
import { toUiError } from "@/ipc/types";
import { copyText } from "@/lib/clipboard";
import { showsCopyPrompt } from "@/lib/copyPrompt";
import { osText } from "@/lib/osText";
import { MEETINGS, meetingPath } from "@/lib/routes";
import { useAppStore } from "@/state/app";
import { COPY_PATH_LABEL, SHOW_IN_FILE_MANAGER_LABEL } from "./MeetingHeader";
import type { MenuGroups } from "./menu";

/** Everything the menu can do for one meeting. */
export type MeetingAction =
  | "open"
  | "reveal"
  | "copy-path"
  | "write-notes"
  | "stop-notes"
  | "copy-prompt"
  | "notes-off"
  | "notes-on"
  | "copy-transcript"
  | "copy-notes"
  | "rename"
  | "delete";

export type MeetingMenuEntry = { action: MeetingAction; label: string; danger?: boolean };

/** What the list depends on besides the meeting itself. */
export type MeetingMenuContext = {
  /** This meeting is the one recording right now. */
  isRecording: boolean;
  /** No agent is set up, so Copy prompt stands in for writing notes. Null until asked. */
  harnessIsNone: boolean | null;
  /** This meeting's notes run, or null until asked. */
  runStatus: NotesRunState | null;
};

export const OPEN_LABEL = "Open";
export const WRITE_NOTES_LABEL = "Write notes now";
export const WRITE_NOTES_AGAIN_LABEL = "Write notes again";
export const STOP_NOTES_LABEL = "Stop writing notes";
export const COPY_PROMPT_LABEL = "Copy prompt";
export const NOTES_OFF_ITEM_LABEL = "Turn notes off for this meeting";
export const NOTES_ON_ITEM_LABEL = "Turn notes on";
export const COPY_TRANSCRIPT_LABEL = "Copy transcript";
export const COPY_NOTES_LABEL = "Copy notes";
export const RENAME_ITEM_LABEL = "Rename…";
export const DELETE_ITEM_LABEL = "Delete…";

/** The ⋯ button's name: says which meeting, since every row has one. */
export function moreActionsLabel(title: string): string {
  return `More actions for ${title}`;
}

/**
 * The menu for `meeting`, in groups: getting to it, its notes, copying what
 * is in it, and changing it. The meeting being recorded right now offers
 * only Open and Show in Finder: everything else either writes into the
 * folder the recorder is writing, or reads a transcript that is not done.
 */
export function meetingMenuItems(
  meeting: MeetingSummary,
  { isRecording, harnessIsNone, runStatus }: MeetingMenuContext,
): MeetingMenuEntry[][] {
  const reach: MeetingMenuEntry[] = [
    { action: "open", label: OPEN_LABEL },
    { action: "reveal", label: SHOW_IN_FILE_MANAGER_LABEL },
  ];
  if (isRecording || meeting.recordingState === "recording") return [reach];
  reach.push({ action: "copy-path", label: COPY_PATH_LABEL });

  const hasLines = meeting.lineCount > 0;

  const notes: MeetingMenuEntry[] = [];
  if (runStatus?.state === "running") {
    notes.push({ action: "stop-notes", label: STOP_NOTES_LABEL });
  } else if (!meeting.notesOff && hasLines && harnessIsNone !== null) {
    // The same rule the meeting page uses for its Copy prompt button.
    if (showsCopyPrompt({ harnessIsNone })) {
      notes.push({ action: "copy-prompt", label: COPY_PROMPT_LABEL });
    } else {
      notes.push({
        action: "write-notes",
        label: meeting.hasAnalysis ? WRITE_NOTES_AGAIN_LABEL : WRITE_NOTES_LABEL,
      });
    }
  }
  notes.push(
    meeting.notesOff
      ? { action: "notes-on", label: NOTES_ON_ITEM_LABEL }
      : { action: "notes-off", label: NOTES_OFF_ITEM_LABEL },
  );

  const copy: MeetingMenuEntry[] = [];
  if (hasLines) copy.push({ action: "copy-transcript", label: COPY_TRANSCRIPT_LABEL });
  if (meeting.hasNotes) copy.push({ action: "copy-notes", label: COPY_NOTES_LABEL });

  const change: MeetingMenuEntry[] = [
    { action: "rename", label: RENAME_ITEM_LABEL },
    { action: "delete", label: DELETE_ITEM_LABEL, danger: true },
  ];

  return [reach, notes, copy, change].filter((group) => group.length > 0);
}

/** A transcript as `transcript.md` writes it (SPEC §3.4), one line each. */
export function transcriptText(lines: readonly TranscriptLine[]): string {
  return lines.map((line) => `[${line.time}] ${line.speaker}: ${line.text}`).join("\n");
}

/** The Delete confirmation: the question, and what happens. */
export function deleteQuestion(
  title: string,
  keepsTickets: boolean,
): { title: string; body: string } {
  const trash = osText("trash");
  return {
    title: `Delete “${title}”?`,
    body: `Its transcript, notes and audio will be moved to the ${trash}.${
      keepsTickets ? " Tickets made from it stay in Tickets." : ""
    }`,
  };
}

/** What the window's dialogs say when an action did not work. */
const FAILED: Record<MeetingAction, string> = {
  open: "Could not open the meeting",
  reveal: `Could not show the folder in ${osText("fileManager")}`,
  "copy-path": "Could not copy the folder path",
  "write-notes": "Could not start writing notes",
  "stop-notes": "Could not stop writing notes",
  "copy-prompt": "Could not copy the prompt",
  "notes-off": "Could not turn notes off",
  "notes-on": "Could not turn notes on",
  "copy-transcript": "Could not copy the transcript",
  "copy-notes": "Could not copy the notes",
  rename: "Could not rename the meeting",
  delete: "Could not delete the meeting",
};

/**
 * The menu for one meeting row, acting. `onRename` opens the row's own title
 * field; the row owns it, since that is where the user is looking.
 */
export function useMeetingMenu(
  meeting: MeetingSummary,
  { isRecording, onRename }: { isRecording: boolean; onRename: () => void },
): { groups: MenuGroups; onOpenChange: (open: boolean) => void } {
  const navigate = useNavigate();
  const location = useLocation();
  // Read when Delete finishes, not when it was picked: the dialog may have
  // been up while the user moved on.
  const path = useRef(location.pathname);
  path.current = location.pathname;

  const [harnessIsNone, setHarnessIsNone] = useState<boolean | null>(null);
  const [runStatus, setRunStatus] = useState<NotesRunState | null>(null);
  const live = useRef(true);
  useEffect(() => {
    live.current = true;
    return () => {
      live.current = false;
    };
  }, []);

  const id = meeting.id;

  const onOpenChange = useCallback(
    (open: boolean) => {
      if (!open) return;
      // As the meeting page does: a failed answer counts as "an agent is set
      // up", and an unknown run as none.
      copyPromptFallback().then(
        (answer) => live.current && setHarnessIsNone(answer),
        () => live.current && setHarnessIsNone(false),
      );
      notesRunStatus(id).then(
        (answer) => live.current && setRunStatus(answer.state),
        () => live.current && setRunStatus(null),
      );
    },
    [id],
  );

  const reload = useCallback(() => useAppStore.getState().loadMeetings({ silent: true }), []);

  const remove = useCallback(async () => {
    // Only shared tickets stay behind; the meeting's own go with its folder.
    let keepsTickets = false;
    try {
      keepsTickets = (await listTickets()).some((ticket) => ticket.meeting === id);
    } catch {
      // The question still reads right without the extra sentence.
    }
    const question = deleteQuestion(meeting.title, keepsTickets);
    const confirmed = await ask(question.body, {
      title: question.title,
      kind: "warning",
      okLabel: "Delete",
      cancelLabel: "Cancel",
    });
    if (!confirmed) return;
    await deleteMeeting(id);
    await reload();
    if (path.current === meetingPath(id)) navigate(MEETINGS);
  }, [id, meeting.title, navigate, reload]);

  const perform = useCallback(
    async (action: MeetingAction) => {
      switch (action) {
        case "open":
          navigate(meetingPath(id));
          return;
        case "reveal":
          return revealMeeting(id);
        case "copy-path":
          return copyText((await readMeeting(id)).path);
        case "write-notes":
          await startNotesRun(id);
          return;
        case "stop-notes":
          await cancelNotesRun(id);
          return;
        case "copy-prompt":
          return copyText(await wrapUpPrompt(id));
        case "notes-off":
        case "notes-on":
          await setMeetingNotes(id, action === "notes-on");
          return reload();
        case "copy-transcript":
          return copyText(transcriptText((await readMeeting(id)).lines));
        case "copy-notes":
          return copyText((await readMeeting(id)).notes);
        case "rename":
          onRename();
          return;
        case "delete":
          return remove();
      }
    },
    [id, navigate, onRename, reload, remove],
  );

  const run = useCallback(
    (action: MeetingAction) => {
      perform(action).catch((thrown: unknown) => {
        void message(toUiError(thrown).message, { title: FAILED[action], kind: "error" });
      });
    },
    [perform],
  );

  const groups = meetingMenuItems(meeting, { isRecording, harnessIsNone, runStatus }).map((group) =>
    group.map((entry) => ({
      key: entry.action,
      label: entry.label,
      danger: entry.danger,
      onSelect: () => run(entry.action),
    })),
  );

  return { groups, onOpenChange };
}
