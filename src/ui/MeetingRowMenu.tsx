/**
 * A meeting row's menu and its in-place rename (TUR-116), shared by the
 * sidebar's Recent meetings and the Meetings list.
 *
 * {@link MeetingRowMenu} wraps a row in the ⋯ button and the right-click
 * menu, both drawing `meetingMenuItems`. Rename… swaps the row's title for
 * {@link RenameField}, the way Finder renames in its list: Enter saves,
 * Escape keeps the old name, clicking away saves. It saves through the same
 * command as the meeting page's title, so the name is the user's from then on.
 */

import { message } from "@tauri-apps/plugin-dialog";
import { type ReactNode, useEffect, useRef, useState } from "react";
import { renameMeeting } from "@/ipc/client";
import type { MeetingSummary } from "@/ipc/types";
import { toUiError } from "@/ipc/types";
import { cn } from "@/lib/cn";
import { useAppStore } from "@/state/app";
import { MAX_TITLE_CHARS } from "./MeetingTitle";
import { moreActionsLabel, useMeetingMenu } from "./meetingMenu";
import { MenuRow } from "./menu";

export function MeetingRowMenu({
  meeting,
  isRecording,
  selected,
  renaming,
  onRename,
  className,
  buttonClassName,
  children,
}: {
  meeting: MeetingSummary;
  isRecording: boolean;
  /** The open meeting's row keeps its ⋯ button showing. */
  selected: boolean;
  /** The title field is open: no menu, so the field keeps the system's text menu. */
  renaming: boolean;
  /** Rename… was picked: open the row's title field. */
  onRename: () => void;
  className?: string;
  buttonClassName?: string;
  children: ReactNode;
}) {
  const { groups, onOpenChange } = useMeetingMenu(meeting, { isRecording, onRename });
  return (
    <MenuRow
      label={moreActionsLabel(meeting.title)}
      groups={groups}
      onOpenChange={onOpenChange}
      shown={selected}
      disabled={renaming}
      className={className}
      buttonClassName={buttonClassName}
    >
      {children}
    </MenuRow>
  );
}

/**
 * The row's title as a text field. `onDone` runs as soon as the field
 * closes, saved or not; `refocus` is true when it closed from the keyboard,
 * so the row can take focus back.
 */
export function RenameField({
  meeting,
  onDone,
  className,
}: {
  meeting: MeetingSummary;
  onDone: (refocus: boolean) => void;
  className?: string;
}) {
  const [draft, setDraft] = useState(meeting.title);
  const field = useRef<HTMLInputElement>(null);
  // Set once saved or cancelled, so the blur that follows Enter or Escape
  // does not save a second time.
  const closed = useRef(false);

  useEffect(() => {
    field.current?.focus();
    field.current?.select();
  }, []);

  function close(refocus: boolean) {
    closed.current = true;
    onDone(refocus);
  }

  function save(next: string, refocus: boolean) {
    if (closed.current) return;
    close(refocus);
    const trimmed = next.trim();
    if (trimmed === "" || trimmed === meeting.title) return;
    renameMeeting(meeting.id, trimmed)
      .then(() => useAppStore.getState().loadMeetings({ silent: true }))
      .catch((thrown: unknown) => {
        void message(toUiError(thrown).message, {
          title: "Could not rename the meeting",
          kind: "error",
        });
      });
  }

  return (
    <input
      ref={field}
      aria-label="Meeting title"
      className={cn(
        "w-full min-w-0 rounded-control border-[0.5px] border-separator bg-glass-sunken px-2",
        "text-body text-fg-primary",
        className,
      )}
      value={draft}
      maxLength={MAX_TITLE_CHARS}
      onChange={(event) => setDraft(event.target.value)}
      onBlur={(event) => save(event.target.value, false)}
      onKeyDown={(event) => {
        // Enter also confirms a word in an input method; that must not save
        // a half-typed title.
        if (event.nativeEvent.isComposing) return;
        if (event.key === "Enter") {
          event.preventDefault();
          save(event.currentTarget.value, true);
        } else if (event.key === "Escape") {
          event.preventDefault();
          close(true);
        }
      }}
    />
  );
}
