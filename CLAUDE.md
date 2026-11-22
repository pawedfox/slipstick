# Slipstick — notes for agents

The app was called Clawculator until 0.9.0 (2026-11-20). The project folder
on the dev machine still has the old name; do not rename it from a session.
"Clawculator" should now appear only where `SPEC.md` records history and
in the one place the app looks for its old settings (`settings.rs`).

**This repository is public.** The author's private working notes (on the
name, on Anthropic's terms, and on correspondence about them) live in
`private/`, which is not in the repository. Never copy from them into a
tracked file, a commit message, a release or an issue, and never write
anything about the author's accounts, purchases or correspondence there.

Read `SPEC.md` before doing anything. It is the source of truth for what this
app is and what has been decided. If a task conflicts with it, stop and ask;
do not quietly change the spec or work around it.

## Current state

Version 0.9.2, the first public release. Work follows the "Roadmap"
section of `SPEC.md`, in order. It was revised on 2026-11-22:

- The first roadmap is built: the instant path, session history, a
  movable window, the system theme, the installer, Claude through the
  user's Claude Code, pasted pictures, the settings screen, the error
  states, the content security policy and the README.
- Next is item 2 of the revised roadmap: widening the instant path
  (decibels, temperature differences, `rpm to Hz`, then complex numbers).
- The installer is unsigned. The application for signing is held until
  there is use to show; see "Release" in `SPEC.md`.
- The API-key route is not built. It is item 4.
- See "Release" in `SPEC.md` for how a release is made. Never change the
  repository's visibility, publish a release or push a tag without being
  asked.

**Never run the installer from a Claude desktop session.** The session
redirects writes under AppData into the Claude app's private storage, so
the install looks real from inside the session and does not exist for
Windows. Build the installer, then ask the user to run it themselves from
`src-tauri/target/release/bundle/nsis/`. To check what Windows itself can
see, ask through a service outside the session (`Get-CimInstance
CIM_DataFile`, `StdRegProv`), not with `Test-Path`.

An installed copy and a development copy use the same hotkey and count as
the same app, so the installed one must be quit (tray menu) before
`npm run tauri dev`, or the development copy just hands over to it. A copy
started from the session keeps its settings in the redirected storage; the
user's own copy keeps them in their real profile. The two do not share
settings.

When changing the app, bump the version in `package.json`,
`src-tauri/Cargo.toml` and `src-tauri/tauri.conf.json` together.

Rust (stable, MSVC) and the Microsoft C++ Build Tools are installed. A shell
opened before the install may not have `cargo` on its PATH; if so, prepend
`%USERPROFILE%\.cargo\bin`.

`README.md` is what the public will read. Keep it in step with the app: a
change to what the user sees or to what leaves the PC is a change to the
README too, and its example answers must come from the engine, not from
memory.

`docs/` holds the README's pictures. They come from the real app:
`tools/screenshots.cjs` says how to retake them. `.github/workflows/
release.yml` builds the installer on GitHub when a version tag is pushed
and leaves it on a draft release.

Already in the repo: `tests/reference/` (the reference problems; edit
`build.mjs` and regenerate, never edit `problems.json` by hand) and
`design/mockups.html` (window design options).

## Stack

- Tauri 2. Rust core in `src-tauri/`, web UI in `src/` (once scaffolded).
- Claude API over raw HTTPS from Rust. There is no official Anthropic Rust
  SDK, so do not invent one or guess its API. Check request shapes against
  the live Anthropic docs before writing or changing API code.
- Windows first.

## Commands

Run from the repo root unless noted.

- Run the app in development: `npm run tauri dev`
- Type-check the UI: `npx tsc --noEmit`
- Compile the Rust side only: `cargo build` (from `src-tauri/`)
- Run the Rust unit tests: `cargo test --lib` (from `src-tauri/`)
- Release build and installer: `npm run tauri build`. The installer lands in
  `src-tauri/target/release/bundle/nsis/`. Only one copy of the app can run,
  so quit the development copy before testing a release build.
- Regenerate the icons after changing `design/icon.svg`:
  `npx tauri icon design/icon.svg`, then delete the `android` and `ios`
  folders it creates in `src-tauri/icons/`.
- Regenerate the reference problems: `npm run reference`
- Run the reference set through a real model:
  `cargo build --example solve` (from `src-tauri/`), then
  `node tests/reference/run.mjs --model sonnet --effort low`. It uses the
  installed Claude Code and the user's plan, so it spends their usage: run
  it when the instructions or the engine's language change, not casually.
  Details of each run go to `tests/reference/results/` (not committed).

## Layout

- `src-tauri/src/lib.rs` — window placement, hotkey, tray, single instance.
- `src-tauri/src/settings.rs` — the one settings file. Add new settings as
  fields here; never write user content (problems, answers, history) to it.
  A new setting also needs a case in `Change` and `SettingsView` in
  `lib.rs`, a row in `index.html`, and a stand-in value in
  `src/dev-bridge.ts`. Significant figures reach the engine through
  `calc::with_sig_figs`, never through a global.
- `src-tauri/src/calc.rs` — the instant path: local maths with SI prefixes,
  electrical and mechanical units, conversions, and its unit tests. To add a
  unit, add one line to the `UNITS` table and a test. Check any new name
  against the prefix letters first: a unit called `mil` must not be read as
  milli-something.
- `src-tauri/src/solver.rs` — runs Claude's reply through the engine and
  produces what the window shows. `src-tauri/prompts/solver.md` is the
  instructions given to Claude; it documents the formula language, so
  change the two together.
- `src-tauri/src/claude_code.rs` — starts the user's own Claude Code, keeps
  one process ready, asks it one problem (with one follow-up if the working
  would not run), and stops it. It must never read or store a credential,
  and must leave Claude Code's sign-in alone: run the program as it is
  published, with its environment untouched (no variable added or removed).
  Anthropic's conditions for products that run Claude Code depend on this;
  see "Running Claude Code: the conditions the app keeps" in `SPEC.md`.
  Tools must stay switched off. Every
  way asking can fail ends in one plain line, what happened and what to do
  next (`explain` and the constants above it); add new ones there, with a
  test, and to the table under "Error states" in `SPEC.md`.
- `src-tauri/examples/solve.rs` — command-line front for the solver, used by
  the reference runner. `solve --ask` asks Claude one problem from standard
  input, which tests the connection without the window.
- `src/main.ts`, `src/styles.css`, `index.html` — the window UI.
- In `main.ts`, everything on screen is derived from the one `state` object
  by `render()`, and only `update()` calls `render()`. Change the state and
  call `update()`; never write to the page from anywhere else.
- The input is first in `index.html` so keyboard focus lands on it; CSS
  (`order`) puts it at the bottom. Keep it the first focus stop.
- The 12 px shell margin is defined in three places that must agree:
  `SHELL_MARGIN` in `lib.rs`, `SHELL_MARGIN` in `main.ts`, `--margin` in
  `styles.css`. The 660 px window height is likewise in `lib.rs`, `main.ts`
  and `tauri.conf.json`.
- **Never resize the OS window to fit content.** It is a fixed transparent
  rectangle clipped to the part in use (`clip` in `lib.rs`). Any change that
  alters the shell's height must go through `relayout()` in `main.ts`, which
  opens the clip, changes the page, then closes the clip to the new height.
- The window runs under a content security policy (`tauri.conf.json`): no
  inline `style="…"` or inline scripts, and nothing loaded from outside the
  app. The test browser does not apply it, so a breach only shows in the
  real app; `SPEC.md` ("Content security policy") says how it was tested
  without the desktop.
- Colours are CSS variables in `styles.css`. The theme follows Windows; the
  light set appears twice (media query and `data-theme`) and both copies
  must be kept in step. Orange is the accent and is never
  a fill; flags are yellow on a filled strip.

## Checking the window

The user works on this PC while the app is being built. Tests must not take
their keyboard, mouse, window focus or clipboard unless there is no other
way. Use these, in this order of preference:

1. **Unit tests** for the engine and solver: `cargo test --lib`.
2. **The command-line front** for Claude and the instant path:
   `solve --ask`, `solve --instant` (see `src-tauri/examples/solve.rs`).
   Claude Code started from a session signs in through the desktop app,
   not through the user's own login, so a pass here does not prove the
   installed app can reach Claude. To test sign-in as the installed app
   has it, remove every `CLAUDE*` and `ANTHROPIC*` variable from the
   command's environment first.
3. **The window in the built-in browser, with the dev bridge.** Start the
   `dev-bridge` and `window-ui` servers from `.claude/launch.json`, open
   `http://localhost:1420`, and set the viewport to 444×660. In a plain
   browser there is no native side, so `src/dev-bridge.ts` passes requests
   to `tools/dev-bridge.mjs`, which calls the real engine and the real
   Claude. Drive the page with scripted events inside the page (an `input`
   event, a `keydown`, a `paste` carrying a file). Nothing reaches the
   user's desktop. The bridge starts Claude Code fresh for each question,
   so its timings are slow and mean nothing.
4. **The real app on the desktop**, only for what the browser cannot show:
   the hotkey, focus, always-on-top, dragging, the clipped window, the tray.
   Tell the user first, keep it short, check the calculator has focus
   immediately before sending keys, and never put test data on the
   clipboard. The Claude desktop app takes focus back a second or two after
   the calculator is summoned.

Say plainly which behaviours were not checked.

## Rules that must not be broken

1. **The model never does arithmetic.** Every number shown to the user comes
   from code: the local evaluator (`calc.rs`) for plain arithmetic, or code
   execution for everything else. Do not add a path where the model's own
   arithmetic reaches the answer line.
   The local evaluator must stay strict: when it is unsure, it returns
   nothing. Never make it guess, and add a unit test for every new thing it
   accepts or refuses.
2. **Nothing new goes to disk.** Only settings and the memory notes file are
   written. No history, no logs of problems or answers, no caches of
   responses, no telemetry.
3. **The API key lives only in Windows Credential Manager.** Never in a file,
   an environment file committed to the repo, a log line or an error message.
4. **Never silently correct or silently learn.** Corrections follow the
   correction setting; memory notes are added only after the user clicks to
   confirm.
5. **Understand the user; do not standardise them.** Valid non-standard
   formulas and notation are accepted without comment. Only the three
   "definite error" conditions in `SPEC.md` produce a flag.
6. **Keep the UI minimal.** Do not add visible elements, settings or messages
   that are not in `SPEC.md` without asking.

## How to work

- Build in the slices listed in `SPEC.md`, in order, one at a time.
- A slice is done when it works in the running app, passes the reference set
  where that applies, and has not broken an earlier slice. Check in the real
  app, not only in tests.
- Keep changes small and scoped to the slice. No speculative features.
- When a decision is not covered by `SPEC.md`, ask the user. When it is
  decided, record it in `SPEC.md` in the same change.
- Commit only when the user asks.

## Testing prompts

Changes to the instructions or the answer format are behaviour changes.
Re-run the full reference set after any such change, including the
"must not flag" and "must block" cases, and report failures as they are.

The instructions have been tuned on the reference set, so a pass there is
weaker evidence than it looks. When adding a rule to fix a failure, write it
as a general rule, not as a patch for that problem, and say in the report
that the set was used for tuning. Prefer checking on problems the
instructions have not seen.

The failure that matters most is a wrong number shown with no warning. A
blocked or failed answer is an inconvenience; a silent wrong one is a
defect. Weigh fixes accordingly.
