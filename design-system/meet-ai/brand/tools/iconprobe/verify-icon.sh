#!/bin/zsh
# verify-icon.sh <app> [--label NAME] [--against RUN_DIR|none]
#
# One pass over a built .app for TUR-86: is the bundle still sealed, does it
# carry the Icon Composer icon the way macOS 26 expects, does the system
# actually draw that icon rather than the .icns shipped beside it, does the
# .icns fallback still work, and how does every rung compare with the baseline.
#
# Everything lands in target/icon-lab/runs/<label>/ (gitignored): renders,
# contact sheets, summary.md. Nothing touches the bundle you pass in — every
# trial runs on a lab copy.
#
#   verify-icon.sh /path/to/meet-ai.app --label baseline      # once, today's .icns
#   verify-icon.sh /path/to/meet-ai.app --label icon          # compares to runs/baseline
#   verify-icon.sh /System/Applications/Utilities/Terminal.app --label calib-terminal --against none
#                                  # calibration: an Apple app that ships both
#                                  # Assets.car and an .icns must pass every check
#
# Exit 0 only when every hard check passes. Rungs that differ from the baseline
# are flagged REVIEW, not failed: whether a difference is better or worse is a
# judgement for a person looking at the sheets, not for a pixel count.
set -euo pipefail

HERE="${0:A:h}"
REPO="${HERE:h:h:h:h:h}"
LAB="${ICONLAB:-$REPO/target/icon-lab}"
BIN="$LAB/bin"
LSREG="/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister"
PB=/usr/libexec/PlistBuddy
IDENTITY="${ICONPROBE_IDENTITY:-meet-ai Local Signing}"

# Every rung Tess and Leo measured on TUR-14/TUR-22 (16/32/41/64/128/256, 41
# being the user's Dock tile at 1x), the rest of the .icns ladder up to 1024,
# and the 2x reps a Retina Finder or Dock draws. "pt@2x" means that point size
# on a 2x backing store.
RUNGS=(16 32 41 64 128 256 512 1024 16@2x 32@2x 128@2x 256@2x 512@2x)
# Contact-sheet groups: rungs of one output pixel size, and the magnification
# that makes each readable.
SHEETS=(
  "small:8:16 32 16@2x"
  "mid:4:41 64 32@2x"
  "large:1:128 256 128@2x"
  "xl:1:512 256@2x"
  "xxl:1:1024 512@2x"
)
# pixdiff "changed" percentages. SAME: two renders the eye cannot tell apart
# (two draws of one icon measure 0.00). GENERIC: a render this close to the
# folded-page or generic-app icon is LaunchServices giving up, not a result.
# VISIBLE: the decoy must move at least this much of the tile to count as seen.
SAME=0.5 GENERIC=2 VISIBLE=20

app="" label="candidate" against=""
while (( $# )); do
  case "$1" in
    --label)   label="$2"; shift 2 ;;
    --against) [[ "$2" == none ]] && against=none || against="${2:A}"; shift 2 ;;
    -*)        echo "unknown option $1" >&2; exit 2 ;;
    *)         app="${1:A}"; shift ;;
  esac
done
[[ -d "$app/Contents" ]] || { echo "usage: verify-icon.sh <app> [--label NAME] [--against RUN_DIR]" >&2; exit 2 }
[[ -z "$against" && "$label" != baseline && -d "$LAB/runs/baseline/renders" ]] && against="$LAB/runs/baseline"
[[ "$against" == none ]] && against=""

RUN="$LAB/runs/$label"
rm -rf "$RUN"; mkdir -p "$RUN"/{renders,trials,generic,sheets}
SUMMARY="$RUN/summary.md"
typeset -i failures=0
typeset -a rows

# --- tools ------------------------------------------------------------------
mkdir -p "$BIN"
for tool in sysicon sheet pixdiff decoy; do
  if [[ ! -x "$BIN/$tool" || "$HERE/$tool.swift" -nt "$BIN/$tool" ]]; then
    echo "==> building $tool"
    swiftc -O "$HERE/$tool.swift" -o "$BIN/$tool"
  fi
done

# check <name> <PASS|FAIL|REVIEW|INFO|N/A> <detail>
check() {
  rows+=("| $1 | **$2** | $3 |")
  printf '%-22s %-6s %s\n' "$1" "$2" "$3"
  [[ "$2" == FAIL ]] && failures+=1
  return 0
}
changed() { local o; o="$("$BIN/pixdiff" "$1" "$2")" || { echo 100; return }; o="${o#*changed=}"; echo "${o%\%}" }
render() {  # render <app> <rung> <out.png>
  local pt="${2%@2x}" scale=1
  [[ "$2" == *@2x ]] && scale=2
  "$BIN/sysicon" "$1" "$pt" "$3" --scale "$scale"
}

plist="$app/Contents/Info.plist"
res="$app/Contents/Resources"
echo "== $label: $app"
{
  echo "bundle:   $app"
  echo "built:    $(stat -f %Sm "$app/Contents/Info.plist")"
  echo "sealed:   $(codesign -dvv "$app" 2>&1 | grep -E '^(Authority|Signed Time)=' | head -2 | tr '\n' ' ')"
  echo "os:       $(sw_vers -productVersion) ($(sw_vers -buildVersion))"
  echo "icon style: $(defaults read -g AppleIconAppearanceTheme 2>/dev/null || echo 'default (unset)')"
} | tee "$RUN/conditions.txt"

# --- 1. signature, on the bundle exactly as handed to us ---------------------
if out="$(codesign --verify --deep --strict --verbose=2 "$app" 2>&1)"; then
  check "codesign --deep --strict" PASS "valid on disk, satisfies its Designated Requirement"
else
  check "codesign --deep --strict" FAIL "$(echo "$out" | tail -1)"
fi
echo "$out" > "$RUN/codesign.txt"

# --- 2. Info.plist -----------------------------------------------------------
iconfile="$($PB -c 'Print :CFBundleIconFile' "$plist" 2>/dev/null || true)"
iconname="$($PB -c 'Print :CFBundleIconName' "$plist" 2>/dev/null || true)"
icns=""
if [[ -n "$iconfile" ]]; then
  icns="$res/$iconfile"; [[ -f "$icns" ]] || icns="$res/$iconfile.icns"
  if [[ -f "$icns" ]]; then check "CFBundleIconFile" PASS "\`$iconfile\` -> Resources/${icns:t} ($(shasum -a 256 "$icns" | cut -c1-12)…)"
  else check "CFBundleIconFile" FAIL "\`$iconfile\` set but no such file in Resources"; icns=""; fi
else
  check "CFBundleIconFile" FAIL "missing — the .icns fallback is not declared"
fi
if [[ -n "$iconname" ]]; then check "CFBundleIconName" PASS "\`$iconname\`"
else check "CFBundleIconName" FAIL "missing — macOS will not look in Assets.car for the app icon"; fi

# --- 3. Assets.car -------------------------------------------------------------
car="$res/Assets.car"
if [[ -f "$car" ]]; then
  check "Assets.car present" PASS "$(stat -f %z "$car") bytes"
  /usr/bin/assetutil --info "$car" > "$RUN/assets.json"
  # An Icon Composer .icon compiles to IconImageStack renditions (one per
  # appearance) plus flattened Icon Image / MultiSized Image fallbacks, all
  # under the icon's name. Terminal.app's catalog has exactly that shape.
  carinfo="$(python3 - "$RUN/assets.json" "$iconname" <<'PY'
import json, sys
d = json.load(open(sys.argv[1])); name = sys.argv[2]
mine = [x for x in d[1:] if x.get("Name") == name]
types = sorted({x.get("AssetType") for x in mine})
looks = sorted({x.get("Appearance") for x in mine if x.get("AssetType") == "IconImageStack" and x.get("Appearance")})
names = sorted({x.get("Name") for x in d[1:] if x.get("AssetType") in ("IconImageStack", "Icon Image", "MultiSized Image")})
print(("ok" if any(t in ("IconImageStack", "Icon Image", "MultiSized Image") for t in types) else "none")
      + "\t" + ", ".join(types) + "\t" + ", ".join(looks) + "\t" + ", ".join(n for n in names if n))
PY
)"
  IFS=$'\t' read -r carok cartypes carlooks carnames <<< "$carinfo"
  if [[ -z "$iconname" ]]; then
    check "Assets.car app icon" FAIL "no CFBundleIconName to look up; icons in the catalog: ${carnames:-none}"
  elif [[ "$carok" == ok ]]; then
    check "Assets.car app icon" PASS "\`$iconname\`: $cartypes${carlooks:+ — appearances: $carlooks}"
    [[ "$cartypes" == *IconImageStack* ]] \
      || check "Icon Composer layers" FAIL "no IconImageStack — this is a flattened app-icon set, not a compiled .icon"
  else
    check "Assets.car app icon" FAIL "no app icon named \`$iconname\`; icons in the catalog: ${carnames:-none}"
  fi
else
  check "Assets.car present" FAIL "no Contents/Resources/Assets.car"
  check "Assets.car app icon" FAIL "nothing to inspect"
fi

# --- 4. lab copies ---------------------------------------------------------------
# Each trial is a re-signed copy with its own bundle id. A copy that kept the
# real id could be answered out of the IconServices cache with the icon of a
# different build of the same app — which would make every comparison "same".
REAL_HOME="$(/usr/bin/dscl . -read "/Users/$(id -un)" NFSHomeDirectory 2>/dev/null | awk '{print $2}')"
KEYCHAIN="${REAL_HOME:-$HOME}/Library/Keychains/meet-ai-signing.keychain-db"
resign() {
  # --preserve-metadata keeps the app's entitlements; nested code (meet-stt)
  # keeps its own signature and is sealed into the new outer one.
  if [[ -f "$KEYCHAIN" ]] && security find-identity -v -p codesigning "$KEYCHAIN" 2>/dev/null | grep -q "$IDENTITY"; then
    codesign --force --options runtime --timestamp=none --preserve-metadata=entitlements \
      --keychain "$KEYCHAIN" -s "$IDENTITY" "$1" >/dev/null 2>&1
  else
    codesign --force --timestamp=none --preserve-metadata=entitlements -s - "$1" >/dev/null 2>&1
  fi
  codesign --verify --deep --strict "$1" 2>/dev/null
}

# The decoy .icns mirrors the real one's rep ladder slot for slot — TUR-22
# showed which reps exist changes how macOS 26 draws a legacy .icns, so a decoy
# with a different ladder would not be testing the same path.
decoy_icns="$RUN/decoy.icns"
if [[ -n "$icns" ]]; then
  "$BIN/decoy" "$RUN/decoy-1024.png"
  iconutil -c iconset "$icns" -o "$RUN/real.iconset"
  mkdir -p "$RUN/decoy.iconset"
  for f in "$RUN/real.iconset"/*.png; do
    sips -z "$(sips -g pixelHeight "$f" | awk '/pixelHeight/{print $2}')" \
            "$(sips -g pixelWidth "$f" | awk '/pixelWidth/{print $2}')" \
         "$RUN/decoy-1024.png" --out "$RUN/decoy.iconset/${f:t}" >/dev/null
  done
  iconutil -c icns "$RUN/decoy.iconset" -o "$decoy_icns"
fi

registered=()
# trial <name> <mutation...>: mutations are "decoy" (swap the .icns for the
# decoy) and "nocar" (drop Assets.car and CFBundleIconName, i.e. the bundle as
# it would be with only the legacy path).
trial() {
  local name="$1"; shift
  local copy="$RUN/trials/$name/${app:t}"
  mkdir -p "${copy:h}"
  # ditto refuses some system bundles (Terminal.app, used to calibrate this
  # script) with "Operation not permitted"; plain cp -R copies them fine.
  ditto "$app" "$copy" 2>/dev/null || { rm -rf "$copy"; cp -R "$app" "$copy"; }
  for m in "$@"; do
    case "$m" in
      decoy) cp "$decoy_icns" "$copy/Contents/Resources/${icns:t}" ;;
      nocar) rm -f "$copy/Contents/Resources/Assets.car"
             $PB -c 'Delete :CFBundleIconName' "$copy/Contents/Info.plist" 2>/dev/null || true ;;
    esac
  done
  local id; id="$($PB -c 'Print :CFBundleIdentifier' "$copy/Contents/Info.plist")"
  $PB -c "Set :CFBundleIdentifier $id.iconprobe.$(date +%s)$RANDOM" "$copy/Contents/Info.plist"
  resign "$copy" || { echo "lab copy $name did not re-sign cleanly" >&2; exit 1 }
  "$LSREG" -f "$copy" >/dev/null 2>&1 || true
  registered+=("$copy")
  touch "$copy"
  mkdir -p "$RUN/trials/$name/renders"
  for r in "${RUNGS[@]}"; do render "$copy" "$r" "$RUN/trials/$name/renders/R-$r.png"; done
}

# Generic references, per rung: the folded-page document icon and the generic
# application icon. Any render that matches one of these is a harness failure.
stub="$LAB/generic/Stub.app"
mkdir -p "$stub/Contents"
[[ -f "$stub/Contents/Info.plist" ]] || \
  printf '<?xml version="1.0" encoding="UTF-8"?>\n<plist version="1.0"><dict><key>CFBundlePackageType</key><string>APPL</string></dict></plist>\n' > "$stub/Contents/Info.plist"
for r in "${RUNGS[@]}"; do
  render "$LAB/generic/does-not-exist.app" "$r" "$RUN/generic/doc-$r.png"
  render "$stub" "$r" "$RUN/generic/app-$r.png"
done
not_generic() {  # not_generic <trial>  -> prints offending rungs, empty if clean
  local bad=() r
  for r in "${RUNGS[@]}"; do
    local f="$RUN/trials/$1/renders/R-$r.png"
    (( $(changed "$f" "$RUN/generic/doc-$r.png") < GENERIC || $(changed "$f" "$RUN/generic/app-$r.png") < GENERIC )) && bad+=("$r")
  done
  echo "${bad[*]:-}"
}
# worst <trialA dir> <trialB dir> -> the largest "changed" % across all rungs
worst() {
  local w=0 r c
  for r in "${RUNGS[@]}"; do c="$(changed "$1/R-$r.png" "$2/R-$r.png")"; (( c > w )) && w=$c; done
  echo "$w"
}

echo "==> rendering trials"
trial shipped
cp "$RUN/trials/shipped/renders/"*.png "$RUN/renders/"
"$BIN/sysicon" "$RUN/trials/shipped/${app:t}" 128 "$RUN/trials/reps-probe.png" --reps > "$RUN/reps.txt"
cp "$RUN/reps.txt" "$RUN/renders/reps.txt"

bad="$(not_generic shipped)"
if [[ -z "$bad" ]]; then check "renders are the icon" PASS "no rung matches the generic document or app icon"
else check "renders are the icon" FAIL "generic fallback at: $bad — LaunchServices gave up; nothing below is trustworthy"; fi

# The direct render of the bundle as handed to us, rung for rung against the
# lab copy. A difference here usually means IconServices answered the real
# bundle id from its cache (Terminal.app shows ~1%) — the lab copy is the
# measurement either way.
mkdir -p "$RUN/direct"
for r in "${RUNGS[@]}"; do render "$app" "$r" "$RUN/direct/R-$r.png"; done
w="$(worst "$RUN/direct" "$RUN/renders")"
if (( w <= SAME )); then check "direct == lab copy" PASS "worst rung ${w}% changed"
else check "direct == lab copy" INFO "worst rung ${w}% — the real bundle id may be served from the icon cache; the lab copy is the measurement"; fi

# --- 5. which icon does the system draw? ---------------------------------------
if [[ -z "$icns" ]]; then
  check "draws Assets.car" N/A "no .icns to swap, so this cannot be told apart"
elif [[ -f "$car" && -n "$iconname" ]]; then
  trial decoy decoy                 # Assets.car + decoy .icns
  trial fallback nocar              # real .icns only
  trial decoy-fallback decoy nocar  # decoy .icns only: the positive control
  seen="$(worst "$RUN/trials/shipped/renders" "$RUN/trials/decoy-fallback/renders")"
  moved="$(worst "$RUN/trials/shipped/renders" "$RUN/trials/decoy/renders")"
  if [[ -n "$(not_generic decoy-fallback)" ]]; then
    check "draws Assets.car" FAIL "control failed: the decoy-only copy came back generic — LaunchServices gave up, so the test proves nothing"
  elif (( seen < VISIBLE )); then
    check "draws Assets.car" FAIL "control failed: with Assets.car removed the decoy .icns moved only ${seen}% — the swap is not being seen, so the test proves nothing"
  elif (( moved <= SAME )); then
    check "draws Assets.car" PASS "swapping the .icns for a decoy changes nothing (worst rung ${moved}%); control without Assets.car shows the decoy (${seen}%)"
  else
    check "draws Assets.car" FAIL "the decoy .icns shows through at some rung (worst ${moved}%) — the system is drawing the .icns, not Assets.car"
  fi
  # Not a pass/fail: on Terminal.app the two paths differ visibly (the glass
  # rim), so a non-zero number here is normal. Zero would be odd enough to note.
  w="$(worst "$RUN/trials/shipped/renders" "$RUN/trials/fallback/renders")"
  check "Assets.car vs .icns" INFO "the two paths differ by up to ${w}% of pixels on this bundle"
  bad="$(not_generic fallback)"
  if [[ -n "$bad" ]]; then
    check ".icns fallback" FAIL "with Assets.car removed, generic icon at: $bad"
  elif [[ -n "$against" ]]; then
    w="$(worst "$RUN/trials/fallback/renders" "$against/renders")"
    if (( w <= SAME )); then check ".icns fallback" PASS "with Assets.car removed, every rung matches the baseline (worst ${w}%)"
    else check ".icns fallback" REVIEW "with Assets.car removed, differs from the baseline (worst ${w}%) — has the .icns changed?"; fi
  else
    check ".icns fallback" PASS "with Assets.car removed, the .icns renders (no baseline to compare)"
  fi
else
  # No catalog: the decoy trial is the proof that the .icns is what is drawn.
  trial decoy decoy
  moved="$(worst "$RUN/trials/shipped/renders" "$RUN/trials/decoy/renders")"
  check "draws Assets.car" FAIL "no Assets.car/CFBundleIconName; the decoy .icns shows through (${moved}%), so the system draws the .icns"
fi

# --- 6. rung by rung against the baseline ----------------------------------------
typeset -a rungrows
if [[ -n "$against" ]]; then
  for r in "${RUNGS[@]}"; do
    o="$("$BIN/pixdiff" "$against/renders/R-$r.png" "$RUN/renders/R-$r.png")"
    c="${${o#*changed=}%\%}"
    if (( c <= SAME )); then v="same"; else v="**REVIEW** — differs, judge better/worse on the sheet"; fi
    rungrows+=("| $r | $o | $v |")
  done
  diffs=0; for row in "${rungrows[@]}"; do [[ "$row" == *REVIEW* ]] && diffs=$((diffs + 1)); done
  if (( diffs == 0 )); then check "vs baseline" PASS "all ${#RUNGS} rungs indistinguishable from ${against:t}"
  else check "vs baseline" REVIEW "$diffs of ${#RUNGS} rungs differ from ${against:t} — see the sheets"; fi
fi

# --- 7. contact sheets -------------------------------------------------------------
for spec in "${SHEETS[@]}"; do
  gname="${spec%%:*}" rest="${spec#*:}"; mag="${rest%%:*}"; rs=(${=rest#*:})
  cells=()
  for r in "${rs[@]}"; do
    [[ -n "$against" ]] && cells+=("base $r=$against/renders/R-$r.png")
    cells+=("$label $r=$RUN/renders/R-$r.png")
  done
  "$BIN/sheet" "$RUN/sheets/$gname.png" "$mag" "${cells[@]}" >/dev/null
done
# The trials side by side, one sheet per size so each keeps its magnification.
if [[ -d "$RUN/trials/decoy" ]]; then
  for pair in 16:8 32:8 128:2; do
    r="${pair%%:*}" cells=()
    for t in shipped decoy fallback decoy-fallback; do
      [[ -d "$RUN/trials/$t" ]] && cells+=("$t $r=$RUN/trials/$t/renders/R-$r.png")
    done
    "$BIN/sheet" "$RUN/sheets/trials-$r.png" "${pair#*:}" "${cells[@]}" >/dev/null
  done
fi

# LaunchServices keeps what we registered; take the lab copies back out.
for c in "${registered[@]}"; do "$LSREG" -u "$c" >/dev/null 2>&1 || true; done

# --- summary -------------------------------------------------------------------------
{
  echo "# iconprobe — $label"
  echo
  echo '```'; cat "$RUN/conditions.txt"; echo '```'
  echo
  echo "| Check | Result | Detail |"
  echo "|---|---|---|"
  printf '%s\n' "${rows[@]}"
  if (( ${#rungrows} )); then
    echo
    echo "## Rungs vs ${against:t}"
    echo
    echo "| Rung | pixdiff | Verdict |"
    echo "|---|---|---|"
    printf '%s\n' "${rungrows[@]}"
  fi
  echo
  echo '## `sysicon --reps` (128)'
  echo
  echo '```'; cat "$RUN/reps.txt"; echo '```'
  echo
  echo "Sheets: $RUN/sheets/"
} > "$SUMMARY"

echo
echo "summary: $SUMMARY"
echo "sheets:  $RUN/sheets/"
(( failures == 0 )) || { echo "$failures check(s) FAILED" >&2; exit 1 }
echo "all hard checks passed"
