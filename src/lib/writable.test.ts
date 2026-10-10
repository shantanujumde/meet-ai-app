import { expect, test } from "vitest";
import {
  DEFAULT_CALENDAR_SOURCES,
  DEFAULT_TODAYS_MEETINGS,
  type meet_ai_lib_calendar_sources_CalendarSources,
} from "@/ipc/bindings";
import { writable } from "./writable";

test("a copy, so editing it never edits Rust's default (TUR-173)", () => {
  const sources: meet_ai_lib_calendar_sources_CalendarSources = writable(DEFAULT_CALENDAR_SOURCES);
  sources.configured.push("google");

  expect(sources).not.toBe(DEFAULT_CALENDAR_SOURCES);
  expect(DEFAULT_CALENDAR_SOURCES.configured).toEqual([]);
  expect(writable(DEFAULT_TODAYS_MEETINGS)).toEqual(DEFAULT_TODAYS_MEETINGS);
});
