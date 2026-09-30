# TUR-1 — Paperclip onboarding

[← Back to the handoff](../HANDOFF.md)

| | |
|---|---|
| Status | **done** |
| Priority | medium |
| Owner | Alen |
| Created | 2026-09-27 07:05 UTC by You (board) |
| Completed | 2026-09-27 07:19 UTC |

## Sub-tasks

- [TUR-2](TUR-2.md) **done** — Repo scaffold — Tauri 2 + React 19 + Rust workspace
- [TUR-3](TUR-3.md) **done** — Phase 0a — macOS audio-capture permission spike (signed bundle)
- [TUR-4](TUR-4.md) **done** — Phase 0 — meet-rec dual-track recorder
- [TUR-5](TUR-5.md) **blocked** — Phase 1 — transcription: Apple built-in + Whisper fallback
- [TUR-6](TUR-6.md) **blocked** — Phase 2 — the app window
- [TUR-7](TUR-7.md) **blocked** — MVP verification — fixtures plus a real 45-minute call

## Description

This is the user's first task in Paperclip. Your job is to understand what they want and propose a path forward. A greeting and an opening question card were already posted for you; the card offered two choices: "Interview me and propose a plan and an agent team to execute it." (option `interview`) or "I have a task in mind" (option `task`, with a text field). You are running because the user answered that card (the answer is in your wake payload) or wrote a message instead of answering. Don't re-introduce yourself and don't post the opening card again.

Work in this order.

1. Take the path the user picked.

   - `interview` → reply with ONE ask_user_questions card of 3–4 questions that pin down what the organization does, what they want to achieve first, any constraints (time, budget, tools), and what "done" looks like. Don't guess; ask. Don't post anything else before the card. The answers lead to the plan-and-team path in step 2.

   - `task` → the text they typed is the task. If it is clear enough to propose on, go straight to step 2. If not, reply with ONE ask_user_questions card of 2–3 questions specific to their message (concrete goal, constraints, what "done" looks like), then go to step 2.

   - If they wrote a message instead of answering the card, treat the message as the `task` path.

2. Propose, don't decide. From what you now know, pick the path:

   - They want a plan and/or a team → write a short `plan` document (goal, approach, team as one line per hire: name, role, responsibility; follow-up tasks). Then post ONE request_checkbox_confirmation targeting the plan, each hire and follow-up task as its own option, checked by default, each with a stable id. Keep the card's message to a line or two and point to the Plan in the right sidebar.

   - They want one thing done now → post ONE request_confirmation that says, in a few lines, what you will do and what they will get (and by when, if you can say). No plan document.

3. Wait. Do nothing until a card is accepted. If they ask for changes, revise and re-confirm. Hiring or creating tasks without an accepted card is never allowed on this task.

4. On acceptance, execute only what was approved: hire the checked agents, create and delegate the checked tasks, or do the single task yourself and post the result as a document on this task.

## Document: meet-ai MVP — plan

_Key `plan`, last updated 2026-09-27 07:11 UTC._

### meet-ai MVP — plan

#### Where we are

The thinking is done. `PROBLEM.md`, `SPEC.md` (v2, locked decisions L1–L18), `SETUP.md` (pinned versions) and `FINDINGS.md` already exist and agree with each other. What does not exist yet is code — the repo holds documents, a `bin/` folder and a design-system draft, nothing that runs.

So this plan is not a re-design. It is an execution plan against the spec that is already written.

#### Goal

A working MVP on your own Mac that does the four things you named:

1. Records a meeting with nothing joining the call — your mic on one track, system audio on the other.
2. Uses the Mac's built-in speech model (Apple `SpeechTranscriber`, macOS 26+) when it is there.
3. Downloads an open-source model (Whisper) when it is not, and works offline either way.
4. Turns the audio into a live transcript, labelled `You` / `Others`, saved as plain markdown under `~/Meetings/`.

Out of scope for this MVP, by your own ordering: release, marketing, and the $1k MRR goal. Those come after the thing works. No hires for them now.

#### Approach

Follow the phase order in `SPEC.md` §5, because each phase has a hard exit gate and stacking work on a broken audio layer is the one way this project dies.

**Phase 0a — the permission spike (2 days, do this first).**
A signed Swift helper inside a signed app bundle, asking for audio-capture permission. The gate: does the prompt appear, does it say *meet-ai* and not the helper, and does real non-silent audio actually arrive. Prompt-but-silence is a fail. This one answer decides whether capture lives in a Swift sidecar or in Rust, and it is the project's biggest unknown. Nothing else starts until it is answered.

**Phase 0 — capture.** `meet-rec` writes `mic.wav` + `system.wav` + `segments.json`. Gate: a real 45-minute call, both files intact, under 200ms drift, survives an AirPods swap mid-call and a force-quit.

**Phase 1 — transcribe.** One `SttEngine` interface with two engines behind it: Apple's built-in on macOS 26+, `whisper-rs` with lazy model download everywhere else. Gate: same call reads accurately on both engines, speakers split correctly, and 30 seconds of silence produces **zero** lines.

**Phase 2 — a window you'd actually use.** Tauri + React: meeting list, live transcript, notes pane, ⌘⇧R, and the permission-denied path. Gate: you pick it over Notes for a real meeting.

Phases 3–6 from the spec (search index, agent hand-off, calendar, polish) stay on the roadmap and are not in this MVP.

##### An honest note on "instant"

Phases 0a through 2 are roughly five weeks of work in the spec's own estimate, and Phase 0 is the part nobody can shortcut — it is Core Audio process taps and clock drift, not UI. Agents working in parallel compress the app-shell and transcription work, not the capture work. You will have a real answer on the riskiest question in about two days, and a recording that works before you have a window to look at it in.

#### Team

Four hires, each one owning a phase gate end to end.

- **Rune** — systems engineer (Rust + Swift, macOS audio). Owns Phase 0a and Phase 0: the permission spike, the process tap, mic capture, device-change handling, crash-safe WAV writes.
- **Vox** — speech engineer. Owns Phase 1: the Apple `SpeechTranscriber` sidecar, the `whisper-rs` fallback, VAD gating, model download with resume and checksum, `transcript.md` output.
- **Nia** — app engineer (Tauri, React, TypeScript). Owns Phase 2 and the repo scaffold: workspace setup per `SETUP.md`, the app shell, live transcript view, notes pane, onboarding and permission-denied flows.
- **Tess** — QA engineer. Owns the gates: audio fixtures, the drift check, the silence-hallucination guard, the signed-bundle test loop, and the end-to-end MVP verification on a real call.

#### Follow-up tasks

Created and delegated on approval, in this order:

1. **Repo scaffold** — Tauri 2 + React 19 + Rust workspace, pinned per `SETUP.md`, `just check` green on an empty project. (Nia)
2. **Phase 0a — TCC permission spike** — signed bundle, does audio actually flow. Blocks everything after it. (Rune)
3. **Phase 0 — capture CLI** — dual-track recording, drift, device swap, crash safety. (Rune)
4. **Phase 1 — transcription** — both engines, model download, silence guard. (Vox)
5. **Phase 2 — app shell** — the window, live transcript, notes, permissions onboarding. (Nia)
6. **MVP verification** — fixtures suite plus a real 45-minute call, end to end. (Tess)


## Questions you answered

- **Q:** What would you like to do?
  **A:** Interview me and propose a plan and an agent team to execute it.
- **Q:** What does your organization do, and what is this Paperclip company for?
  **A:** "1. develope this product; 2. we release the product once it is usabe enough; 3. market to reach users expand; 4. earn atleast 1000$MRR"
- **Q:** What is the first thing you want this team to get done?
  **A:** "create a mvp working; all the core features should be working; capturing audio, using macs builtin models, downloading open source models, transcribing"
- **Q:** What limits should I design around? Pick all that apply.
  **A:** "mvp instant, then we will progess on that"
- **Q:** What does "done" look like for that first goal?
  **A:** "as described in previous question"
- **Q:** Approve the MVP plan, the team, and the first tasks?
  **A:** Yes

## Comments (4)

### Alen · 2026-09-27 07:19 UTC

> ## Approved plan executed — team hired, work started
>
> Everything you left checked is live. No board approval was required, so all four agents went straight to `idle` and Paperclip started the two unblocked tasks on its own.
>
> ### The team
>
> | Agent | Role | Owns |
> |---|---|---|
> | [Rune](/TUR/agents/rune) | Systems Engineer (macOS audio) | Process taps, permission, dual-track capture, drift, crash safety |
> | [Vox](/TUR/agents/vox) | Speech Engineer | Apple `SpeechTranscriber`, `whisper-rs` fallback, model download, silence gating |
> | [Nia](/TUR/agents/nia) | Application Engineer (Tauri/React) | Repo scaffold, app window, onboarding and permission-denied flows |
> | [Tess](/TUR/agents/tess) | QA Engineer | Fixtures, drift measurement, silence guard, signed-bundle tests, MVP verification |
>
> All four work in `/Users/shantanujumde/apps/meet-ai`, report to me, and were briefed on `SPEC.md`, `SETUP.md`, `PROBLEM.md` and `FINDINGS.md` as locked source-of-truth. Their instructions tell them not to re-design what you already decided.
>
> ### The work
>
> | Task | Owner | Status | Waiting on |
> |---|---|---|---|
> | [TUR-2](TUR-2.md) Repo scaffold | Nia | **running now** | — |
> | [TUR-3](TUR-3.md) Permission spike | Rune | **running now** | — |
> | [TUR-4](TUR-4.md) meet-rec recorder | Rune | blocked | TUR-3 |
> | [TUR-5](TUR-5.md) Transcription | Vox | blocked | TUR-2 |
> | [TUR-6](TUR-6.md) App window | Nia | blocked | TUR-2, TUR-5 |
> | [TUR-7](TUR-7.md) MVP verification | Tess | blocked | TUR-4, TUR-5, TUR-6 |
>
> Dependencies are real blocker edges, not notes — each task wakes its owner automatically the moment its blockers finish. Nothing needs a nudge from you.
>
> ### One change I made to the ordering
>
> The plan had transcription sitting behind the recorder. I put [TUR-5](TUR-5.md) behind the scaffold only, so Vox can build the engine interface, model download and silence gating against synthetic audio while Rune is still deep in Core Audio. The gate is unchanged: TUR-5 cannot be marked done on synthetic audio — Vox is instructed to block on [TUR-4](TUR-4.md) and wait for real recordings before claiming the phase. It buys parallel time without softening the bar.
>
> ### What to expect first
>
> [TUR-3](TUR-3.md) is the one that matters. In roughly two days you should have a straight answer to: does the permission prompt appear in a signed bundle, does it say *meet-ai*, and does real non-silent audio arrive. Rune is explicitly told that a prompt followed by silent buffers is a failure, not a pass, and to report measured audio levels rather than API return codes.
>
> Onboarding is finished, so I am closing this one. The six tasks are children of it, so I get woken when they all land — and I will check in before that if anything goes sideways.

### Alen · 2026-09-27 07:12 UTC

> Plan is in the **Plan** document in the right sidebar, and the card above has every piece as its own checkbox — all checked. Uncheck what you do not want and approve; I hire and create only what stays checked.
>
> One thing I want to be straight about: the spec you already wrote is good and I am not re-doing it. But "MVP instant" and macOS audio capture do not fit together. The process-tap work is the hard part and it is real engineering, not UI. What you will get fast is the answer to the riskiest question — does the permission prompt work in a signed bundle and does real audio actually flow — in about two days.

### Alen · 2026-09-27 07:06 UTC

> Posted four questions above. Answer them and I will come back with a written plan and a proposed team — one line per hire — for you to approve or edit.

### Alen · 2026-09-27 07:05 UTC

> Welcome to Paperclip! I'm Alen, your first agent teammate. Pick how you'd like to start and I'll take it from there.
