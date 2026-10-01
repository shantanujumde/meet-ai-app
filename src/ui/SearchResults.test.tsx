import { act, fireEvent, render, screen } from "@testing-library/react";
import { useState } from "react";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import type { SearchHit } from "@/ipc/types";
import { ipc } from "@/test/ipcMock";
import { SearchBox } from "./SearchBox";
import { SearchResults } from "./SearchResults";

vi.mock("@/ipc/client", async (importOriginal) =>
  (await import("@/test/ipcMock")).mockClient(await importOriginal()),
);

const { search } = ipc;

function hit(over: Partial<SearchHit> = {}): SearchHit {
  return {
    meetingId: "2026-09-27-standup",
    title: "Standup",
    date: "2026-09-27",
    snippet: "we should «ship» on Friday",
    timestamp: "00:12:03",
    ...over,
  };
}

function Harness({ onOpen }: { onOpen: (id: string) => void }) {
  const [query, setQuery] = useState("");
  return (
    <>
      <SearchBox value={query} onChange={setQuery} />
      {query.trim() ? <SearchResults query={query} onOpen={onOpen} /> : <p>normal list</p>}
    </>
  );
}

async function wait(ms: number) {
  await act(async () => {
    await vi.advanceTimersByTimeAsync(ms);
  });
}

async function type(text: string) {
  fireEvent.change(screen.getByRole("searchbox", { name: "Search meetings" }), {
    target: { value: text },
  });
  await wait(250);
}

describe("search", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  test("debounces, then shows title, date, marked snippet and timestamp", async () => {
    search.mockResolvedValue([hit()]);
    render(<Harness onOpen={() => {}} />);

    fireEvent.change(screen.getByRole("searchbox"), { target: { value: "sh" } });
    fireEvent.change(screen.getByRole("searchbox"), { target: { value: "ship" } });
    expect(search).not.toHaveBeenCalled();
    await wait(250);

    expect(search).toHaveBeenCalledTimes(1);
    expect(search).toHaveBeenCalledWith("ship");
    expect(screen.getByText("Standup")).toBeTruthy();
    expect(screen.getByText("ship").tagName).toBe("MARK");
    expect(screen.getByText(/00:12:03/)).toBeTruthy();
    expect(screen.queryByText("normal list")).toBeNull();
  });

  test("clicking a result opens that meeting", async () => {
    search.mockResolvedValue([hit()]);
    const onOpen = vi.fn();
    render(<Harness onOpen={onOpen} />);
    await type("ship");

    fireEvent.click(screen.getByRole("button", { name: /Standup/ }));
    expect(onOpen).toHaveBeenCalledWith("2026-09-27-standup");
  });

  test("clearing the box restores the normal list", async () => {
    search.mockResolvedValue([hit()]);
    render(<Harness onOpen={() => {}} />);
    await type("ship");
    await type("");
    expect(screen.getByText("normal list")).toBeTruthy();
  });

  test("no hits shows No matches", async () => {
    search.mockResolvedValue([]);
    render(<Harness onOpen={() => {}} />);
    await type("zzz");
    expect(screen.getByRole("heading", { name: "No matches" })).toBeTruthy();
  });

  test("a failed search shows the error", async () => {
    search.mockRejectedValue({ domain: "app", kind: "unexpected", message: "index is broken" });
    render(<Harness onOpen={() => {}} />);
    await type("ship");
    expect(screen.getByRole("alert")).toBeTruthy();
    expect(screen.getByText("index is broken")).toBeTruthy();
  });

  test("ignores a stale response that arrives after a newer one", async () => {
    let resolveOld: (hits: SearchHit[]) => void = () => {};
    search.mockImplementationOnce(() => new Promise((resolve) => (resolveOld = resolve)));
    search.mockResolvedValueOnce([hit({ title: "Newer" })]);
    render(<Harness onOpen={() => {}} />);

    await type("a");
    await type("ab");
    expect(screen.getByText("Newer")).toBeTruthy();

    await act(async () => resolveOld([hit({ title: "Older" })]));
    expect(screen.queryByText("Older")).toBeNull();
    expect(screen.getByText("Newer")).toBeTruthy();
  });
});
