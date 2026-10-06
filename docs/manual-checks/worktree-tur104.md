# Manual checks: TUR-104

The Windows installer now ships `onnxruntime.dll` (Microsoft's official
`onnxruntime-win-x64-1.28.3.zip`, CPU build, SHA-256
`1d6fab48e85f948436af7c8c971d2c145cf224e2c444755dec894f8b0de11a83`, computed
here from a fresh download) next to `meet-ai.exe`, through `bundle.resources` in
`src-tauri/tauri.windows.conf.json`. check.yml and release.yml download it; check.yml
also sets `ORT_DYLIB_PATH` for the Windows tests. release.yml lists the NSIS
installer with 7-Zip and fails if `onnxruntime.dll` is missing.

Settings → Speech already hides the "not ready" state whenever
`runtime_ready` is true (`src/ui/engine/EnginePicker.tsx`), and that flag is
true once the DLL sits beside the exe, so no UI code changed. The guard stays
for a DLL that was deleted. `src/ui/engine/**` belongs to another task in
this run.

Only `actionlint` and the quality gate ran here; nothing Windows ran.

## Run by hand

1. **Fresh install picks Parakeet.** On a Windows 11 x64 PC with no meet-ai,
   install the NSIS installer from a release or from check.yml's bundle
   artifact. Open Settings → Speech.
   Expect: Parakeet is offered with a download, not "not ready". Download the
   model, record a short meeting, and get a transcript, all on the CPU.
   Why skipped: needs a real Windows machine and the running app.

2. **The DLL is next to the exe.** After step 1, look in the install folder
   (`%LOCALAPPDATA%\meet-ai` for a per-user NSIS install; verify it).
   Expect: `onnxruntime.dll` beside `meet-ai.exe`, about 15.8 MB.
   Why skipped: needs a Windows install.

3. **The release check runs.** On the next release run, the step "Installer
   carries onnxruntime.dll (Windows)" passes and its log lists
   `onnxruntime.dll`. Whether 7-Zip on the runner image can list the files
   in a Tauri NSIS installer has not been checked here: verify it. If it
   cannot, that step fails loudly; it never passes by mistake.
   Why skipped: needs a real release run.

4. **Old CPU.** On a PC without AVX2, if one is around, repeat step 1.
   Expect: it transcribes (CPU build, no DirectML).
   Why skipped: needs that hardware.
