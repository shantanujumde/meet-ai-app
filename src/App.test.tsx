import { render, screen } from "@testing-library/react";
import { expect, test } from "vitest";
import { App } from "./App";

// Smallest thing that proves the React + Vitest + Testing Library wiring is
// real: the shell mounts and renders its copy.
test("the app shell renders", () => {
  render(<App />);
  expect(screen.getByRole("heading", { name: "meet-ai" })).toBeInTheDocument();
});
