# Manual checks: TUR-22 (model download hangs when the connection stalls)

The fix is in `crates/modelfetch` only. A download attempt now fails with the
usual retryable `Error::Download` when the server sends nothing for
`STALL_TIMEOUT` (30 s), while waiting for the response headers or for any
body chunk. The retry then resumes from the `.part` file. There is still no
limit on total download time, so a slow but live connection is not cut off.

## What was run here, headless

- `cargo test -p modelfetch`: the new in-process server tests use a 200 ms
  (or 300 ms) stall limit:
  - `a_connection_that_goes_silent_mid_body_is_retried_and_resumed`: the
    server sends headers and 5 of 16 bytes, then keeps the socket open and
    goes silent. The attempt fails, the retry sends `Range: bytes=5-`, and
    the finished file matches.
  - `a_server_that_always_stalls_ends_in_a_download_error`: every connection
    stalls. The result is a `Download` error that says "stalled", after
    1 try + 3 retries.
  - `a_slow_but_live_download_is_not_cut_off`: one byte every 50 ms, so the
    whole body takes ~800 ms against a 300 ms limit. It finishes in one try.
  - Each test fails after 10 s instead of hanging if the fix regresses.
- `cargo clippy -p modelfetch --all-targets -- -D warnings`, `cargo fmt --all --check`.
- Mutation check: with the body-chunk timeout removed, the stall tests fail
  through the 10 s guard ("the download hung"). Reverted.

## Found on the way: a failed attempt could lose or misplace its bytes

Running the suite 4 at a time, 30 rounds, the *existing* retry tests failed
9 of 120 runs on `main` (`resume_sends_range_and_bytes_match` saw no
`Range` header; `fails_twice_then_succeeds` failed its checksum). Cause: a
`tokio::fs::File` write only queues the bytes, and an attempt that failed
mid-body dropped the file without flushing. The queued write could then land
after the retry had already measured or truncated the `.part` file. In the
real app that means a resume from the wrong offset or a corrupt file that
fails its checksum. `download` now flushes before it returns an error.
Same 4×30 loop afterwards (and 4×40 with the new tests): 0 failures.

## Needs a person (not run: needs the real network and a real model download)

1. Delete the whisper model from the models folder under the meetings root
   (`meeting_format::layout::models_dir`) and start a download from the app,
   or run `just model <id>`.
2. Partway through, cut the network so the TCP session cannot reset. Turning
   Wi-Fi off often makes the connection fail fast, which is not the case
   under test. Pulling the uplink cable from the router, or a Network Link
   Conditioner profile with 100% loss, is closer.
3. Expected: about 30 s after the bar stops moving, the log shows
   `download failed; retrying` with "the connection stalled". When the
   network comes back, the next attempt resumes from the same byte count
   (it does not start from 0) and the model verifies. If the network stays
   down through all 3 retries, the app shows the download error with a
   "try again" option, not a frozen bar.

Skipped here because the run must not make real network calls or download
models.
