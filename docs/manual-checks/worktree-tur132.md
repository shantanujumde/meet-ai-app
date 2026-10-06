# Manual checks: tur132

TUR-132: Settings → Speech can delete a downloaded whisper model that is not
picked and not in use (`delete_model`, `src-tauri/src/engine/delete.rs`). The
refusal logic and the file removal are unit-tested in a temp dir; the UI row
is tested in `ModelRow.test.tsx`. Everything below needs the running app.

## Run by hand

1. With two whisper models downloaded, open Settings → Speech. On the model
   that is not picked press Delete, then Delete again in "Delete <size>?".
   Expect: the row loses "On this Mac" and shows Download; the file is gone
   from `<meetings root>/.app/models/` (and any `<file>.part` with it).
   Why skipped: needs the running app and real model files.
2. Press Download on the deleted model.
   Expect: it downloads from scratch and verifies, and can be picked again.
   Why skipped: downloads a model file, which this run must not do.
3. Press Delete, then Keep.
   Expect: nothing is deleted and the row goes back to its Delete button.
4. Start a recording, then try to delete a model that is not picked.
   Expect: "That model is in use. Pick another model first, and stop any
   recording." and the file stays.
   Why skipped: needs a signed build and a mic.
5. Start a download of a model and, from a second window or quickly via the
   console, call `delete_model` for it.
   Expect: refused with "That model is downloading."
   Why skipped: needs the running app.
6. A model only in the stranded `~/Meetings/.app/models` (meetings folder moved
   elsewhere): Delete removes that copy. Verify it.
