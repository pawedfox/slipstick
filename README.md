# Slipstick

A small engineering calculator for Windows that stays on top of your other
windows. It answers arithmetic, units and conversions as you type, and can
hand harder problems to Claude if you want it to.

<img src="docs/instant.png" width="444" alt="The Slipstick bar showing 1/(2*pi*4.7kohm*100nF) answered as 338.6 Hz">

Unofficial project. Not affiliated with or endorsed by Anthropic.

Status: version 0.9.2, the first public release. It is one person's tool,
in daily use by its author; expect rough edges. See
[What is not done](#what-is-not-done).

## What it does

Press **Ctrl+Alt+C** anywhere. A small bar appears at the bottom-right of
the screen with the cursor in it. Type, read the answer, carry on working;
the answer stays visible. **Esc** hides it.

### Plain maths, as you type

No account and no network. The answer changes with every key.

| You type | It shows |
|---|---|
| `2.2mA * 4.7kohm` | 10.34 V |
| `1/(2*pi*4.7kohm*100nF)` | 338.6 Hz |
| `10k \|\| 22k \|\| 47k` | 5.998 k |
| `9V * 4.7k / (10k + 4.7k)` | 2.878 V |
| `4k7 + 330` | 5.030 k |
| `6 bar * pi * (25mm)^2` | 1.178 kN |
| `10 Nm * 1500 rpm` | 1.571 kW |
| `3.5 bar to psi` | 50.76 psi |
| `12 L/min to gpm` | 3.170 gal/min |
| `100 C to F` | 212.0 °F |
| `VGS = -0.3V * (1 - sqrt(0.1mA / 0.2mA))` | −87.87 mV |

It understands:

- **Prefixes** (`4.7k`, `100n`, `2.2u`, `1M`, `1meg`) and component notation
  (`4k7`, `2n2`, `4R7`). `m` is milli and `M` is mega.
- **Units, with the dimensions checked.** Electrical (V, A, Ω, F, H, S, W,
  Hz, J, Wh), magnetic (Wb, tesla), time, length, mass, force, pressure
  (Pa, bar, psi, atm), torque (N·m, lb·ft), power (W, hp), angle and
  rotation (deg, rad, rpm), volume and flow (L, gal, L/min, gpm, cfm), and
  speed. Volts times amps comes out in watts; volts plus amps is refused.
- **Conversions** with `to` (or `in`): `25 mm to in`.
- `||` for parallel, `x` or `×` for multiply, `²` and `³`, `√`, `π`, `%`,
  and the usual functions (`sqrt ln log exp sin cos tan asin acos atan abs`).
- **A name in front**, as you would write it on paper: `VGS = …`.

It is strict on purpose. Anything it is not sure of gets no instant answer,
never a guess. Click the answer, or press Ctrl+C with nothing selected, to
copy the plain number. The up and down arrows recall earlier inputs.

### Harder problems, with Claude (optional)

Press **Enter** on something that is not plain maths:

<img src="docs/steps.png" width="444" alt="A loaded voltage divider described in words, answered as 2.512 V, with the formula used under the answer and the two steps of working shown above it">

- **Claude never does the arithmetic.** It replies with the formulas, and
  the app's own engine works out every number, checking the units as it
  goes. If a step will not run, no number is shown.
- Every answer from Claude has **one line with the formula it used**, so you
  can check it at a glance, and **Steps** to see the working.
- Anything Claude assumed (a V_BE, a β, a temperature) is named under the
  answer. If the answer hangs on an assumed value, it says so.
- **It does not correct your notation.** A valid formula written your own
  way is accepted without comment. A formula that is definitely wrong (the
  units do not balance, say) is flagged, with what you typed and the usual
  form side by side:

  <img src="docs/flagged.png" width="444" alt="P = V / R with V = 12 V and R = 8 ohm, answered as 1.500 A with a yellow note: Gives amps, not watts. The usual form gives 18.00 W.">

- **Pictures.** Paste a screenshot of a formula, a datasheet row or a small
  schematic, up to four at once, with or without words.

Claude can be wrong. The formula line and the steps are there so that you
can check an answer before you rely on it; please do.

## Install

Slipstick is for Windows 11. It has not been tried on Windows 10.

1. Download `Slipstick_<version>_x64-setup.exe` from the
   [Releases page](https://github.com/pawedfox/slipstick/releases). Each
   release is built from this source by GitHub, and lists the installer's
   SHA-256 so you can check your download.
2. Run it. It installs for your user only and does not ask for
   administrator rights.
3. **The installer is not signed yet**, so Windows will show "Windows
   protected your PC". Choose *More info*, then *Run anyway*. If you would
   sooner not, build it from source (below).

On a PC without Microsoft's WebView2 the installer downloads it, which needs
the internet. Windows 11 already has it.

The app lives in the system tray. Right-click its icon for Settings and
Quit.

## Using Claude

Claude is off until you ask for it. The first time you press Enter on a
problem that needs it, the app offers to use the Claude Code on your PC.

- **Slipstick runs Claude Code.** You need
  [Claude Code](https://claude.com/claude-code) installed and signed in.
  Slipstick starts your own copy, exactly as Anthropic publishes it, and
  passes it the problem. It never sees, stores or handles your sign-in.
- **How Claude Code signs in is left alone.** If yours is signed in with a
  Claude plan, problems count against that plan's limits. If you have set
  it up with an API key (for example in `ANTHROPIC_API_KEY`), the key is
  used and billed. Slipstick pays for nothing and changes nothing.
- Anthropic's [legal page for Claude Code](https://code.claude.com/docs/en/legal-and-compliance)
  sets the conditions for programs that run Claude Code, and its terms
  decide what your plan may be used for. Slipstick is built to keep those
  conditions, but this project cannot speak for Anthropic and is not
  approved or endorsed by it. How these terms treat tools like this one has
  changed before and may change again.

Disconnect at any time in Settings.

## Settings

Open with the gear, **Ctrl+,** or the tray menu.

| Setting | Choices |
|---|---|
| Shortcut | Any combination with two of Ctrl, Alt and Shift |
| Start with Windows | Off / On |
| Theme | Follow Windows / Dark / Light |
| Significant figures | 3 to 6 |
| Claude | Connect or disconnect |
| Model | Fast (Sonnet) / Deeper (Opus) |
| Wrong formulas | Flag them, or show the corrected result with a note |

## Privacy

- **Plain maths never leaves your PC.**
- **A problem you send to Claude does leave it.** The text, and any
  pictures you pasted with it, go to Anthropic through Claude Code, and are
  handled under the terms of your Claude account. A problem is sent only
  when you press Enter on it, and never before you have agreed to connect
  Claude. Once connected, Claude Code is started in the background while
  you type something that is not plain maths, so that it is ready; what you
  are typing is not passed to it until you press Enter.
- **The app keeps almost nothing.** It writes one small settings file (the
  window position, your settings, and whether Claude is connected) and a
  copy of its instructions for Claude Code to read. It keeps no history and
  no log of problems or answers, and the app itself sends no usage data to
  anyone. Recalled inputs are held in memory and are gone when the app
  quits.
- Claude Code is started with its own session saving switched off and with
  all its tools switched off, so it can only reply with text.

## Build from source

You need [Node.js](https://nodejs.org), [Rust](https://rustup.rs) (stable,
MSVC) and the Microsoft C++ Build Tools.

```
npm install
npm run tauri dev
```

To build the installer, which lands in
`src-tauri/target/release/bundle/nsis/`:

```
npm run tauri build
```

To run the engine's tests, from `src-tauri/`:

```
cargo test --lib
```

[SPEC.md](SPEC.md) is the full record of what the app does, what was
decided and why, and what has and has not been tested.

## What is not done

- The installer is not signed.
- A way to use Claude without Claude Code installed.
- Memory of your notation and preferences.
- Not tested: Windows 10, and displays scaled above 125%.
- Not in the instant path: complex numbers, decibels, temperature
  differences, and conversions between rpm and Hz. Those go to Claude.
- Answers from Claude are checked against a set of 46 reference problems.
  The instructions given to Claude were tuned on that same set, so passing
  it says less than it seems to.

## Licence

MIT. See [LICENSE](LICENSE). The software comes with no warranty: check any
result that matters.

Claude and Claude Code are products of Anthropic. This project is not made,
sponsored or approved by Anthropic.
