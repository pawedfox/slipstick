# Slipstick — specification

Called Clawculator until version 0.9.0 (2026-11-20). See "Name" under
"Phase 2" for why it changed. Where this file describes things as they
were under the old name (its installers, its install folder, the name
check), the old name is left in place.

Status (2026-11-21, version 0.9.2): everything on the roadmap up to the
README is built, for the Claude Code route, and 0.9.2 is the first public
release (see "Release"). It added limits on Claude's reply (see "Solver").
Not built: the API-key route, signing, memory.
The roadmap was reordered on 2026-11-19 after a review from an engineering
technician's point of view (see "Roadmap").

## Purpose

The fastest engineering calculator on Windows, which can also reason when
asked. A tiny always-on-top window for electrical, mechanical and general
mechatronics work.

Two ways it answers:

- **Instantly, on the machine.** Arithmetic, engineering prefixes, units and
  conversions are answered as you type, with no account and no network.
  This is the part used dozens of times a day, so it comes first.
- **By reasoning, with Claude.** For problems that need a formula chosen or
  several steps worked through. Optional: the app is complete without it.
  Claude interprets the problem; real code computes the numbers.

Unofficial project. Not affiliated with or endorsed by Anthropic.

## Core flow

1. Summon (global hotkey or taskbar pin). Input is focused.
2. Type or paste. Plain maths and conversions answer as you type.
3. For anything else, press Enter to ask Claude (text, or a pasted
   screenshot).
4. The answer appears large, in engineering notation with units.
5. Click back into other work. The window stays on top with the answer visible.
6. Esc hides it to the tray.

## Design principle

Show the minimum the user needs to trust and use the number. Every extra line,
chip or control has to justify itself. When in doubt, leave it out or put it
behind the "show steps" caret.

## Settled decisions

| Area | Decision |
|---|---|
| Compute | Claude writes the formulas; the app's own engine computes every number. Claude replies with a short program of named steps and never writes a number it worked out. This replaced the earlier plan to use Anthropic's hosted code execution (2026-11-19): it works the same on both routes, needs one model turn instead of two, and a mistake in a formula shows up as a step that will not run, not as a plausible wrong answer. See "Solver". |
| Instant path | Plain arithmetic is answered locally as you type, with no network call. Claude is used only when the input needs reasoning. See "Instant path" below. |
| Access | Claude is optional. The app works fully with no account, and offers to connect Claude only when the user first presses Enter on something that needs it. Two routes: the user's own installed Claude Code on their plan (detected automatically), or their own API key, entered with guidance and stored in Windows Credential Manager. Never in the repo, never in a plain file. |
| Stack | Tauri 2. Rust core (API calls, key storage, window, hotkey, tray, memory file). Small web UI with KaTeX for formulas. |
| API client | Raw HTTPS from Rust (there is no official Anthropic Rust SDK). Streaming responses. |
| Model | Default: Sonnet 5.5 at low effort, chosen on accuracy (see "Solver"). A settings dropdown offers others. Haiku 5.5 is about 15 times cheaper but was not accurate enough to be the default. |
| Context | Stateless. Every Enter is a fresh problem. No follow-up context in v1. |
| History | Up and down arrows recall earlier inputs from the current session. Held in memory only; gone when the app quits; never written to disk. |
| Trust | Every answer from Claude shows one small line with the formula it used (`F = P × A, extend stroke`). Instant answers stay bare. The line is what lets a user check an AI answer without opening the steps, and it marks which answers came from AI. |
| Input | Text and pasted images (formula or schematic screenshots). |
| Steps | Generated in the same request as the answer, streamed behind a "show steps" caret. Each step is the maths on the left and a very short plain note on the right saying what was done ("divide", "put in R and C"). The note is one to five words, longer only when there is room. |
| Look | Terminal style: monospace text, orange for the prompt, focus and hover. The theme follows Windows (dark shell when Windows is dark, light when light), with a manual override in settings. Flags are yellow on a filled strip, so they are never confused with the orange accent. |
| Position | Bottom-right by default. The window can be dragged, and remembers where it was left. |
| Number format | 4 significant figures, engineering notation, SI prefixes. Complex results: rectangular, with polar in smaller text. Significant figures adjustable; per-problem override by saying so in the input. |
| Hotkey | Default Ctrl+Alt+C. Checked at every launch; if another program holds it, prompt for a new one. Simple "press a new shortcut" control in settings. |
| Background | Esc hides to tray; app keeps running. "Start with Windows" optional, off by default. Quit from the tray menu. |
| Copy | Click the answer, or Ctrl+C with nothing selected, copies a plain number with no unit. Nothing is copied automatically. Which number is set out under "Instant path". |
| Platform | Windows first. |
| Licence | MIT. Repo private until v1 works, then public. |

## Answer states

The UI shows exactly one of these per problem.

| State | When | What the user sees |
|---|---|---|
| Clean | Nothing to report | The answer only |
| Assumed | A value or condition was assumed (300 K, V_BE = 0.7 V, ideal op-amp…) | The answer plus one line naming the assumption |
| Flagged | A definite error in the user's formula, "Trust me" mode | The result of what was typed, plus an amber line with the standard form and its result |
| Corrected | A definite error, "Auto-correct" mode | The corrected result, plus a note of what was changed |
| Blocked | Ambiguous input, missing value, or units that cannot work | No number. One line saying what is needed. The input stays in the box for editing. |

### What counts as a definite error

Only these three:

1. The units do not balance.
2. The expression is not algebraically equivalent to any valid form of the
   relationship the user named.
3. The result is physically impossible.

Flagged or blocked: if a number can still be computed from what was typed, it
is flagged. If the expression cannot be evaluated at all (adding volts to
amps, a missing value), it is blocked.

"Assumed" means the app supplied a number or condition the user did not give
and which changes the result (a temperature, a V_BE, a β). The idealisations
built into a standard formula (an ideal op-amp in a gain formula) do not count.

Never an error: unusual symbols, valid rearrangements, common approximations,
personal shorthand. Example that must pass with no comment:
`V_ov = |VGS(off)| × √(ID ÷ IDSS)` (a correct rearrangement of the JFET
square law). The app's job is to understand the user, not to standardise them.

### Correction setting

One setting, two positions:

- **Trust me** (default): compute what was typed; flag definite errors only.
- **Auto-correct**: fix definite errors and show the corrected result as the
  main answer, with a note of the change.

A user can also ask for correction in a single problem ("fix my formula if
it's wrong").

Built in 0.7.0 as the "Wrong formulas" row in settings, with the positions
named **Flag** and **Correct**. It is done in the window, from what Claude
already sends for a flagged formula (the reason, the result as typed, and
the usual form's result), so the instructions to Claude did not change.
With Correct, the usual form's result is the main number and the yellow
line reads, for example, "Gives amps, not watts. Corrected to the usual
form. What you typed gives 1.500 A." It applies only where there is a usual
form to fall back on and a single answer; otherwise the formula is flagged
as before. Asking for correction inside one problem is not built.

## Memory

**Moved to after v1** (decided 2026-11-19). The design below stands for when
it is built.

On by default. One toggle turns it off.

- A short plain-text list of notes in one local file in the app's data folder.
- Four kinds of note: notation ("writes ohms as `ohm` or `R`"), preferred
  formula forms, default assumptions, and forms the user confirmed after a flag.
- The app asks before remembering: a small one-click chip under the answer,
  shown once per new thing. Ignoring it means "no". It never learns silently.
- Every note is viewable, editable and deletable in settings.
- A note can never suppress a definite-error flag.
- Notes are sent to Anthropic with each problem as context.

Memory holds preferences only. It never holds problems, answers or history.

## Privacy

- Written to disk: one settings file (`settings.json` in the app's config
  folder; it holds the window position, whether Claude is connected, and
  the choices made in settings), a
  copy of the solver instructions in the app's cache folder for Claude Code
  to read, and, once built, the
  memory notes file. The API key lives in Windows Credential Manager.
  Nothing else. Session history is never written to disk.
- No telemetry, no analytics, no history, no logging of problems or answers.
- Every problem (and the memory notes) is sent to Anthropic's API to be
  solved, under Anthropic's API data-retention terms. The README must say this
  plainly. (It does, in its "Privacy" section, worded for the Claude Code
  route: problems go to Anthropic through Claude Code, under the terms of
  the user's Claude account.)

## README

Written 2026-11-20 (`README.md`). It covers what the app does, with
examples; installing; using Claude; settings; privacy; building from
source; what is not done; the licence; and the "unofficial" line, at the
top and again at the end.

Every example answer in it was produced by the engine or seen in the app,
not written from memory. When the engine's output changes (a new default
for significant figures, say), the examples have to be redone.

It was written for the day the repo goes public. The three things in it
that waited on the user are settled, bar the last step:

- **The name.** Settled 2026-11-20: Slipstick (see "Name").
- **Using Claude.** The app keeps the Claude Code route, built to meet
  Anthropic's published conditions (see "Running Claude Code: the
  conditions the app keeps"). The README says what the app does and that
  the project cannot speak for Anthropic.
- **Releases.** It links to the repository's Releases page, which will
  hold nothing until the first release is published. See "Release".

**Pictures** (`docs/instant.png`, `flagged.png`, `steps.png`), added
2026-11-20. They are taken from the real app, not drawn: a build under a
test identifier is started hidden and its web view is asked for a picture
of the shell, at twice the size and with a transparent background, so it
sits on a light or a dark page. `tools/screenshots.cjs` does it and says
how to run it. Claude is asked for real, so the two Claude pictures change
a little each time they are retaken. Retake them when the window's look
changes.

## Release

Prepared 2026-11-20 for the user's approval. Nothing here has been made
public.

- **Checked from a clean copy:** the committed repository was cloned
  afresh; the window built and the 48 unit tests passed there, so nothing
  the build needs is missing from the repository.
- **Checked for personal details:** no real name, email address or local
  path in any tracked file. The commits carry the GitHub no-reply address.
- **The build** (`.github/workflows/release.yml`). Pushing a version tag
  (`v0.9.1`) has GitHub build the installer from the source on a Windows
  machine, run the unit tests, attach the installer and its SHA-256 to a
  **draft** release, and stop. A draft is not public; someone publishes it
  by hand. The tag has to match the version in `tauri.conf.json`.
  Its shape follows Tauri's guide for `tauri-apps/tauri-action@v1`, read
  the same day.
  - **Run for the first time 2026-11-20**, in the private repository, on
    the user's word: the tag `v0.9.1` was pushed and every step passed
    in 14 minutes. The draft release "Slipstick v0.9.1" holds
    `Slipstick_0.9.1_x64-setup.exe` (1.44 MB) and `SHA256SUMS.txt`, and
    the checksum in that file is the same one GitHub reports for the
    uploaded installer. The drafted release notes were then put on the
    draft. The repository was still private and the release still a draft
    afterwards.
  - **Tried by the user the same day:** the installer GitHub built was
    downloaded from the draft, installed over 0.9.0, and worked, for a
    plain sum and for a problem sent to Claude. Windows showed its
    "unrecognised app" warning first, as it will for everyone until the
    installer is signed.
- **Why GitHub builds it:** the installer is unsigned, so the only way for
  someone to trust it is that it was built from the public source where
  they can see the build, with a checksum to compare. It is also what
  SignPath's free signing for open-source projects requires.
- **The first public release is 0.9.2**, decided by the user 2026-11-21.
  0.9.1 was built and tried (above), but a security review the same day
  led to 0.9.2 (see "Solver") before anything was published for good, so
  0.9.1 has no release. The tag `v0.9.1` marks its source.
  - A first publication of 0.9.1 on 2026-11-20 was withdrawn the next day
    and the repository started afresh. An installer downloaded in that
    time is 0.9.1, without the limits 0.9.2 adds.
- **Release notes** are on the release and are not in the repo.
- **What is in the public repository.** Its history begins at the first
  public release: the working history before it is kept privately by the
  author. Working notes on the name, on Anthropic's terms and on
  correspondence about them are kept privately too (`private/`, which is
  not in the repository). `SPEC.md` and `CLAUDE.md` are public and are
  the engineering record.
- **Publishing a release:** push the version's tag, let the build finish,
  try the installer from the draft release, then publish the draft by
  hand. Each of these is done on the user's word.
- **Published 2026-11-21**, on the user's word, after the user installed
  the 0.9.2 installer from the draft and found it working. Checked
  without signing in: the repository page, the README's pictures and the
  latest release open, and the downloaded installer's SHA-256 is the one
  in `SHA256SUMS.txt`.
- **Signing: held, decided by the user 2026-11-21.** SignPath
  Foundation's [conditions](https://signpath.org/terms) and application
  form were read that day. The form requires evidence that the project is
  widely used or trusted (coverage, download figures, discussion), and
  their terms say programs people download and run need "a certain
  verifiable reputation". A project one day old has none, so the
  application waits until there is something to show.
  - Already met: an OSI licence (MIT), no proprietary part shipped (the
    app runs the user's own Claude Code and bundles none of it; they may
    ask about that), a public release described on its download page, a
    build GitHub makes from the tag, and the product name and version
    stamped on the app and the installer.
  - To do when applying: a "Code signing policy" section on the README
    and the release page (their wording for who signs, the team's roles,
    and a privacy statement), multi-factor sign-in on the GitHub and
    SignPath accounts, and a tried uninstall (the installer has an
    uninstaller; it has not been tried).
  - What it would mean: the certificate is issued to SignPath Foundation,
    which is the publisher Windows shows; every release is approved by
    hand in SignPath; the build would hand the installer to SignPath
    before attaching it to the release.
  - The conditions were a draft when read. Read them again before
    applying.
- Still to do: add the repository's topics.

## Content security policy

Set in 0.8.1 (`app.security.csp` in `src-tauri/tauri.conf.json`). The
window may load only what ships inside the app:

| What | Allowed from |
|---|---|
| Anything not listed | nowhere |
| Scripts | the app's own files |
| Styles | the app's own stylesheet; no inline styles |
| Pictures | the app's own files, and pasted pictures (`blob:`) |
| Requests | Tauri's own channel to the Rust side, and nothing else |
| Forms, frames, a changed base address | not allowed |

The window never talks to the internet itself: problems go out through
Claude Code, which the Rust side starts. So an outside address in the
window is always a fault, and the policy refuses it. Its use is as a second
line of defence: text from Claude is already only ever placed as text, and
if that were ever got wrong, an injected script still could not run or send
anything out.

Rules this puts on the window's code: no inline `style="…"` in the page or
set as an attribute (setting `element.style.height` from a script is
fine), no inline scripts, nothing loaded from an address outside the app.
A new kind of content needs its line added to the policy.

`devCsp` is the same policy with what the development server needs added
(inline styles and its live-reload connection).

Checked 2026-11-20 in the real app, without showing it: a test copy built
under a different identifier, so it could run beside the installed one,
was started with `--hidden` and its web view's debugging port, and driven
from a script (the script is not kept in the repo). Under the policy:

- the page loaded with no violations, styled, and Tauri's fast channel was
  used (it did not fall back);
- the instant path, the settings and the Claude status all answered;
- a pasted picture showed its thumbnail;
- an injected inline script did not run; a request and a picture from an
  outside address were refused; an inline style attribute was not applied.

Not checked: the development policy under `npm run tauri dev`, which needs
the desktop; and whether `eval` is refused, because the debugging port
runs code outside the policy, so that test proved nothing. (The policy
does not allow it.)

This way of testing is worth reusing: it reaches the real app's web view
without taking the keyboard, mouse or focus. A second tray icon shows
while the test copy runs.

## Window

- A 420 px wide shell at the bottom-right of the main screen, above the
  taskbar. About 77 px tall when empty, about 125 px with an answer (146 px
  when there is a note under it), and up to about 600 px with steps open. It
  grows upward.
- The answer line reads `= 338.6 Hz`, with the Steps control at its right
  end. The `=` and the input's `>` are orange and share one column, so the
  number sits directly above the text that produced it. What was typed is a
  step dimmer than the answer.
- The answer is set at 34 px. On the development laptop (15.4 inch,
  1920×1200, 100% scaling) that makes the digits about 4 mm tall, roughly
  the smallest size that reads at a glance from 60 cm (character height of
  about 0.007 × viewing distance). Do not go smaller without re-checking.
- Frameless, always on top, rounded corners.
- Layout: an outer rounded rectangle, a gap, and an inner rounded rectangle
  holding the input bar, with a `>` prompt. The layout came from design A in
  `design/mockups.html`; the colours and type were then changed to the dark
  terminal style, so the running app, not the mockup, is the reference.
- A minimise button (a short, thick orange line) at the top-right of the
  shell hides the window, the same as Esc.
- **The OS window never resizes.** It is a fixed 444×660 px transparent
  rectangle (the shell plus a 12 px margin for the shadow), clipped to the
  part in use. Resizing it from the top edge made the content flicker.
  Clicks above the shell pass through to whatever is underneath.
- **Taskbar icon.** The window sets its own large icon from the icon built
  into the program (since 0.5.1). That is harmless, but it was not the cause
  of the blank taskbar icon the user saw on 2026-11-19; see "The install on
  the dev machine was never real" under the roadmap. The earlier explanation
  here, a stale Windows icon cache, was wrong.
- On re-summon, typing replaces the previous input.
- **Moving it.** Drag the shell by any empty part of it (the top strip is
  the obvious handle). The position is kept in the settings file and used
  again next time, unless that spot is no longer on any screen, in which
  case it returns to the default corner. The tray menu has "Move back to
  the corner".
- **History.** Up and down arrows recall earlier inputs when the caret has
  nowhere else to go: at either end of the text, with everything selected,
  or mid-recall. An input is remembered when Enter is pressed, when the
  window is summoned again, or when it is typed or pasted over, provided it
  had a current answer. At most 100 entries, in memory only.

## Speed

The tool is only worth using if it is faster than the alternatives, so speed
is a requirement, not a nicety. Targets, from the standard response-time
limits (0.1 s feels instant, 1 s keeps the train of thought, 10 s is the
limit of attention):

- Something visibly responds within 0.1 s of pressing Enter.
- The answer line starts filling as soon as the first part arrives.
- No wait passes 10 s without visible progress and a way to cancel (Esc).
- The whole loop works from the keyboard, with no mouse.

Model response time has not been measured yet (needs the API key). It is the
largest unknown in the project.

Proposed, not yet decided by the user:

- Enter a second time, with the input unchanged, copies the answer.
- Ctrl+Enter types the answer into the app you came from and hides the
  window.

## Instant path

Built and tested (`src-tauri/src/calc.rs`, 17 groups of unit tests). Added
to v1 on 2026-11-19 and widened the same day to cover mechatronics work.

What it answers, as you type:

- **Prefixes:** `4.7k`, `100n`, `2.2u`, `1M`, `1meg`. Case matters: `m` is
  milli, `M` is mega. `K` is accepted as kilo.
- **Component notation:** `4k7`, `2n2`, `4R7`.
- **Units, with the dimensions checked:**
  - electrical: V, A, Ω (`ohm`), F, H, S, W, Hz, J, Wh;
  - magnetic: Wb, tesla (written out; a lone `T` is tera);
  - time: s, min, h;
  - length: m, cm, in, ft, thou; mass: g, lb, oz;
  - force: N, lbf, kgf; pressure: Pa, bar, mbar, psi, atm;
  - torque: N·m (`Nm`), lb·ft, lb·in, oz·in; power: W, hp, PS;
  - angle and rotation: rad, deg (`°`), rev, rpm;
  - volume and flow: L, gal (US), cc, L/min (`lpm`), gpm, cfm;
  - speed: mph, km/h, or any length over time.
  Units combine: `6 bar * pi * (25mm)^2` is 1.178 kN.
- **Conversions:** `3.5 bar to psi`, `12 L/min to gpm`, `2 in² to cm²`.
  `in` works as well as `to` where it cannot mean inches.
- **Temperature:** only as a direct conversion, `100 C to F`, because
  temperature scales have offsets.
- **Operators:** `+ - * / ^`, brackets, `||` for parallel, `√`, `×`, `÷`,
  `π`. Also `x` for multiply, `²` and `³`, and `%`.
- **Functions:** `sqrt ln log exp sin cos tan asin acos atan abs`. `log` is
  base 10. Trig takes radians or an angle (`sin(30 deg)`).

Rules:

- **It never guesses.** Anything it does not fully understand gives no
  instant answer and is left for Claude: words, variables, an equation,
  units outside the set, quantities that cannot be added, a result
  whose unit it cannot name, and ambiguous input such as `1/2pi` or
  `200 + 15%`.
- **A name in front of plain maths is a label** (decided by the user,
  2026-11-19, 0.6.1). `VGS = −0.3V × (1 − √(0.1mA / 0.2mA))` is answered
  instantly: −87.87 mV. The name is one word (`VGS`, `V_ov`, `R1`), with
  or without a tag in brackets (`VGS(off)`). It takes no part in the sum
  and is not shown again with the answer. What follows the `=` must be
  answerable by itself.
  - Anything else to the left of an `=` is an equation and is refused:
    `2x = 10`, `5V = 2*3`, `a = b = 6`.
  - Under a name, a value with a prefix and no unit is refused:
    `V = 2.2mA * 4.7k` goes to Claude. The `4.7k` there is a resistor
    with its unit left out, and read as typed the answer would come out in
    amps. Someone who names the answer is working with real quantities, so
    a missing unit needs understanding. Without a name, `2.2mA * 4.7k`
    is still answered as typed, as before.
- **Which unit the answer comes in:**
  1. the one asked for with `to`;
  2. otherwise the one typed, if it takes no prefixes (`3 bar * 2` is
     6 bar, `1500 rpm / 3` is 500 rpm);
  3. otherwise SI with an automatic prefix (`100 psi + 1 bar` is
     789.5 kPa).
- **A lone `m` after a number** is milli in electrical shorthand and metres
  in mechanical work. Both readings are tried. If only one makes sense it is
  used; if both do, the rest of the input decides (other bare prefixes or
  electrical units mean milli; mechanical units or a length asked for mean
  metres); if it still cannot tell, there is no answer. `mm`, `mA` and
  the like are never ambiguous.
- **rpm is an angular speed and Hz is not.** They differ by 2π, so
  `1500 rpm to Hz` is refused, while `10 N·m * 1500 rpm` correctly gives
  1.571 kW.
- **Energy and torque share dimensions.** A result is labelled J when the
  input used electrical, power or energy units, and N·m otherwise.
- **What copy gives:** the number as shown when the unit was asked for or
  typed (`1 in to mm` copies `25.4`); the full value in the unprefixed unit
  when a prefix was added automatically (`47.00 nF` copies `4.7e-8`).
- A bare number on its own gives no answer.
- Standard operator precedence applies. `1/2*pi` is (1/2)×π. A power on a
  unit belongs to the unit: `5 m^2` is five square metres.
- While the input is being edited and is not valid, the previous answer
  stays in place but dimmed, and cannot be copied. A dimmed answer is never
  current.
- Instant answers have no steps and no notes.
- A plain number between 0.1 and 1000 is shown without a prefix (`0.1250`,
  `338.6`); everything else uses engineering notation (`5.998 k`).
- A plain number in the thousandths is written out (`0.05000`), never given
  a lone `m`. Since lengths were added, "1.000 m" would read as one metre.
  Found when a magnetic-flux answer came back as "1.000 m" (2026-11-19).
- `gal` is the US gallon and `hp` is mechanical horsepower (745.7 W);
  metric horsepower is `PS`.

Not in the instant path: complex numbers, decibels, variables, the decimal
comma, temperature differences, conversions between rpm and Hz. Those go to
Claude.

## Solver

How a problem that needs reasoning is answered. Built: `src-tauri/src/solver.rs`,
the instructions in `src-tauri/prompts/solver.md`, and the program runner in
`calc.rs`.

1. The instant path is tried first. If it answers, Claude is never asked.
2. Otherwise the problem goes to Claude with the instructions. Tools are
   off: Claude can only reply with text.
3. Claude replies with JSON: a state (answer or blocked), a one-line
   formula, any assumptions, an optional flag, and the working as named
   steps. Each step is a formula in the engine's own language, and may use
   the names of earlier steps. A step can also be an equation to solve.
4. The engine runs the steps. Every number shown, including each step's
   value, comes from the engine. If a step will not run (units that do not
   balance, a name that was never defined), the result is "failed" with the
   step named, and no number is shown.

What the engine gained for this: named values, `solve` with a starting guess
(secant method), a unit to show a result in, and a label for plain numbers
such as dB. A value whose unit has no name (mA/V²) is shown in SI base units
so later steps can still use it.

Limits on a reply (0.9.2, from a security review on 2026-11-21). Claude's
reply can be steered by what is pasted into a problem, so the engine does
not trust its size. Anything over a limit is "failed", like any other
working that will not run, and no number is shown:

- A formula may be 1000 characters (`MAX_STEP_CHARS` in `calc.rs`). Typed
  input keeps its own limit of 400.
- Signs, brackets, roots and powers may nest about a hundred brackets deep
  (`MAX_DEPTH`), for typed input too. Deeper nesting used to run the
  thread out of stack, which ends the app.
- One run of a program may try 20,000 values across all its equations
  (`SOLVE_TRIES`).
- A reply may have 60 steps and 12 assumptions (`MAX_STEPS` and
  `MAX_ASSUMPTIONS` in `solver.rs`), because every assumption runs the
  whole program again.

The dev bridge (`tools/dev-bridge.mjs`) answers only the window's own page
at `http://localhost:1420`: it checks the request's origin and host. Before
0.9.2 any web page open on the PC could post to it and have Claude asked on
the user's plan.

Not possible in this design: anything the formula language cannot express
(matrices, integrals, lookup tables). Claude is told to say so. Complex
numbers are handled as separate real steps (real part, imaginary part,
magnitude, angle).

### Measured on the reference set, 2026-11-19

46 problems (the two memory problems are left out), asked through the
user's own Claude Code with tools off. Model time is the model's own time
per problem; starting Claude Code added about 4 s on top in this test, which
a process kept ready will remove.

| Model | Passed | Flags caught | Wrong number shown | Model time, median | Cost per problem |
|---|---|---|---|---|---|
| Sonnet 5.5, low effort | 46 of 46 | 4 of 4 | 0 | 1.1 s | about 0.7 cents |
| Haiku 5.5 | 39 and 42 of 46 on two runs | 1 of 4 | 1 (a gain 3% off) | 1.7 s | about 0.04 cents |

Cost is what Claude Code reported at API prices, including its own overhead.
No reply ever took more than one model turn, and replies averaged about 175
output tokens.

Caveats:

- **The instructions were tuned on these same problems.** Five rounds of
  fixes went in between the first run (29 of 46 on Haiku) and the last. The
  46 of 46 is therefore optimistic. It needs checking on problems the
  instructions have never seen; the user's own problems are the obvious set.
- Three reference problems were changed during this work because they asked
  for more than their wording did (two) or were ambiguous (one).
- Each model was run once or twice. Results vary between runs.
- Before the last fix, Sonnet silently replaced a wrong formula
  (`P = V / R`) with the right one. The instructions now forbid that in
  strong terms and it passed afterwards, but it is the failure to watch for.
- **A later change broke something that had passed.** After the
  instructions were extended for pictures and assumptions, Sonnet stopped
  flagging a physically impossible result (a collector at −14.5 V on a 9 V
  supply), five times out of five. "Never calculate" had been taken to mean
  "never estimate". The instructions now tell it to estimate roughly in its
  head as a sanity check while still writing no number down; the problem
  then passed three times out of three and the full set returned to 46 of
  46. The lesson: re-run the whole set after every change to the
  instructions, however unrelated the change looks.
- The same happened again when several answers and comparisons were added:
  one problem then failed because Claude named a step `x`, which the engine
  reserves for "multiply". No number was shown, only a failure. The
  instructions now list the reserved names, and the set returned to 46 of
  46. A failed step is the price of a strict engine; a second try, with the
  error shown to Claude, would hide most of them from the user and is still
  to do. (Built since: see "A second try" under "Claude in the app".)

## Claude in the app

Built for the Claude Code route: `src-tauri/src/claude_code.rs`, and the
window code in `src/main.ts`.

- **Nothing happens until the user agrees.** At launch the app only looks
  for an installed Claude Code. The first time Enter is pressed on something
  the instant path cannot answer, the window offers: "This needs Claude. Use
  the Claude Code on this PC? Problems you send this way go to Claude, on
  your plan." The button, or Enter again, accepts. The choice is kept in the
  settings file. The tray menu has "Disconnect Claude".
- **If Claude Code is not installed,** the window says so and how to fix it.
- **Slipstick never sees a credential.** It starts the user's own
  unmodified `claude` program, with every tool switched off, in an empty
  folder, with the solver instructions, and passes the problem on standard
  input.
- **Speed.** Starting Claude Code takes about 4 s, so a process is started
  ahead of time: when the user connects, and whenever they type something
  that is not plain maths. Each process answers one problem and is replaced,
  so problems never share context. A waiting process uses about 215 MB and
  is stopped 90 s after the window is hidden.
- **Waiting.** "Asking Claude" with moving dots appears at once. Esc stops
  the question (a second Esc hides the window). Editing the input also stops
  it. A question is abandoned after 60 s.
- **What an answer shows:** the number; any other values asked for
  (`V_c = 4.824 V`); the formula line; "Assumed …" if anything was; a yellow
  strip for a flag, with the usual form's result; and Steps. Each step shows
  the formula exactly as computed, a short note, and the value the engine
  got for it.
- **Blocked** shows what is needed. **Failed** (a step that would not run)
  says the working did not check out and shows no number.
- **A second try.** If Claude's working will not run, the app tells Claude
  exactly what went wrong and asks once more, in the same conversation, so
  the problem is not sent again. The second reply stands, whether or not it
  runs. The user only sees "failed" if both tries fail. A blocked reply is
  Claude declining, not a mistake, and is not retried.
- Text from Claude is only ever placed on the page as text, never as HTML.
- **Claude Code's own messages are not Claude's reply** (fixed in 0.6.1).
  When a request fails (a usage limit reached, a server error, no access to
  the model), Claude Code prints its own message in the same form as a
  reply from Claude. Up to 0.6.0 the app took that message for Claude's
  working, could not read it, and showed "Claude's working did not check
  out… Claude's reply could not be read", which sent the user looking in
  the wrong place. Such a message is now recognised (`is_error_message` in
  `claude_code.rs`) and shown as it is, after "Claude Code:". No second
  try is made for it.
- **Reversed in 0.8.2: Claude Code's sign-in is left alone.** The app no
  longer leaves the two key variables out. Anthropic's conditions for
  products that run Claude Code forbid restricting any of its ways of
  signing in (see "Running Claude Code: the conditions the app keeps"),
  and hiding a key variable does
  that. Claude Code now gets the environment it would have anyway, and
  signs in as the user set it up: with a plan, or with a key, which is then
  what is billed. The app looks only at whether a key variable is set, never
  at its value, so that when sign-in is refused it can say: "Claude Code was
  refused sign-in. It is using the key in ANTHROPIC_API_KEY on this PC.
  Correct or remove that variable, restart Slipstick, then try again."
  The restart is needed because a variable changed in Windows does not
  reach an app that is already running.
  - The offer now reads "…go to Claude, under your own Claude Code sign-in"
    where it said "on your plan", and the usage-limit and model messages no
    longer mention a plan.
  - This also means a separate API-key route is not needed for people who
    have Claude Code: they sign Claude Code in with a key.
  - Checked as the installed app runs it: with a refused key in
    `ANTHROPIC_API_KEY`, the message above; with no key variable, the user's own sign-in answered;
    signed out with no key variable, the not-signed-in message.
  - What follows is the 0.7.1 account, kept as the record of how the
    fault was found.
- **An API key in the environment is not handed to Claude Code** (0.7.1 to
  0.8.1). Claude Code uses `ANTHROPIC_API_KEY` or `ANTHROPIC_AUTH_TOKEN`
  from the environment in place of the user's plan login. This route is
  offered as "on your plan", so a real key there would be billed without
  the user knowing, and a stale one fails. The app now starts Claude Code
  with both variables left out. It never reads their values.
  - Found 2026-11-19 in the real 0.7.0 install: every problem sent to
    Claude gave "Claude Code: Invalid API key · Fix external API key". The
    PC had `ANTHROPIC_API_KEY` set to a value that was not a working key.
  - This is almost certainly also what the "reply could not be read"
    failure in 0.6.0 was, before the app could show Claude Code's own
    message.
  - **Commands run from a Claude desktop session do not show this.** Claude
    Code started from a session signs in through the desktop app and
    ignores the variable, so every earlier test of this route from a
    session passed while the installed app could not reach Claude at all.
    To test as the installed app does, start the command with every
    `CLAUDE*` and `ANTHROPIC*` variable removed from its environment.
    Done that way: with such a variable set, the old build gave the same
    message the user saw and the new build answered the user's problem;
    with no key, the user's own login answered.
  - **Confirmed by the user in the installed 0.7.1, 2026-11-20:** with the
    variable still set, `P = V / R, V = 12 V, R = 8 ohm` came back
    flagged, 1.500 A as typed and 18.00 W for the usual form. This is the
    first answer from Claude in a real install.

Checked in the running app, 2026-11-19: the offer; Enter accepting it; the
waiting line; an answer with a second value and a formula line (cylinder
force, 1.178 kN); a flagged formula with the usual form's result; a
five-step answer with the steps open (BJT bias, 888.4 µA and 4.824 V, matching
the reference set); Esc stopping a question without hiding the window; and
that a finished Claude Code process exits, leaving one waiting.

Version 0.2.0 was then built, installed over the old copy and started from
the Start menu. In the installed copy the instant path answered
`10 Nm * 1500 rpm`, and Claude answered a problem it had not seen (voltage
drop over 30 m of 1.5 mm² copper at 10 A, there and back): 6.880 V, with
the loop resistance, the formula line, and the resistivity it assumed.

Rough edges seen there, not yet fixed: a value with no named unit shows
"(SI)" (resistivity in Ω·m); names like `rho_cu` are not turned into Greek
letters; and unit text inside an assumption is not tidied.

Not checked: "Disconnect Claude" in the tray; a PC without Claude Code; the
light theme with the new elements; how much the head start while typing
saves. (The 60 s limit and Claude Code not signed in were checked later:
see "Error states".)

Second try, checked from the command line (2026-11-19): with the first
reply deliberately treated as failed, the same Claude Code process returned
a second, correct reply (338.6 Hz), which shows the second turn is read
properly. An ordinary problem made no second try. I could not provoke a
real failure to watch it recover: three attempts to trip the engine were
all answered correctly first time. The forced second try took about 6.5 s
on top of the first reply, so a rescued answer is slower than a normal one.

Still to do here: the API-key route; copying a secondary value.

## Several answers, and comparisons

Built 2026-11-19 after the user's first real problems showed that one big
number with small lines under it reads badly when two things were asked for.

- **One answer:** as before, one big number.
- **Several answers of equal standing** ("the current for D1 and D2", "find
  VGS, then ID"): a row for each, all the same size, each with its name.
  There is no single big number. Clicking a row copies that row's number.
- **A comparison** ("the difference in current, D2 versus D1"): the two
  values as quieter rows, then the big number, which is the size of the
  difference, then a line saying which is larger and by how much
  ("I_D2 is 33.3% lower than I_D1").
- Claude only says which steps are the answers, or which two are compared.
  The difference and the percentage are worked out by the engine from two
  extra steps it adds itself, so the units are checked like everything else.
  If the two cannot be subtracted, they are shown as a list.
- Supporting values the user did not ask for still go in small lines under
  the answer.

Checked in the browser copy of the window, with the user's three problems
typed as words: two LED circuits gave rows of 13.64 mA and 9.091 mA; the
difference between them gave 4.545 mA with "33.3% lower"; and the two-part
JFET problem gave V_GS = −0.4700 V and I_D = 0.08508 mA. All match hand
calculation. Not checked in the real app.

## Sending a problem

- When Enter sends a problem to Claude, the selection is cleared, a long
  problem scrolls back to its first line, and the text and the bar's border
  go quiet until the answer arrives. Together with "Asking Claude" this
  shows it has gone.
- After a problem has been sent, typing or pasting starts a new problem in
  its place. An editing key (arrows, Home, End, Backspace, Delete) or a
  click in the text means the old one is being changed instead.
- The input's scrollbar is thin, in the app's colours, and has its own lane
  beside the text, so it never covers any.
- **Enter straight after typing** (fixed in 0.9.1). The instant path looks
  at each change to the text a few thousandths of a second after it is
  made. Enter pressed inside that gap used to be judged against the answer
  to the previous text: if that answer was still current, the app took the
  new problem as already answered and did nothing. Enter now waits for the
  instant path to catch up with what is in the box before deciding. Found
  while taking the README's pictures, where a script types and presses
  Enter with no pause; a fast paste-and-Enter would have hit it too.
  Checked in the real app by that same script.

## Pictures

Built 2026-11-19. In the installed copy from version 0.3.0.

- Paste a picture into the input (a screenshot of a formula, a datasheet
  row, a small schematic). A thumbnail appears in a row above the text and
  the placeholder changes to "Add a note, or press Enter".
- **Several pictures** (from 0.4.0): paste again to add another, up to four.
  They are sent together, in the order pasted, as one problem: a schematic
  and the datasheet values for its parts, say.
- **Full size** (from 0.4.0): click a thumbnail to see the picture at full
  size in the space above the answer. Click it again, click the picture, or
  press Esc to close it. It shares that space with the steps.
- Enter sends the pictures to Claude. Typed words go with them and say what
  is wanted; with no words Claude answers what they most plainly ask.
- With a picture attached the instant path stands aside: the words are for
  Claude.
- To remove one: click the small cross on its thumbnail. Backspace in an
  empty box removes the last one. Typing or pasting text over the whole
  input drops them all, since they belonged to the old problem.
- Pictures over 5 MB are refused. Pictures are not kept in history.
- Claude is told to use exactly the numbers shown and to block, saying what
  it could not read, if anything it needs is unclear.

Checked without touching the desktop: from the command line, a picture of
`f = 1/(2πRC)` with R = 6.8 kΩ and C = 33 nF and no words gave 709.2 Hz in
about a second. In the window (loaded in a browser with the dev bridge), a
scripted paste showed the thumbnail, Enter gave 709.2 Hz with the formula
line, the same picture with "what is the time constant instead" gave
224.4 µs, and Backspace removed the picture.

Confirmed by the user in the installed copy (0.3.0): a three-transistor
schematic pasted from the Windows clipboard with "Find the voltage and
current at analog out" gave 1.428 V, with the collector current and both
resistor currents, the formula line, and the assumptions (V_BE = 0.7 V,
β = 100, output unloaded, DC only). The figures were re-worked by hand and
are right for those assumptions.

What that case showed was missing: the answer depends heavily on the assumed
β. At β = 150 the same circuit gives about 0.23 V, and at 200 the transistor
saturates. The window named the assumption but gave no sign that the result
hangs on it. That led to "Assumptions the answer hangs on", below.

Checked for 0.4.0 without touching the desktop: from the command line, two
pictures (one with `F = P × A`, one with a 63 mm bore and 5.5 bar) and no
words gave 1714 N. In the browser copy of the window, two pasted pictures
showed two thumbnails, clicking one showed it at full size, Esc closed it,
Enter gave 1.714 kN, the cross removed one and dimmed the answer, and
Backspace removed the other.

Not checked: several pictures or the full-size view in the real app; a
photograph or a blurry picture; the 5 MB and four-picture limits.

## Assumptions the answer hangs on

Built 2026-11-19, for 0.4.0.

- Claude gives each assumed number its own step and names that step in the
  assumption. The engine then changes that step by 10%, runs the whole
  working again, and measures how far the answer moves.
- An assumption is called out when that movement is 15% or more, or when
  Claude marks it as a value that varies widely in practice (a transistor's
  gain is the usual case, since it can differ by a factor of two between
  parts).
- Ordinary assumptions stay on the one quiet "Assumed …" line. One the
  answer hangs on gets its own line in the warning colour: "Depends heavily
  on the assumed β = 100: a 10% error moves the answer 17%." The figure is
  the engine's measurement, not Claude's opinion.
- It does not use the filled yellow strip. That stays for a definite error
  in the user's own formula.
- A condition with no number ("output unloaded") cannot be measured and is
  never called out.

Checked: a unit test of the user's output stage (gain moves the answer 17%
and is called out; the base-emitter drop moves it 1% and is not). From the
command line, the user's own schematic came back with β marked. In the
browser copy of the window, the same circuit described in words showed the
warning line.

Limit: the test is a small 10% nudge around the assumed value. It will not
show that a larger error would push the circuit into a different regime
(saturation, in that example).

## Error states

Each needs a one-line, plain message and a next step:
no API key set · offline · rate-limited · request declined by the model ·
hotkey already taken · code execution failed.

Built in 0.8.0 for the Claude Code route. That list was written for the
API-key route and hosted code execution; this is what each became. Every
message says what happened and then what to do. After any of them, Enter
asks again.

| What went wrong | What the window says |
|---|---|
| Claude Code is not installed | This needs Claude, and Claude Code was not found on this PC. Install it and sign in, then press Enter again. |
| Not signed in, or the sign-in is refused | Claude Code is not signed in. Open a terminal, run claude and sign in, then press Enter here to try again. |
| Sign-in refused while a key variable is set | Claude Code was refused sign-in. It is using the key in *ANTHROPIC_API_KEY* on this PC. Correct or remove that variable, restart Slipstick, then try again. |
| No connection | Claude could not be reached. Check your internet connection, then press Enter to try again. |
| A usage limit | Your Claude usage limit has been reached (*Claude Code's own words, which give the reset time*). Plain maths still works. |
| The service is overloaded or failing | Claude is busy or having trouble right now. Press Enter to try again in a moment. |
| The chosen model is not available | That model is not available to your Claude Code. Choose the other one under Model in settings (Ctrl+,), then press Enter. |
| An account problem (billing, on hold, verification) | Your Claude account needs attention before it can be used here. Claude Code says: *its words*. |
| The request is refused as invalid | Claude could not take that problem. Claude Code says: *its words*. |
| The reply is cut off | Claude's reply was cut short. Try a shorter problem, or split it in two. |
| No answer in 60 s | Claude did not answer within a minute. Press Enter to try again. |
| Claude Code will not start | Claude Code would not start. Check that claude runs in a terminal, then press Enter here to try again. |
| Claude Code stops with nothing said | Claude Code stopped without answering. Press Enter to try again. |
| Anything Claude Code reports that is not in this list | Claude Code reported a problem: *its words*. Press Enter to try again. |
| Claude's working will not run, after the second try | Claude's working did not check out, so no answer is shown. *Which step, and why.* Press Enter to ask again, or reword the problem. |
| The shortcut is held by another program | *Ctrl+Alt+C* is in use by another program, so the shortcut is off. Choose another in settings (Ctrl+,). |

How it works (`claude_code.rs`):

- Claude Code names each failure (`authentication_failed`, `rate_limit`,
  `overloaded`, `model_not_found` and so on; the list was read out of
  Claude Code 2.1.325). `explain` turns the name into the message. A name
  it does not know gets Claude Code's own words, cut to 140 characters.
- **No connection.** Claude Code says nothing while it cannot connect: it
  tries again by itself, ten times over several minutes. Before 0.8.0 that
  ended as "Stopped." after 60 s. It does announce each new try, and a try
  with no status means the request never arrived, so the app gives up at
  the third such try, about three seconds in. When the service answers but
  is overloaded, it waits for five tries, about ten seconds.
- After any failure the Claude Code process is ended at once, so none is
  left trying in the background.
- **Installed while the app is running.** Claude Code is looked for again
  each time it is needed and not yet found, on the PATH and in the two
  folders its installers use, so "then press Enter again" is true without
  restarting the app.
- A request the model declines has no state of its own. It reaches the app
  as a reply with no working in it, so after the second try it is shown as
  working that did not check out.

Checked 2026-11-20, each asked for real through the command-line front,
started as the installed app starts it (none of the session's variables):
not signed in (an empty sign-in folder); no connection (an address nothing
listens on; the message came about three seconds after asking); a stand-in
Claude Code that never answers (the message came at 60 s, and nothing was
left running); and one that stops at once. The messages for the other
kinds are covered by unit tests, written from the shape of the real
not-signed-in and wrong-key messages. In the window, in the built-in
browser: four of the messages, to see how they sit (two or three lines).

Not checked: a real usage limit, a real overload, and a model missing from
the plan, none of which can be brought about on purpose; and a PC with no
Claude Code at all.

## Settings (complete list for v1)

Claude connection (Claude Code or API key) · model · correction mode ·
significant figures · hotkey · start with Windows · theme (follow Windows,
dark or light).

Built in 0.7.0, all but the API-key route.

- **Opening it:** the small gear beside the minimise line, Ctrl+comma, or
  "Settings" in the tray menu. It opens in the space above the input, in
  place of the answer. Esc closes it; so does typing a problem, or
  summoning the window again. The gear is grey until pointed at.
- **Rows**, a name on the left and its choices on the right. The choice in
  force is bright with an orange line under it. A box marks a control that
  does something when clicked.
  - **Shortcut.** Click the box, then press the new keys. It needs two of
    Ctrl, Alt and Shift (one of them Ctrl or Alt) with a key, because a
    global shortcut takes its keys away from every other program: Ctrl+C
    would break copying everywhere. If another program already holds the
    new one, the old one stays and the screen says so. While the screen
    waits for the keys the current shortcut is switched off, so pressing it
    is read, not acted on. Esc, or leaving the window, cancels.
  - **Start with Windows.** Off or On. Windows keeps this itself (the
    per-user "Run" list), through `tauri-plugin-autostart` on its 2.x
    line; it is not in the settings file. Started this way the app gets
    `--hidden` and waits in the tray.
  - **Theme.** Windows (follow it), Dark or Light.
  - **Significant figures.** 3 to 6. Applies to the instant path and to
    answers from Claude, to what is shown and to what is copied. Values
    carried between steps are never rounded. An instant answer on screen
    is redone at once; an answer from Claude stays as it was until the
    problem is sent again.
  - **Claude.** "Use Claude Code" with the line "Sent problems go to Claude
    under your sign-in", or "Claude Code" and "Disconnect". If Claude
    Code is not installed it says so.
  - **Model**, shown only when Claude is connected. Fast (Sonnet, low
    effort) or Deeper (Opus, medium effort). Deeper has not been run
    against the reference set; one problem asked through it answered
    correctly in about 3 s.
  - **Wrong formulas**, shown only when Claude is connected. Flag or
    Correct: see "Correction setting".
- When the shortcut is held by another program at launch, the standing
  message now ends "Choose another in settings (Ctrl+,)."

Checked in the window in the built-in browser, 2026-11-19: opening by the
gear and by Ctrl+comma; every row; the shortcut being read, refused for
too few modifiers, refused as taken, and cancelled with Esc; both themes;
figures changing an instant answer on screen (338.6 Hz to 338.628 Hz);
the Claude rows appearing on connecting; a wrong power formula shown
corrected (18.00 W) through the real Claude; typing closing the screen;
the order Tab moves through the controls. From the command line: five
figures on an answer from Claude, and the Deeper model.

**Checked by the user in the installed 0.7.0, 2026-11-19:** changing the
shortcut and using the new one; the current shortcut being read, not acted
on, while a new one is awaited; "Start with Windows" (in the tray with no
window after signing in again); "Settings" in the tray menu; and the
Claude connection and window position surviving the update.

The Model and Wrong formulas rows were checked by the user in the installed
0.7.1 the next day: everything works, and switching between Correct and
Flag changes the answer as described.

## Out of scope for v1

- Follow-up context between problems (possible later, optional).
- macOS and Linux builds.
- History saved to disk. (Session history in memory is in scope.)
- Memory of preferences (designed above; built after v1).

## Phase 2 — research and stress-test (in progress)

Findings so far, 2026-11-19.

### Done

- [x] **Name check.** Not free. `echoudhry/clawculator` exists on GitHub (an
      unrelated JavaScript cost tool for another AI project, 4 stars, last
      updated March 2026) and an npm package named `clawculator` exists.
      crates.io has no crate of that name. Decision needed from the user:
      keep the name, or change it before going public.
      - Looked at again 2026-11-20. The other project is a cost tool for
        OpenClaw (npm 2.9.1, last updated March 2026).
- [x] **Name, decided by the user 2026-11-20: Slipstick.** An old
      engineers' word for a slide rule. No calculator uses it, and it
      leans on nobody else's name.
      - Checked the same day: four small GitHub repositories carry the
        name, none of them a calculator; it is free on npm and crates.io;
        a web search found no calculator or Windows app called Slipstick.
      - One other use to know about: Slipstick Systems (slipstick.com), a
        long-running help site for Microsoft Outlook. A different field.
        For that reason the app's identifier does not use `com.slipstick`:
        it is `io.github.pawedfox.slipstick`, after where the code lives.
      - What changed with the name (0.9.0): the product name, the program
        file (`slipstick.exe`), the installer (`Slipstick_<version>_x64-
        setup.exe`), the install folder (`%LOCALAPPDATA%\Slipstick`), the
        identifier, the tray and window titles, the messages, the licence
        line and the docs.
      - **Windows sees a new app.** It installs beside the old one and
        does not replace it, so the old Clawculator has to be quit and
        uninstalled by hand, and a taskbar pin made again.
      - **Settings are carried over once.** The settings file lives in a
        folder named after the identifier. On its first start under the
        new name, if it has no settings file of its own, the app reads the
        old one (`com.clawculator.desktop`) and saves it as its own. "Start
        with Windows" is kept by Windows under the app's name and is not
        carried over; it has to be switched on again.
      - Checked 2026-11-20 in the real 0.9.0 build, started hidden with
        its web view's debugging port (as for the security policy), in
        the session's own storage with a stand-in old settings file: the
        app took the old file's shortcut, theme, figures, model and
        correction setting, applied them (light theme on the page, `1/3`
        to six figures), wrote its own settings file, showed the title
        "Slipstick", and loaded with no policy violations. One lesson for
        this way of testing: wait until the web view has left its empty
        first page before reloading it, or the app's own page never loads.
      - **Done by the user the same day, in the real install:** the old
        Clawculator quit and uninstalled, Slipstick 0.9.0 installed, and
        it worked. This is the first time uninstalling was tried. The user
        reported it working as a whole; which settings carried over was
        not gone through one by one.
      - Not changed: the icon (two slanted strokes forming an equals sign;
        drawn as claw marks, but it reads as an equals sign), the orange
        accent, and the default shortcut Ctrl+Alt+C.
- [x] **Hotkey, local check.** Ctrl+Alt+C is free on the dev machine. KiCad's
      hotkey file has no Ctrl+Alt+C binding; there are no custom REAPER or
      VS Code key files. REAPER's built-in defaults were not checked.
      Ctrl+Alt+Space is already held by a running program.
- [x] **Reference set drafted.** 48 problems in `tests/reference/`, every
      expected number computed by `build.mjs` (33 answer, 5 must-not-flag,
      4 must-flag, 4 must-block, 2 memory). Still wanted: 5–10 of the user's
      own problems, pasted exactly as they would type them.
- [x] **Window mockups.** `design/mockups.html`: three directions (Datasheet,
      Enamel, Graphite), eight states each. User picks one.

- [x] **Toolchain installed.** Rust 1.100.0 (stable, MSVC) via rustup, and
      Visual Studio Build Tools 2022 with the C++ workload. A test program
      compiled, linked and ran.

- [x] **Design chosen.** Layout A (answer on the left, flags as a highlighted
      strip), then restyled after the first hands-on try: dark terminal look,
      a minimise button, and a short note beside each step.
- [x] **Window shell built and tested (slice 1).** Tauri 2.12, vanilla
      TypeScript. Measured on the dev machine (1920×1200, 100% scaling) by
      reading the window's state from Windows and driving it with real key
      presses and a real mouse click:
      - docked flush to the bottom-right of the work area, above the taskbar;
      - always on top; frameless with transparent rounded corners;
      - Ctrl+Alt+C summons and focuses it, and dismisses it when it has focus;
      - Esc hides it, and so does a click on the minimise button; the process
        keeps running;
      - the OS window stays at 444×660 while the part in use changes (105 px
        empty, 198 px with an answer, 485 px with steps open), and screen
        points above the shell belong to the windows underneath;
      - launching the exe a second time summons the running window instead of
        starting another copy (tested before the restyle).
      The user confirmed by eye that the steps open without flicker.
- [x] **Instant path built and tested.** Checked in the running app with
      real key presses: `1/(2*pi*4.7kohm*100nF)` shows 338.6 Hz,
      `2.5V*sqrt(1.2mA/6mA)` shows 1.118 V, an unfinished expression dims the
      previous answer, and input that is not arithmetic shows "This needs
      Claude, which is not connected yet." on Enter. Typing `demo` and
      pressing Enter shows a labelled sample answer with steps.
- [x] **Keyboard focus fixed.** Focus could land on the minimise button, so
      typing went nowhere and Enter hid the window. The input is now the
      first focus stop and takes focus whenever the window becomes active.

### Read from documentation, not yet tested with a live request

Note: the hosted code-execution points below are no longer part of the
design (see "Solver"). The rest still applies to the API-key route.

- Endpoint `POST https://api.anthropic.com/v1/messages`, headers `x-api-key`,
  `anthropic-version: 2026-10-21`, `content-type: application/json`.
- Code execution is declared as a tool with no schema
  (`{"type": "code_execution_20260521", "name": "code_execution"}`). The
  sandbox is Python with numpy, scipy, sympy and mpmath, and has no internet
  access. Results arrive as `bash_code_execution_tool_result` blocks.
- Code execution is billed by container time with a large free monthly
  allowance, so for one person it should add nothing to the token cost.
- Structured output is `output_config.format` with a JSON schema; the docs
  list it as working with streaming and with thinking.
- On `claude-opus-5-5`: thinking cannot be turned off; depth is set with
  `output_config.effort`; forcing a specific tool is rejected.
- A declined request comes back as HTTP 200 with `stop_reason: "refusal"`,
  so the stop reason must be checked before reading the answer.

### Running Claude Code: the conditions the app keeps

Slipstick answers harder problems by starting the user's own installed
Claude Code in non-interactive mode and passing it one problem at a time.
Anthropic's [Legal and compliance page for Claude Code](https://code.claude.com/docs/en/legal-and-compliance)
sets conditions for products that run Claude Code. As read on 2026-11-20,
in paraphrase, with how the app keeps each:

| Condition | How the app keeps it |
|---|---|
| Claude Code is not modified and is run as Anthropic publishes it. No built-in way of signing in is removed, disabled or restricted. | The app starts the `claude` the user installed, with published options only, and hands on the environment untouched (since 0.8.2). It ships no copy of Claude Code. |
| The product does not pay for, resell or stand between users and their usage. Each user signs in with their own credentials and is billed directly. | The user signs in to Claude Code themselves, with a Claude plan or an API key, and that is what is billed. The app never sees, stores or handles a credential, and pays for nothing. |
| A product may say in plain words that it runs Claude Code. It may not use the Claude Code or Anthropic names or logos in its own name, or suggest endorsement. | The README says the app runs Claude Code and that the project is unofficial. Neither name is in the app's name. |
| The author agrees to Anthropic's Commercial Terms. | Done by the author. |

- Use is one question at a time, asked by a person.
- Not allowed, and not done: calling the API with a plan's token
  (`claude setup-token` is for Claude Code alone), offering Claude sign-in
  inside the app, or holding any credential.
- These conditions are Anthropic's and may change; how such tools are
  treated changed more than once during 2026. Read the page again before
  changing how Claude Code is started, signed in or billed. This project
  cannot speak for Anthropic and is not approved or endorsed by it.
- **A plugin, an MCP server or a channel in its place** (looked at
  2026-11-20):
  - A Claude Code plugin can carry skills, hooks and an MCP server, and can
    be given out from any git repository. Slipstick's engine could be
    such a server, so that Claude, in the user's own session, works numbers
    out with it. But it turns the app round: the problem is typed into
    Claude, not into the calculator. It is a second product, not a
    different way to connect this one.
  - An MCP server cannot ask Claude a question: Claude Code's pages do not
    document that part of the protocol ("sampling").
  - A channel is the one documented way for another program to put a
    message into a running Claude Code session and get the reply. It is a
    research preview; a channel that is not on Anthropic's own list needs
    `--dangerously-load-development-channels` in an interactive session,
    and it does not work with `-p`. The user would have to keep a terminal
    session open with that flag. Not usable for a calculator given to other
    people.
- Timing, one BJT bias problem, no tools, model doing its own arithmetic
  (so not yet a valid answer path):
  - Starting Claude Code for each question adds about 3.7 s before the model
    even begins (about 5 s wall time for a one-word reply).
  - With the process already running and waiting: Haiku 5.5 about 3.0 s,
    Sonnet 5.5 about 3.8 s, Opus 5.5 at medium effort about 9 s.
  - So this route needs a process kept ready in the background, replaced
    with a fresh one after each answer to stay stateless.
- Consequences: use counts against the user's plan limits or key; Claude
  Code must be installed and signed in; there is no hosted code execution,
  so the numbers must come from a local, safe maths tool (never arbitrary
  code, since the input is pasted text).

### Cost per problem, measured 2026-11-19

Two real problems (an easy BJT bias, a harder iterative diode loop), output
tokens measured, priced at the published rates as of 2026-11-03. The low
figure assumes a direct API call with a short prompt; the high figure is
what Claude Code reported for the same problems including its own overhead.
Neither includes the calculation step, which is not built yet.

| Model | Output tokens (easy / hard) | Cost per problem | Problems per $5 |
|---|---|---|---|
| Haiku 5.5 | 731 / 3026 | $0.0005–0.002 | about 2,500–10,000 |
| Sonnet 5.5 | 447 / 1100 | $0.0075–0.023 | about 200–650 |
| Opus 5.5, medium effort | 850 / 1683 | $0.023–0.058 | about 85–220 |

Plain arithmetic costs nothing: it never leaves the machine. Accuracy by
model has not been compared yet; that comparison, not price alone, decides
the default.

**Decided by the user 2026-11-19: build both routes** (the user's own Claude
Code on their plan, and an API key).

### Still open

- [ ] **Does structured output combine with code execution in one streamed
      request?** The docs do not say either way. Needs a live test. If not,
      fall back to a strict "report answer" tool or a plain-text format.
- [ ] Whether reusing a code-execution container between problems is faster,
      and whether that fits the "stateless" rule (containers keep files).
- [ ] Time and cost real problems on Opus 5.5, Sonnet 5.5 and Haiku 5.5.
      Pick the default from the numbers. **Blocked: needs the API key.**
- [ ] In the window shell, not yet checked by hand: the tray icon (left
      click to show, right click for Show / Quit), copying the answer, the
      light theme, displays scaled above 100%, and whether a launch from the taskbar
      pin takes keyboard focus (a launch from a background process does not;
      the hotkey does).
- [x] Before release: set a content security policy (done in 0.8.1, see
      "Content security policy"), and replace the template icons with an
      original one (done earlier: `design/icon.svg`).
- [ ] REAPER default shortcuts for Ctrl+Alt+C.

## In flight

Written 2026-11-19 so that nothing is lost if the conversation is reset.
Remove each item when it is done.

### The failure the user hit in the real 0.6.0 install (fixed in 0.6.1)

Input: `VGS = −0.3V × (1 − √(0.1mA / 0.2mA))`. The window showed "Claude's
working did not check out, so no answer is shown. Claude's reply could not
be read." The right answer is −87.87 mV.

What was found (2026-11-19):

- Claude answers this problem correctly. Asked five times from the command
  line (`solve --ask --show-replies`, which prints Claude's own replies),
  it gave −87.87 mV every time, first try.
- The app had a real fault that produces exactly that message: it took
  Claude Code's own error messages for Claude's reply. See "Claude Code's
  own messages are not Claude's reply" under "Claude in the app". The shape
  of such a message was captured by asking for a model that does not exist.
- **The cause, found the next time it happened (0.7.0):** an
  `ANTHROPIC_API_KEY` in the environment that was not a working key,
  which Claude Code used in place of the user's login. See "An API key in the environment is not
  handed to Claude Code" under "Claude in the app". The earlier guess here,
  a usage limit or a server error, was wrong.
- The input no longer needs Claude at all: see "A name in front of plain
  maths is a label" under "Instant path".
- Checked: unit tests for both changes; the input in the window in the
  built-in browser (answers as typed); every reference problem through the
  instant path alone, which answers the same two as before, so the
  reference results are unchanged. The instructions to Claude were not
  changed, so the set was not re-run through the model.
- Not checked: a real usage-limit or server-error message in the installed
  app. Only the unknown-model message was seen for real.

## Roadmap

Reordered 2026-11-19 after reviewing the app as an engineering technician
would use it. The findings that drove the change:

- The everyday value is the instant path, which needs no setup and already
  beats a desktop or handheld calculator. The AI is used a few times a day,
  is waited for longest and is trusted least.
- Setup was the biggest barrier: an unsigned installer, and a Claude
  connection that asked too much for a calculator.
- Units stopped at electrical, and there were no conversions, which are the
  commonest calculation in the trade.
- An AI number could not be checked without opening the steps. In a January
  2026 survey of 403 US engineers and students, 89% said they verify every
  AI result.

Each item is done only when it works in the running app, passes its tests,
and has not broken an earlier item.

1. **Broaden the instant path.** Mechatronics units, conversions with `to`,
   forgiving input (`x`, `²`, `%`). *Built 2026-11-19.*
2. **Remove daily friction.** Session history on the arrow keys, a movable
   window that remembers its place, theme that follows Windows. *Built
   2026-11-19.* Still to do: a check at 125% and 150% display scaling,
   which needs the Windows display setting changed by hand.
3. **Make installation trivial.** An installer that needs no admin rights,
   signed (SignPath Foundation offers free signing for open-source
   projects), working fully with no account. An original icon.
   *Partly built 2026-11-19:*
   - Done: a per-user installer (`npm run tauri build` produces
     `Clawculator_0.1.0_x64-setup.exe`, 1.36 MB; the app itself is 4.3 MB
     and used about 30 MB of memory when measured). It installs for the
     current user only, so it needs no admin rights. The release build was
     started on its own and answered `3.5 bar to psi` correctly.
   - Done: an original icon, two claw strokes forming an equals sign
     (`design/icon.svg`).
   - Not done: signing. SignPath Foundation needs a public repository and
     builds run by GitHub Actions, so this waits until the repo is public.
     Until then Windows will warn about the installer on other machines.
   - **The install on the dev machine was never real** (found 2026-11-19).
     Every install up to 0.6.0 was run from a Claude desktop session, which
     redirects writes under AppData into the Claude app's own private
     storage. The program files, the app's settings and the installed-apps
     entry all went there. From inside the session they looked as if they
     were at `%LOCALAPPDATA%\Clawculator`; Windows itself has no such
     folder. Only the Start-menu and desktop shortcuts were real, and they
     point at that missing folder. That is why the taskbar showed the
     default icon whenever it identified the app by its shortcut, and why
     the shortcut would not have launched anything.
   - What this means for the earlier claims in this file: "installed",
     "listed under installed apps" and "started from the Start-menu
     shortcut" were true only as seen from inside the session.
   - The installer itself is not at fault. It has to be run by the user,
     outside a Claude session.
   - **Real install, 2026-11-19.** The user ran the 0.6.0 installer
     themselves. Asked from outside the session, Windows now reports the
     program file at `%LOCALAPPDATA%\Clawculator`, version 0.6.0, an entry
     under installed apps, and a running copy started from that path. The
     taskbar button was captured and shows the app icon. This was also the
     first run of the installer's click-through screens.
   - A leftover copy of the program still sits in the Claude app's private
     storage from the earlier attempts. It is not used by anything.
   - Not tested: uninstalling.
   - Note: on a PC without WebView2 the installer downloads it, which needs
     the internet. Windows 11 already has it.
4. **Add Claude as an optional extra.** Both routes, offered at first need.
   A fast default model chosen by accuracy on the reference set. The five
   answer states, the formula line, steps, numbers computed by code.
   *Built 2026-11-19 for the Claude Code route (see "Solver" and "Claude
   in the app"). Not built:* the API-key route (it cannot be tested
   without a key).
5. **Then:** image paste (built), settings (built in 0.7.0, see
   "Settings"), error states (built in 0.8.0, see "Error states"), content
   security policy (set in 0.8.1), README with the privacy statement and
   "unofficial" line (written 2026-11-20, see "README"), public repo
   (prepared 2026-11-20, see "Release"; not yet public).
6. **After v1:** memory of preferences, follow-up context.

Already built: the window shell (item 0), and items 1 and 2.

Checked in the running app for item 2: an earlier input comes back on the up
arrow with its answer; a window moved 300 px left and 200 px up was written
to the settings file on hide and reappeared in the same place; a real mouse
drag on the top strip moved it by exactly the distance dragged; returning it
to the default corner cleared the saved position. The light colours were
checked in a browser with the system set to report light. Not checked: the
"Move back to the corner" tray item, restoring the position after a full
restart, and display scaling above 100%.

## Reference set

Location (planned): `tests/reference/`. Runs with memory off, plus a small
group of memory-specific cases. Must include:

- Correct-answer cases across BJT and MOSFET biasing, the diode equation,
  JFET square law, small-signal gain, RC/RLC filters, complex impedance, dB
  and unit conversions.
- "Must not flag" cases: valid but non-standard forms, starting with the JFET
  overdrive expression above.
- "Must flag" cases: unit mismatches and genuinely wrong formulas.
- "Must block" cases: missing values and ambiguous input.

v1 is done when every slice is built and the reference set passes.
