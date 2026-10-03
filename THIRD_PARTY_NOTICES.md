# Third-party notices

meet-ai is licensed under Apache-2.0 (see [`LICENSE`](./LICENSE) and
[`NOTICE`](./NOTICE)). Some of its code is copied or adapted from other open
source projects. Their licences let us do that only if we keep their copyright
and licence notices, so this file lists every such source: the project, where
it lives, its licence, the commit we took it from, and which of our files hold
the copy. The licences of the libraries we depend on (crates, npm packages)
travel with those packages and are not repeated here.

Every copied or adapted file keeps the upstream licence header, if it had one,
and has this comment above the copied code, in the file's own comment syntax:

```
// Adapted from github.com/<owner>/<repo>/<path> @ <commit> (<SPDX>)
```

`<commit>` is the full commit hash, never a branch name: a project can change
its licence later, and the commit is what proves which licence our copy came
under. Rule R9 in `scripts/quality-rules.sh` fails a change that has such a
comment without a section below whose `URL:` line names that repository, or
whose licence is GPL, AGPL, LGPL or missing. Which licences we may copy from,
and how to add a section, is in [`CONTRIBUTING.md`](./CONTRIBUTING.md#code-from-other-projects).

Each source gets one section in this shape. For MIT, BSD, ISC and Zlib code the
full licence text goes under it, because those licences ask for it. For
Apache-2.0 code, name the licence and link it; our `LICENSE` file already
carries the full Apache-2.0 text, and any upstream `NOTICE` text goes into our
`NOTICE` file.

```
## <Project>

- URL: https://github.com/<owner>/<repo>
- Licence: <SPDX id>
- Copyright: <the upstream copyright line, as written>
- Commit: <full commit hash>
- Files:
  - `<our path>` from `<upstream path>`

<full licence text>
```

No code has been copied into meet-ai yet, so there are no sections above "To
confirm" so far.

## To confirm

Sources the project's documents name as a model for code we wrote, where it is
not known what, if anything, was taken, or under which licence. R9 does not
accept an entry here as a notice. Before copying from one of these, confirm the
licence and the commit, then give it a full section above.

### AudioCap

- URL: https://github.com/insidegui/AudioCap
- Licence: BSD-2-Clause (the repository's `LICENSE`, unchanged since 2024-05-17)
- Copyright: Copyright (c) 2024 Guilherme Rambo
- Commit: unknown. `6f609e8ad1b1e11fa0e8edbe91864cb099f00de3` (2025-08-07) is
  the latest commit, and was already the latest when our tap code was written
  (2026-09).
- What is known: SPEC.md (§2.3 and §7) and `docs/findings.md` (§4) say to
  "port structure from `insidegui/AudioCap`" for the Core Audio process tap. The tap was first written in `spikes/phase0a-tcc/src/probe/main.swift`
  and then ported to Rust in `crates/audio/src/macos/tap.rs`. Neither file
  says it was adapted from AudioCap. The aggregate-device setup in the spike
  uses the same Core Audio keys in the same order as AudioCap's
  `AudioCap/ProcessTap/ProcessTap.swift`; they are the keys the API needs, so
  this alone does not show a copy.

### sudara's Core Audio tap gist

- URL: https://gist.github.com/sudara/34f00efad69a7e8ceafa078ea0f76f6f
- Licence: no SPDX id. The file says: "License: You're welcome to do whatever
  you want with this code. If you do something cool please tell me though."
- Copyright: none stated
- Commit: gist revision `173d01fb4be1afce60bac4eedae67b6ef1fed683` (2024-05-10)
- What is known: named next to AudioCap in SPEC.md §2.3 and
  `docs/findings.md` §4 as a model for the tap. No file in the repo says it
  was adapted from it. The licence is not a standard one, so ask before
  copying from it.

### swift-scribe

- URL: https://github.com/FluidInference/swift-scribe
- Licence: MIT
- Copyright: none stated in its `LICENSE` file
- Commit: unknown. `dc80edc72dfe288e34a76bd89b7ad55ecbf5b199` (2026-07-10) is
  the latest commit.
- What is known: SPEC.md §7 says to read it before writing the speech
  sidecar, as a reference for Apple's `SpeechAnalyzer`. No file in
  `sidecar/meet-stt/` says it was adapted from it, and none was found to be.
