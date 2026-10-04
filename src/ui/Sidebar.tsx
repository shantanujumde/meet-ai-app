/**
 * The sidebar (TUR-102): a find box, the app's pages under small grey
 * headings that fold shut, and the meeting list.
 *
 * On macOS it is see-through glass over the native window material, a tone
 * off the content (`--sidebar-fill`); solid with glass off, Reduce
 * Transparency, Increase Contrast, and on Windows and Linux. It adds no blur
 * of its own: stacking a `backdrop-filter` on top of the window material is
 * the "glass on glass" the design system rules out.
 *
 * Every row is a small accent line icon and a name. The current page is a
 * solid accent pill with white words and icon, marked by `aria-current` as
 * well as by colour.
 *
 * A meeting with notes switched off (TUR-12) says "Notes off" in words. It
 * takes the line count's place, the way Interrupted does, and sits beside the
 * recording dot or Interrupted rather than replacing them: those say what
 * happened to the recording, this says what will happen to the transcript.
 */

import { ChevronDown, FileText, LayoutList, Search, Settings, Ticket } from "lucide-react";
import { type ReactNode, useId, useState } from "react";
import { NavLink, useNavigate } from "react-router";
import type { MeetingList, MeetingSummary, RecordingStatus } from "@/ipc/types";
import { cn } from "@/lib/cn";
import {
  formatLineCount,
  formatRelativeDate,
  INTERRUPTED_LABEL,
  NOTES_OFF_LABEL,
} from "@/lib/format";
import { MEETINGS, meetingPath, SETTINGS, TICKETS } from "@/lib/routes";
import { Icon, type LucideIcon } from "./icons";
import { Checking } from "./states";

/** A sidebar row's shape, shared by the page links and the meeting rows. */
const rowClass = (current: boolean) =>
  cn(
    "flex w-full min-h-(--sidebar-row-h) items-center gap-4 rounded-(--sidebar-row-radius) px-4 py-2 text-left",
    "cursor-default border-0 font-[inherit] text-body",
    "[transition:background-color_var(--dur-fast)_var(--ease-out)]",
    current
      ? "bg-(--sidebar-row-active) text-on-accent"
      : "bg-transparent text-fg-primary hover:bg-(--sidebar-row-hover)",
  );

export function Sidebar({
  list,
  loading,
  recording,
  selectedId,
}: {
  list: MeetingList | null;
  loading: boolean;
  recording: RecordingStatus;
  selectedId: string | undefined;
}) {
  const navigate = useNavigate();
  const [find, setFind] = useState("");
  const meetings = list?.meetings ?? [];
  const needle = find.trim().toLowerCase();
  const shown = needle
    ? meetings.filter((meeting) => meeting.title.toLowerCase().includes(needle))
    : meetings;

  return (
    <nav
      className={cn(
        "[grid-area:sidebar] flex min-h-0 flex-col gap-6 p-(--sidebar-pad)",
        "border-r-[0.5px] border-separator bg-(--sidebar-fill)",
      )}
      aria-label="Sidebar"
    >
      <FindBox value={find} onChange={setFind} />

      <SidebarGroup title="Library">
        <PageLink to={MEETINGS} icon={LayoutList} end>
          Meetings
        </PageLink>
        <PageLink to={TICKETS} icon={Ticket}>
          Tickets
        </PageLink>
      </SidebarGroup>

      <SidebarGroup
        title="Recent meetings"
        count={!loading && meetings.length > 0 ? shown.length : undefined}
        grow
      >
        {loading ? (
          <li className="px-4 py-3">
            <Checking label="Reading your meetings folder…" />
          </li>
        ) : meetings.length === 0 ? (
          // The sidebar's empty state is one quiet line. The full explanation
          // belongs on the content pane beside it, not shouted twice.
          <li className="px-4 py-3 text-caption1 leading-normal text-fg-tertiary">
            Nothing recorded yet.
          </li>
        ) : shown.length === 0 ? (
          <li className="px-4 py-3 text-caption1 leading-normal text-fg-tertiary">
            No meeting called that.
          </li>
        ) : (
          shown.map((meeting) => (
            <li key={meeting.id}>
              <MeetingRow
                meeting={meeting}
                selected={meeting.id === selectedId}
                isRecording={recording.meetingId === meeting.id}
                onOpen={() => navigate(meetingPath(meeting.id))}
              />
            </li>
          ))
        )}
      </SidebarGroup>

      <ul className="m-0 flex list-none flex-col gap-1 border-t-[0.5px] border-separator p-0 pt-5">
        <PageLink to={SETTINGS} icon={Settings}>
          Settings
        </PageLink>
      </ul>
    </nav>
  );
}

/** The rounded find box at the top: narrows the meeting list by title. */
function FindBox({ value, onChange }: { value: string; onChange: (value: string) => void }) {
  return (
    <span className="relative flex items-center">
      <Icon icon={Search} className="pointer-events-none absolute left-4 text-fg-secondary" />
      <input
        type="search"
        aria-label="Find a meeting"
        placeholder="Find a meeting"
        autoComplete="off"
        spellCheck={false}
        value={value}
        onChange={(event) => onChange(event.target.value)}
        className={cn(
          "h-(--control-h-large) w-full rounded-capsule border-[0.5px] border-separator bg-control",
          "ps-9 pe-4 text-body text-fg-primary placeholder:text-fg-tertiary",
          "contrast-more:border-separator-strong",
        )}
      />
    </span>
  );
}

/**
 * A small grey heading over a list of rows, that folds the list shut. The
 * heading is a button with `aria-expanded`, so the fold is reachable from the
 * keyboard and VoiceOver says whether it is open.
 */
function SidebarGroup({
  title,
  count,
  grow = false,
  children,
}: {
  title: string;
  count?: number;
  /** Takes the room that is left and scrolls inside it: the meeting list. */
  grow?: boolean;
  children: ReactNode;
}) {
  const [open, setOpen] = useState(true);
  const listId = useId();
  return (
    <div className={cn("flex min-h-0 flex-col gap-2", grow && open ? "flex-1" : "")}>
      <button
        type="button"
        aria-expanded={open}
        aria-controls={listId}
        onClick={() => setOpen((was) => !was)}
        className={cn(
          "flex cursor-default items-center gap-2 border-0 bg-transparent px-2 py-1 text-left",
          "text-caption1 font-semibold text-fg-secondary",
        )}
      >
        <Icon
          icon={ChevronDown}
          className={cn(
            "size-[12px] [transition:rotate_var(--dur-fast)_var(--ease-out)] motion-reduce:transition-none",
            open ? "" : "-rotate-90",
          )}
        />
        <span className="flex-1">{title}</span>
        {count !== undefined ? <span className="font-normal tabular-nums">{count}</span> : null}
      </button>
      {open ? (
        <ul
          id={listId}
          className={cn(
            "m-0 flex list-none flex-col gap-1 p-0",
            // The padding keeps the first and last rows clear of the fade
            // until the list is scrolled.
            grow
              ? "min-h-0 flex-1 overflow-y-auto py-(--sidebar-list-fade) [mask-image:linear-gradient(to_bottom,transparent,black_var(--sidebar-list-fade),black_calc(100%_-_var(--sidebar-list-fade)),transparent)]"
              : "",
          )}
        >
          {children}
        </ul>
      ) : null}
    </div>
  );
}

/** One of the app's pages: a line icon in the accent, and its name. */
function PageLink({
  to,
  icon,
  end = false,
  children,
}: {
  to: string;
  icon: LucideIcon;
  end?: boolean;
  children: ReactNode;
}) {
  return (
    <li>
      <NavLink to={to} end={end} className={({ isActive }) => rowClass(isActive)}>
        {({ isActive }) => (
          <>
            <Icon icon={icon} className={isActive ? "text-on-accent" : "text-accent-text"} />
            <span className="truncate font-medium">{children}</span>
          </>
        )}
      </NavLink>
    </li>
  );
}

function MeetingRow({
  meeting,
  selected,
  isRecording,
  onOpen,
}: {
  meeting: MeetingSummary;
  selected: boolean;
  isRecording: boolean;
  onOpen: () => void;
}) {
  // On the accent pill every word is white: the status colours would not
  // read on it, and the words already say the state.
  const tone = (colour: string) => (selected ? "text-on-accent" : colour);
  // A recording that caught no speech steps back, so the meetings with
  // something in them stand out. Quieter text tokens, not opacity, so the
  // contrast check still covers it.
  const empty = meeting.lineCount === 0 && !isRecording;
  const date = formatRelativeDate(meeting.date);
  const status = isRecording
    ? "● Recording"
    : meeting.recordingState === "interrupted"
      ? INTERRUPTED_LABEL
      : meeting.notesOff
        ? null
        : formatLineCount(meeting.lineCount);
  const meta = [date, meeting.time, status, meeting.notesOff ? NOTES_OFF_LABEL : null]
    .filter(Boolean)
    .join(" · ");
  return (
    <button
      type="button"
      className={cn(rowClass(selected), "items-start")}
      aria-current={selected}
      onClick={onOpen}
    >
      <Icon
        icon={FileText}
        className={cn(
          "mt-[2px]",
          selected ? "text-on-accent" : empty ? "text-fg-tertiary" : "text-accent-text",
        )}
      />
      <span className="flex min-w-0 flex-1 flex-col gap-1">
        <span className={cn("truncate font-medium", empty && tone("text-fg-secondary"))}>
          {meeting.title}
        </span>
        {/* One line, so every row is the same height: it clips at the end
            when the sidebar is narrow, and the tooltip has all of it. */}
        <span
          title={meta}
          className={cn(
            "flex gap-3 overflow-hidden whitespace-nowrap text-footnote tabular-nums",
            tone(empty ? "text-fg-tertiary" : "text-fg-secondary"),
          )}
        >
          <span>{date}</span>
          {meeting.time ? <span>{meeting.time}</span> : null}
          {/* A recording meeting shows the dot, not a red title — colour
              alone is not a state signal. */}
          {isRecording ? (
            <span className={tone("text-recording")}>● Recording</span>
          ) : meeting.recordingState === "interrupted" ? (
            // Takes the line count's place: the sidebar is too narrow for
            // both, and the meeting itself says how much was kept. The
            // colour only matches the list page's warning pill.
            <span className={tone("text-warning")}>{INTERRUPTED_LABEL}</span>
          ) : meeting.notesOff ? null : (
            <span>{formatLineCount(meeting.lineCount)}</span>
          )}
          {meeting.notesOff ? <span>{NOTES_OFF_LABEL}</span> : null}
        </span>
      </span>
    </button>
  );
}
