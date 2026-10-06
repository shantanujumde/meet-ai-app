# Manual checks: TUR-125

TUR-125: the panic hook and the native crash handler are installed at the top
of `run()` (`logs::install_early`) when the logs folder is known (onboarding
done), so a crash before `setup` still leaves a crash file. A `OnceLock` keeps
`setup` from adding a second hook. Tested headless: installing twice writes one
report per panic; `a_real_panic_leaves_a_crash_file` still passes.

## Run by hand

1. Native crash check 4 of `docs/manual-checks/worktree-tur46.md` ("A forced native
   crash in the real app leaves a file"), on a signed build after onboarding.
   Expect: exactly one new `crash-*` file in `<meetings>/.app/logs`, and the
   log still has one "logs and crash files go here" line.
   Why skipped: needs the running signed app.
2. Before onboarding (fresh config), a crash before `setup` leaves no meet-ai
   crash file; only the OS crash report (Console, Crash Reports). By design.
   Why skipped: needs the running app.
