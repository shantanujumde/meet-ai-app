# Manual checks: tur65

TUR-65: warn when recording without headphones (SPEC L6, problem.md item
51). While recording, the app reads the default output every 3 s and shows a
quiet, dismissible line under the titlebar, "No headphones: the transcript
may repeat lines", when the output is speakers and `audio.warn_no_headphones`
is on (default). Headphones, a headset, or an output it cannot tell apart
show nothing. It never blocks or stops a recording.

The decision is unit-tested on all three CI OSes with fake device info
(`audio::headphones::classify`, `platform/headphones/parse.rs` for each OS's
raw values, `headphone_warning::Tracker` for when to send). The banner is
tested in `src/ui/HeadphoneBanner.test.tsx`. Nothing below could run here:
each needs a signed build, the running app and real output devices.

## Seen on this Mac

`cargo test -p audio the_default_output_is_readable -- --nocapture` (a
property read, nothing opened) printed, with Bluetooth buds as the output:
`OutputDevice { name: "realme Buds Air7", transport: Bluetooth, form:
Headphones } -> Headphones`. So Core Audio gave these buds a headphone
terminal type or data source. Built-in speakers and wired headphones were
not tried.

## Run by hand

Signed build of this branch (`just bundle-signed`). After each step, read
`~/Meetings/.app/logs/meet-ai.log` for `headphone warning output=... show=...`
(one line per change). With `RUST_LOG=audio=debug` the log also has
`default output read` with the device name, transport and form.

### macOS

1. Built-in speakers (MacBook), nothing plugged in. Record 20 s.
   Expect: banner within 3 s of the start; `output=Speakers show=true`.
   Measure it: which `form` the built-in output reports (data source
   `ispk` is expected on Intel MacBooks; Apple silicon lists "MacBook Pro
   Speakers" as its own device).
2. Wired headphones in the jack before recording. Record 20 s.
   Expect: no banner; `output=Headphones show=false`. Measure it: whether
   the jack shows as "External Headphones" (its own device) or as the
   built-in device with data source `hdpn`.
3. Start on speakers, then plug wired headphones in mid-recording.
   Expect: the banner goes away within 3 s; unplug, it comes back.
4. AirPods or other Bluetooth headphones. Record 20 s.
   Expect: no banner.
5. A Bluetooth speaker (JBL, Bose SoundLink, HomePod via AirPlay).
   Expect: banner.
6. A monitor's HDMI/DisplayPort audio, a USB DAC with no headphone word in
   its name, a virtual device (BlackHole, a Multi-Output Device).
   Expect: no banner (unknown never warns); `output=Unknown`.
7. Dismiss the banner on speakers. Expect: it stays hidden for that
   recording; going to headphones and back to speakers shows it again; the
   next recording shows it again.
8. `"audio": { "warn_no_headphones": false }` in `config.jsonc`, speakers.
   Expect: no banner; log `audio.warn_no_headphones is off`.

### Windows 10/11

1. Laptop speakers. Expect: banner (`form` from the endpoint form factor,
   Speakers = 1).
2. Wired headphones in the jack. Expect: no banner. Measure it: many
   Realtek drivers keep one "Speakers" endpoint for the speakers and the
   jack, with form factor Speakers even when headphones are in; then this
   warns wrongly. If so, note the driver and the endpoint name.
3. Bluetooth headphones (A2DP), then the same headset in a call (HFP,
   "Headset (... Hands-Free AG Audio)"). Expect: no banner in both; the
   debug log shows `transport: Bluetooth` from the `bth...` adapter bus.
4. A Bluetooth speaker. Expect: banner. Measure it: the form factor Windows
   gives a Bluetooth speaker.
5. A USB headset. Expect: no banner (form factor Headset/Headphones, or the
   name).
6. HDMI monitor audio. Expect: no banner (DigitalAudioDisplayDevice is
   unknown).

### Linux (PulseAudio and PipeWire)

1. Laptop speakers. Expect: banner; the default sink's active port is
   `analog-output-speaker`.
2. Wired headphones in the jack. Expect: no banner; port
   `analog-output-headphones`. Measure it on PipeWire (pipewire-pulse) as
   well as PulseAudio: the port names should be the same.
3. Bluetooth headphones in A2DP, then switched to the headset profile (HFP
   or HSP). Expect: no banner in both (`bluez_output...`,
   `bluetooth.protocol` or `api.bluez5.profile`).
4. HDMI output or a sink with no port. Expect: no banner.
5. No sound server running. Expect: no banner, recording unaffected
   (`could not read the default output` at debug).
