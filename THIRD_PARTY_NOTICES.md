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

## minutes

- URL: https://github.com/silverstein/minutes
- Licence: MIT
- Copyright: Copyright (c) 2026 Mat Silverstein
- Commit: c1e236acf3a3aea0729976cfc6959dcceb5cd75f
- Files:
  - `crates/store/src/retention.rs` (`apply`, and the preview-then-apply
    split) from `crates/core/src/retention.rs` (`apply_audio_retention`)
  - `crates/agent/src/detect.rs` (`find`: absolute path, then `which`, then
    known folders with `cmd`/`exe`/`bat`) and the folder lists in
    `crates/agent/src/platform/{macos,linux,windows}.rs`, from
    `crates/core/src/summarize.rs` (`resolve_agent_path`)

```
MIT License

Copyright (c) 2026 Mat Silverstein

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

## anarlog

- URL: https://github.com/fastrepl/anarlog
- Licence: MIT (the repository's `LICENSE`; `LICENSING.md` puts everything
  outside `enterprise/**` under it, and every file below is outside it)
- Copyright: Copyright (c) 2023-present Fastrepl, Inc.
- Commit: 93deb8642e75a0a2f8ece1bed186da4362213edd
- Files:
  - `crates/calendar/src/windows_tz.rs` (`windows_tz_to_iana`, the CLDR
    Windows → IANA table, as is, and its test) from
    `crates/calendar/src/windows_tz.rs`
  - `crates/calendar/src/microsoft/types.rs` (the Graph event models,
    trimmed to the fields we select) from
    `crates/outlook-calendar/src/types.rs`
  - `crates/calendar/src/google/types.rs` (the Calendar API v3 event models,
    trimmed to the fields we read) from
    `crates/google-calendar/src/types.rs`
  - `crates/audio/src/platform/windows/activity.rs` (the COM guard and the
    WASAPI capture-session walk, extended to render endpoints, and the
    `sysinfo` pid → name lookup) from `crates/detect/src/list/windows.rs`
  - `crates/audio/src/platform/linux/activity.rs` (the libpulse main loop,
    context-readiness wait and source-output listing, extended to sink
    inputs, and its readiness test) from `crates/detect/src/list/linux.rs`
  - `crates/detect/processes.json` (the Linux meeting-app process names) from
    `crates/detect/src/app/linux.rs`
  - `crates/audio/src/loopback/follower.rs` (`EndpointFollower`: switch
    only after two agreeing reads, and its four tests) from
    `crates/audio-actual/src/speaker/windows.rs` (TUR-37)
  - `crates/audio/src/segments/drift_audio.rs` (`GccPhatLagEstimator`,
    `LagTrendTracker` and the lock / lost probe, for `drift-check --audio`)
    from `crates/audio-sync/src/estimator.rs`, `crates/audio-sync/src/drift.rs`
    and `crates/audio-sync/src/probe.rs` (TUR-64)
  - `src-tauri/src/hooks/mod.rs` (`run_hook`: the timeout, kill on drop,
    capped output reads and `~`/`$HOME` expansion) from
    `crates/hooks/src/runner.rs` (TUR-63)
  - `crates/audio/src/platform/windows_render_choice.rs`
    (`endpoint_to_follow`: the default wins ties, otherwise the one endpoint
    another process is actively rendering to) and
    `crates/audio/src/platform/windows/render_in_use.rs` (the render-session
    walk) from `render_device_in_use` and `has_foreign_active_session` in
    `crates/audio-actual/src/speaker/windows.rs` (TUR-95)

```
MIT License

Copyright (c) 2023-present Fastrepl, Inc.

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

## Handy

- URL: https://github.com/cjpais/Handy
- Licence: MIT
- Copyright: Copyright (c) 2025 CJ Pais
- Commit: ffbc9504cbf004ce4819d2ca872fcea92be0fddf
- Files:
  - `src-tauri/src/logs/mod.rs` (`plugin`: the tauri-plugin-log size cap,
    rotation and targets) from `src-tauri/src/lib.rs`
  - `.github/workflows/check.yml` (the "Enable long paths (Windows)" step of
    the `rust-native` job) from `.github/workflows/build.yml`
  - `.github/workflows/release.yml` (the `build-other` matrix: windows-latest,
    ubuntu-22.04 deb, ubuntu-24.04 AppImage, and its long-paths step) from
    `.github/workflows/build.yml` (TUR-39)
  - `crates/audio/src/platform/windows_consent.rs` (`mic_consent`: the
    HKLM + HKCU + `NonPackaged` microphone consent decision) from
    `src-tauri/src/commands/audio.rs`, at commit
    73ab851c2b6242283759a4c101b60f0ece132f08 (TUR-51)
  - `src-tauri/src/settings_links.rs` (the
    `cmd /C start "" ms-settings:privacy-microphone` fallback) from
    `src-tauri/src/commands/audio.rs`, at commit
    73ab851c2b6242283759a4c101b60f0ece132f08 (TUR-51)
  - `src-tauri/src/tray/icons.rs` (the tray icon pick by OS and taskbar
    theme) and `src-tauri/src/platform/windows.rs` (`taskbar_is_light`, the
    `SystemUsesLightTheme` read) from `src-tauri/src/tray.rs`, at commit
    73ab851c2b6242283759a4c101b60f0ece132f08 (TUR-58)
  - `src/lib/osText.ts` (`shortcutLabel`: OS-aware modifier key names)
    from `src/lib/utils/keyboard.ts`, at commit
    73ab851c2b6242283759a4c101b60f0ece132f08 (TUR-58)
  - `.github/workflows/check.yml` (the "Install the Vulkan SDK (Windows)"
    and "Install the Vulkan SDK (Linux)" steps of the `rust-native` job:
    `humbletim/install-vulkan-sdk`, LunarG's apt list and
    `mesa-vulkan-drivers`) from `.github/workflows/build.yml`, at commit
    73ab851c2b6242283759a4c101b60f0ece132f08 (TUR-61)

MIT License

Copyright (c) 2025 CJ Pais

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.

## Vibe

- URL: https://github.com/thewh1teagle/vibe
- Licence: MIT
- Copyright: Copyright (c) 2024 thewh1teagle
- Commit: 53056d6bf3e5835c15fca3f1725f489de1e199f9
- Files:
  - `src-tauri/src/logs/platform/mod.rs` (`attach`: the `crash_handler` hook that
    records the exception code; its analytics and report dialog were not
    taken) from `desktop/src-tauri/src/setup.rs`

MIT License

Copyright (c) 2024 thewh1teagle

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.

## Tauri

- URL: https://github.com/tauri-apps/tauri
- Licence: Apache-2.0 OR MIT (taken under MIT)
- Copyright: Copyright (c) 2017 - Present Tauri Apps Contributors
- Commit: 6f6ab1207bb3923c2721fbc67d2fdb1c8deb0c7a
- Files:
  - `src-tauri/build.rs` (`embed_manifest_everywhere`) from
    `crates/tauri/build.rs` (`embed_manifest_for_tests`)
  - `src-tauri/windows-app-manifest.xml` from
    `crates/tauri-build/src/windows-app-manifest.xml`

```
MIT License

Copyright (c) 2017 - Present Tauri Apps Contributors

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

## process-wrap

- URL: https://github.com/watchexec/process-wrap
- Licence: Apache-2.0 (process-wrap is Apache-2.0 OR MIT; we chose Apache-2.0,
  because the two functions came to process-wrap from watchexec under
  Apache-2.0 only). Full text: our [`LICENSE`](./LICENSE).
- Copyright: Copyrights in this project are retained by their contributors
  (process-wrap `COPYRIGHT`); `job_object()` and `resume_threads()` are
  adapted from watchexec `lib/src/process.rs`, copyright Matt Green.
- Commit: ca45003a831ac125e6673b8759430e5ef33cc1db
- Files:
  - `crates/agent/src/platform/windows_job.rs` (`guard_tree`,
    `resume_threads`) from `src/windows.rs` (`make_job_object`,
    `resume_threads`)

## Meetily

- URL: https://github.com/Zackriya-Solutions/meetily
- Licence: MIT
- Copyright: Copyright (c) 2024 Zackriya Solutions
- Commit: a2cb62e827da7ef59f65064c97233efb2313878e
- Files:
  - `crates/stt/cmake/force-portable-ggml.cmake` from
    `.github/force-portable-ggml.cmake`
  - `.github/workflows/check.yml` (the "Vulkan loader for the test
    binaries (Windows)" step: where to look for `vulkan-1.dll`) from
    `.github/workflows/build-windows.yml` (TUR-61)

MIT License

Copyright (c) 2024 Zackriya Solutions

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.

## Cap

- URL: https://github.com/CapSoftware/Cap
- Licence: MIT (the repository's `LICENSE` puts the `scap-*` and
  `cap-camera*` crates under `licenses/LICENSE-MIT` and everything else under
  AGPL-3.0; only `crates/scap-cpal` was copied from)
- Copyright: Copyright (c) 2023 Cap Software, Inc.
- Commit: a2a6bd8b1948c48fe92936c265c8402d7fa8ddb3
- Files:
  - `crates/audio/src/loopback/buffer.rs` (`safe_buffer_size`: about 80 ms
    of frames, clamped) from `crates/scap-cpal/src/lib.rs` (TUR-37)
  - `crates/audio/src/platform/windows_loopback.rs` (`start_keepalive`: a
    render stream of zeros on the captured device, from
    `build_silence_keepalive`) from `crates/scap-cpal/src/lib.rs` (TUR-37)

```
MIT License

Copyright (c) 2023 Cap Software, Inc.

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

## cpal

- URL: https://github.com/RustAudio/cpal
- Licence: Apache-2.0 (full text in our [`LICENSE`](./LICENSE); cpal ships
  no `NOTICE` file)
- Copyright: The CPAL contributors (cpal's `LICENSE` carries no copyright
  line of its own)
- Commit: e1612d5d98152f8dc2a62e1b51ef7cbf4f7f26b7 (the `v0.18.2` tag, the
  version we depend on)
- Files:
  - `crates/audio/src/loopback/clock.rs` (`qpc_to_ns`: QPC ticks to ns on
    the 100 ns grid, from `Stream::now`) from
    `src/host/wasapi/stream.rs` (TUR-37)

## Whisper

- URL: https://github.com/openai/whisper
- Licence: MIT
- Copyright: Copyright (c) 2022 OpenAI
- Commit: 86098128c0b4f24f0e2aa2994de830614b474227
- Files:
  - `crates/stt/src/languages.rs` (`WHISPER_LANGUAGES`, the language codes
    and names, names title-cased) from `whisper/tokenizer.py` (`LANGUAGES`)

```
MIT License

Copyright (c) 2022 OpenAI

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

## Parakeet TDT 0.6B v3 (speech model)

Not code: the model the Parakeet engine (TUR-62) downloads at the user's
request and runs. No file of it is in this repository or in the app bundle.
CC-BY-4.0 asks for attribution wherever the model is used, so Settings, About
shows the same credit (`stt::model::parakeet::CREDIT`).

- URL: https://huggingface.co/nvidia/parakeet-tdt-0.6b-v3
- Licence: CC-BY-4.0 (https://creativecommons.org/licenses/by/4.0/)
- Copyright: NVIDIA Corporation
- Commit: `541d1f99c6b0c3cd0b11a95167540bb8edefd82b` (the model card read on
  2026-10-04)
- Files: none in the repository. The app downloads the ONNX export by
  istupakov, https://huggingface.co/istupakov/parakeet-tdt-0.6b-v3-onnx at
  commit `8f23f0c03c8761650bdb5b40aaf3e40d2c15f1ce` (also CC-BY-4.0, derived
  from NVIDIA's model): `encoder-model.int8.onnx`,
  `decoder_joint-model.int8.onnx` and `vocab.txt`, pinned by SHA-256 in
  `crates/stt/src/model/parakeet.rs`. meet-ai does not change them.

## Vulkan Loader

- URL: https://github.com/KhronosGroup/Vulkan-Loader
- Licence: Apache-2.0 (full text in our [`LICENSE`](./LICENSE); the
  repository ships no `NOTICE` file, and its `LICENSE.txt` says the few files
  under another licence are under more permissive ones)
- Copyright: Copyright (c) 2014-2023 The Khronos Group Inc., Copyright (c)
  2014-2023 Valve Corporation, Copyright (c) 2014-2023 LunarG, Inc., and the
  other holders named in each source file
- Commit: fa5f2bf40baa15c22be61e393e1dded6c06ee503 (the
  `vulkan-sdk-1.4.309.0` tag, the SDK check.yml installs)
- Files:
  - `vulkan-1.dll`, unmodified, next to `meet-ai.exe` in the Windows
    installer (`bundle.resources` in `src-tauri/tauri.windows.conf.json`),
    copied from the Vulkan SDK 1.4.309.0 by check.yml (TUR-61). Not in the
    repository.

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
