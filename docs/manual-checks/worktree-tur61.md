# Manual checks: tur61

TUR-61: whisper gets ggml's Vulkan backend on Windows x64 and Linux (macOS
keeps Metal, Windows on ARM stays CPU only), a GPU crash marker in `.app/`
that switches whisper to the CPU after a crash, and one hardware tier (GPU
usable yes/no + total memory) that picks the default model and the
"Recommended" row. Everything that runs headless is unit-tested on every OS;
nothing below could run on the Mac this was built on (no Vulkan SDK, no GPU
PC, no running app).

## Run by hand

1. **Speed on a Windows PC with a GPU.** On a Windows x64 PC with a discrete
   or integrated GPU and its vendor driver, install the NSIS build from CI
   (`meet-ai-windows-nsis` artifact of a push to main), download Large turbo,
   record a 5-minute call with speech on both tracks. Then create
   `<meetings>/.app/gpu-check` (the crash marker, which forces the CPU) with
   the app version on its first line (e.g. `0.4.0`; a marker from another
   version is ignored), start the app again and record the same audio.
   Delete the file afterwards.
   Expect: the log (`<meetings>/.app/logs/meet-ai.log`) shows
   `GPUs whisper can use devices=["<your GPU>"]` and `loading whisper
   use_gpu=true` for the first run, `use_gpu=false` for the second; the GPU
   run keeps up live. For numbers, time the same production path from a
   checkout, once without and once with the marker file:
   `cargo run --release -p stt --example offline_meeting -- transcribe
   --engine whisper --meeting crates/audio/fixtures/two-speaker-60s --scratch
   <tmp> --model-id large-v3-turbo-q5_0` (PowerShell: `Measure-Command`).
   Expect the GPU run to be several times faster. Put both times in the PR.
   Why skipped: needs a Windows PC with a GPU. Do not guess the speed-up;
   measure it.
2. **Same on Linux** (Ubuntu 24.04, Mesa or NVIDIA driver), from the `.deb`
   artifact. Expect: same log lines, GPU faster than CPU. `apt` installs
   `libvulkan1` with the package (new `depends`).
   Why skipped: needs a Linux machine with a GPU.
3. **No Vulkan driver.** A Windows VM with no GPU driver (Microsoft Basic
   Display Adapter), or a Linux machine with `mesa-vulkan-drivers` removed.
   Expect: the app starts, the log shows `devices=[]`, Settings marks Small
   "Recommended" with "has no graphics chip meet-ai can use", and a recording
   transcribes on the CPU.
   Why skipped: needs such a machine. Measure it.
4. **No Vulkan loader on Windows** (`vulkan-1.dll` absent from System32, e.g. a
   fresh VM). Expect, today: the app does not start ("vulkan-1.dll was not
   found"), because whisper-rs-sys 0.15 links the loader at load time. GPU
   drivers install the DLL, so this only hits PCs with no GPU driver at all.
   Fix (follow-up, release packaging is TUR-39's): ship `vulkan-1.dll` beside
   `meet-ai.exe` in the installer, as Meetily does, or move to transcribe-cpp's
   dynamic backends. Why skipped: needs a Windows machine without the DLL.
5. **GPU crash fallback.** On a GPU machine, start a recording, and while
   whisper is loading or before the first line appears, kill the app hard
   (`taskkill /F /IM meet-ai.exe`, `kill -9`). Start it again and record.
   Expect: `<meetings>/.app/gpu-check` exists after the kill; the next start
   logs "whisper crashed on the GPU in an earlier run, so it runs on the CPU;
   delete this file to try the GPU again" and `use_gpu=false`; Settings says
   the graphics chip crashed and recommends Small; the recording transcribes.
   Delete the file and restart: `use_gpu=true` again. A normal quit or a
   finished recording leaves no `gpu-check` behind. After an app update the
   old marker is replaced and the GPU tried again (log: "a GPU crash marker
   from another app version").
   Why skipped: needs the running app on a GPU machine.
6. **AppImage.** Check whether the AppImage (TUR-39) carries
   `libvulkan.so.1` or needs the host's; on a distro without `libvulkan1` the
   binary does not start. Why skipped: no AppImage is built in check.yml.

## Known limits

- Only the first decode is covered by the marker: it is removed once whisper
  has decoded once on the GPU. A ggml assert later in a long meeting (VRAM
  pressure, Handy #2114) crashes the app and the next run tries the GPU again.
  The follow-up is inference in a child process.
- A crash while the Vulkan loader starts (instance creation, device listing)
  is not covered: ggml registers its Vulkan backend for every whisper load,
  CPU or GPU, and the hardware tier lists devices the same way. With the
  backend compiled in (no dynamic backends in whisper-rs-sys 0.15) there is no
  way around that in-process. transcribe-cpp (Handy's move) is the follow-up.
- The app being quit by the OS while a model is loaded and before the first
  decode (no `Drop`) leaves the marker, so that machine then uses the CPU until
  the file is deleted.
- A model left in the old `~/Meetings/.app/models` folder keeps its marker in
  that `.app/`; the Settings recommendation only reads the chosen root's.
- The tier threshold (16 GB, `LARGE_MODEL_MIN_MEMORY`) is TUR-79's Apple
  silicon line, reused untested for Vulkan GPUs. Tune it with the numbers from
  checks 1 and 2.
- The first hardware read off macOS starts the Vulkan loader, which can take a
  moment; it happens once, on whichever thread first needs the default model.

## Decisions taken

- whisper-rs 0.16 / whisper-rs-sys 0.15 cannot load backends at run time, so
  the `vulkan` feature is compiled in (DEFAULTS). With no Vulkan driver ggml
  registers no GPU and whisper runs on the CPU by itself.
- The marker stays after a crash until the user deletes it or the app is
  updated (A2): it holds the app version that wrote it, and a marker from
  another version is replaced and the GPU tried again. A broken driver stays
  broken, so retrying every other run would crash every other meeting.
- The default model is the tier's pick, unless only other models are
  downloaded: then the first of those, so nobody who downloaded Large turbo
  before this change is told to download Small. A model in `config.jsonc`
  always wins. `config.schema.json` still documents Large turbo as the
  default.
- GPU detection: Apple silicon on macOS (as TUR-79 did); ggml's own device
  list off macOS, which skips CPU-type Vulkan devices such as Mesa's
  lavapipe.
