import { describe, expect, test } from "vitest";
import { copyFor, detailsFor, permissionDeniedCopy } from "./errors";
import type { UiError } from "./types";

const error = (
  domain: UiError["domain"],
  kind: string,
  message = "the original sentence",
): UiError => ({
  domain,
  kind,
  message,
});

/**
 * These lock the mapping agreed with Vox on TUR-6. It is a cross-agent
 * contract, so a well-meaning copy edit that quietly drops a button should
 * fail a test rather than ship.
 */
describe("the agreed error copy mapping", () => {
  test("EngineUnavailable offers the model download", () => {
    const copy = copyFor(error("stt", "engine-unavailable"));
    expect(copy.headline).toBe("This Mac will use the downloadable speech model");
    expect(copy.actionLabel).toBe("Download");
    expect(copy.remedy).toEqual({ action: "download-model" });
  });

  test("a failed sidecar or engine reassures, then offers Copy details", () => {
    for (const kind of ["sidecar", "engine"]) {
      const copy = copyFor(error("stt", kind));
      expect(copy.headline).toBe("Transcription stopped. Your recording is still safe.");
      expect(copy.actionLabel).toBe("Copy details");
      expect(copy.remedy).toEqual({ action: "copy-details" });
    }
  });

  test("a failed download offers Try again", () => {
    const copy = copyFor(error("model", "download"));
    expect(copy.actionLabel).toBe("Try again");
    expect(copy.remedy).toEqual({ action: "retry" });
    expect(copy.security).toBeFalsy();
  });

  test("a checksum mismatch is security-flavoured and starts over", () => {
    const copy = copyFor(error("model", "checksum"));
    expect(copy.actionLabel).toBe("Download again from scratch");
    expect(copy.remedy).toEqual({ action: "redownload" });
    expect(copy.security).toBe(true);
    // "Tampered" is the word that makes this different from a flaky network.
    expect(copy.body).toMatch(/tampered/i);
  });
});

describe("how the mapping discriminates", () => {
  test("the two enums do not collide on a shared variant name", () => {
    // Both enums could plausibly grow a variant called `download` or `engine`.
    // Keying on domain + kind is what keeps them apart.
    expect(copyFor(error("model", "download"))).not.toEqual(copyFor(error("stt", "download")));
  });

  test("an unmapped error is never a dead end", () => {
    const copy = copyFor(error("stt", "a-variant-nobody-has-written-yet"));
    expect(copy.headline).not.toHaveLength(0);
    expect(copy.actionLabel).toBe("Copy details");
  });

  test("the message is never what decides the copy", () => {
    // Same kind, wildly different messages — the screen must not change.
    const a = copyFor(error("model", "checksum", "expected aaa, got bbb"));
    const b = copyFor(error("model", "checksum", "totally different wording"));
    expect(a).toEqual(b);
  });
});

test("copied details carry the machine-readable tag as well as the sentence", () => {
  const details = detailsFor(error("model", "checksum", "expected aaa, got bbb"));
  expect(details).toContain("model/checksum");
  expect(details).toContain("expected aaa, got bbb");
});

describe("permission-denied per OS (TUR-51)", () => {
  test("keeps the macOS copy and names Windows Settings on Windows", () => {
    expect(permissionDeniedCopy("macos").body).toContain("System Settings");
    const windows = permissionDeniedCopy("windows");
    expect(windows.body).toContain("Settings, under Privacy & security → Microphone");
    expect(windows.body).not.toContain("System Settings");
    expect(windows.remedy).toEqual({ action: "open-settings" });
    expect(permissionDeniedCopy("linux").remedy).toEqual({ action: "none" });
  });
});

describe("agent and config errors have their own wording (TUR-118)", () => {
  const kinds = [
    "agent-not-installed",
    "agent-not-signed-in",
    "agent-timed-out",
    "agent-cancelled",
    "agent-failed",
    "agent-bad-reply",
    "agent-invalid-json",
    "agent-schema-mismatch",
    "agent-could-not-start",
    "unknown-harness",
  ];
  const generic = copyFor(error("app", "a-kind-nobody-has-written-yet"));

  test.each(kinds)("%s is not the generic text", (kind) => {
    const copy = copyFor(error("app", kind));
    expect(copy.headline).not.toBe(generic.headline);
    expect(copy.body).not.toBe(generic.body);
    expect(`${copy.headline} ${copy.body}`).not.toContain("\u2014");
  });

  test("the three bad-reply kinds share one wording", () => {
    const a = copyFor(error("app", "agent-bad-reply"));
    expect(copyFor(error("app", "agent-invalid-json"))).toEqual(a);
    expect(copyFor(error("app", "agent-schema-mismatch"))).toEqual(a);
  });
});
