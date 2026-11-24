import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { installDevBridge } from "./dev-bridge";

// In a plain browser during development there is no native side; a stand-in
// takes its place. Inside the app, and in release builds, this does nothing.
if (import.meta.env.DEV) installDevBridge();

// The window. Plain maths is answered locally as you type (the instant
// path). Anything else is sent to Claude when you press Enter, if you have
// connected it; Claude writes the formulas and the app's engine computes
// every number.

const SHELL_MARGIN = 12; // must match --margin in styles.css and lib.rs
const WINDOW_HEIGHT = 660; // must match WINDOW_HEIGHT in lib.rs

const el = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;

const shell = el<HTMLElement>("shell");
const minimise = el<HTMLButtonElement>("minimise");
const steps = el<HTMLElement>("steps");
const readout = el<HTMLElement>("readout");
const answer = el<HTMLElement>("answer");
const num = el<HTMLElement>("num");
const unit = el<HTMLElement>("unit");
const angle = el<HTMLElement>("angle");
const copied = el<HTMLElement>("copied");
const also = el<HTMLElement>("also");
const formula = el<HTMLElement>("formula");
const assumed = el<HTMLElement>("assumed");
const flag = el<HTMLElement>("flag");
const toggle = el<HTMLButtonElement>("toggle");
const notice = el<HTMLElement>("notice");
const message = el<HTMLElement>("message");
const action = el<HTMLButtonElement>("action");
const input = el<HTMLTextAreaElement>("input");
const pictures = el<HTMLElement>("pictures");
const preview = el<HTMLElement>("preview");
const previewImage = el<HTMLImageElement>("preview-image");
const sensitive = el<HTMLElement>("sensitive");
const rows = el<HTMLElement>("rows");
const single = el<HTMLElement>("single");
const comparison = el<HTMLElement>("comparison");
const bar = input.parentElement as HTMLElement;
const gear = el<HTMLButtonElement>("gear");
const forget = el<HTMLButtonElement>("forget");
const settingsPanel = el<HTMLElement>("settings");
const hotkeyButton = el<HTMLButtonElement>("hotkey");
const claudeState = el<HTMLElement>("claude-state");
const claudeAction = el<HTMLButtonElement>("claude-action");
const claudeHint = el<HTMLElement>("claude-hint");
const settingsNote = el<HTMLElement>("settings-note");

// ---- what the Rust side sends --------------------------------------------

/** A number as shown, with a plain version for the clipboard. A complex
 *  answer from the instant path has its magnitude as the number, and also
 *  its angle (`−9.043°`) and its parts (`10.00 − j1.592 kΩ`). */
type Value = {
  name?: string;
  number: string;
  unit: string;
  copy: string;
  si_value?: number;
  angle?: string;
  parts?: string;
  /** The input was a formula with its values after it, worked out as
   *  typed. Enter sends it on to Claude to be checked. */
  given?: boolean;
  /** The values kept for the session that an instant answer used:
   *  `Vcc = 9.000 V, R1 = 10.00 kΩ`. Shown beneath it. */
  uses?: string;
  /** How a power typed straight after a value with its unit was read:
   *  `read as (12 V)²`, for `12V^2`. Shown beneath the answer, since it
   *  could have been twelve square volts. */
  read?: string;
};

/** A value kept under a name for the session (`R1 = 10kohm`, then `R1` in
 *  what follows). The Rust side makes these and reads them back; the
 *  window only holds them, and looks at nothing but the name. */
type Kept = { name: string } & Record<string, unknown>;

/** One answer, several of equal standing, or two values compared. */
type Layout = "single" | "list" | "compare";

/** One line of working, with the value the engine computed. Claude's steps
 *  have a number and a unit, and a note in Claude's words. The steps of an
 *  instant answer are the engine's own: the value comes whole, the note is
 *  one of a few fixed words, and the name or the text may be empty. */
type Step = {
  name: string;
  text: string;
  note: string;
  number?: string;
  unit?: string;
  value?: string;
  /** An equation with its value put back: what the right-hand side comes
   *  to, `value` being the left. */
  other?: string;
};

/** Something Claude supplied that the user did not give. */
type Assumption = {
  text: string;
  /** The answer depends heavily on it. */
  sensitive: boolean;
  /** Percent the answer moves if it is 10% off, as measured by the engine. */
  moves: number | null;
};

/** Claude's reply after the engine has run it. */
type Solved = {
  state: "clean" | "assumed" | "flagged" | "blocked" | "failed";
  message: string | null;
  formula: string | null;
  assumptions: Assumption[];
  flag: string | null;
  layout: Layout;
  result: Value | null;
  results: Value[];
  percent: number | null;
  also: Value[];
  standard: Value | null;
  steps: Step[];
};

/** A picture pasted in with, or in place of, the typed problem. */
type Picture = { media_type: string; data: string; url: string };

/** The settings, as the Rust side reports them. */
type Settings = {
  /** The shortcut as people write it: `Ctrl+Alt+C`. */
  hotkey: string;
  /** False when another program holds the shortcut, so it does nothing. */
  hotkey_registered: boolean;
  start_with_windows: boolean;
  theme: "system" | "dark" | "light";
  figures: number;
  /** What to do with a definite error in a formula. */
  correction: "flag" | "correct";
  model: "fast" | "deeper";
};

/** Pictures larger than this are refused. */
const PICTURE_LIMIT = 5 * 1024 * 1024;
/** How many pictures one problem may carry. */
const PICTURE_COUNT = 4;

/** The answer on screen. Only answers from Claude have the optional parts,
 *  but for the steps, which an instant answer with named values has too. */
type Shown = Value & {
  layout?: Layout;
  /** The answers of a list, or the two values of a comparison. */
  rows?: Value[];
  /** For a comparison: the first as a percentage above (or below) the second. */
  percent?: number;
  formula?: string;
  assumptions?: Assumption[];
  flag?: string;
  /** The usual form's result; or, when `corrected`, what was typed gives. */
  standard?: Value;
  /** The number shown is the usual form's, not what was typed. */
  corrected?: boolean;
  also?: Value[];
  steps?: Step[];
};

// ---- state ---------------------------------------------------------------
//
// Everything the window shows is derived from this one object by render().
// Nothing else writes to the page, so a slow or out-of-order update can never
// leave an old answer on screen.

const state = {
  /** The answer on screen, or null. */
  shown: null as Shown | null,
  /** The input has changed since `shown` was worked out. */
  stale: false,
  /** `shown` came from Claude, not from the instant path. */
  fromClaude: false,
  /** Pasted pictures waiting to be sent, or already sent, with the input. */
  pictures: [] as Picture[],
  /** The picture being shown at full size, or null. */
  viewing: null as Picture | null,
  stepsOpen: false,
  /** Waiting for Claude. */
  asking: false,
  /** A one-line message for this input, shown in place of an answer. */
  message: null as string | null,
  /** The message is an offer to connect Claude, with a button. */
  offer: false,
  /** The input whose refusal the message explains. Enter on it again sends
   *  it to Claude after all. */
  explained: null as string | null,
  /** The input Enter has just kept under its name, or said it could not.
   *  Enter on it again does not keep it a second time. */
  keptText: null as string | null,
  /** The names of the values kept for the session, in the order they were
   *  kept. While there are any, the Clear memory button is shown. */
  keptNames: [] as string[],
  /** The settings. These are the defaults until the Rust side has answered. */
  settings: {
    hotkey: "Ctrl+Alt+C",
    hotkey_registered: true,
    start_with_windows: false,
    theme: "system",
    figures: 4,
    correction: "flag",
    model: "fast",
  } as Settings,
  /** The settings screen is open, in place of the answer. */
  settingsOpen: false,
  /** The settings screen is waiting for a new shortcut to be pressed. */
  capturing: false,
  /** Why the last change in settings could not be made. */
  settingsProblem: null as string | null,
};

// ---- formulas as text ----------------------------------------------------
//
// Formulas are shown exactly as the engine computed them, lightly tidied.
// Text from Claude is never treated as HTML: it only ever becomes text nodes.

function tidy(text: string): string {
  return text
    .replace(/\s*\*\s*/g, " × ")
    .replace(/\s*\|\|\s*/g, " ∥ ")
    .replace(/\bpi\b/g, "π")
    .replace(/sqrt\s*\(/g, "√(")
    .replace(/ohms?\b/g, "Ω")
    .replace(/(\d)u(?=[A-Za-zΩ])/g, "$1µ")
    .replace(/\^2(?![\d.])/g, "²")
    .replace(/\^3(?![\d.])/g, "³");
}

/** Text with names like `V_th` set as V with a subscript. */
function mathNodes(text: string): Node[] {
  const nodes: Node[] = [];
  let last = 0;
  for (const match of text.matchAll(/([A-Za-z]+)_([A-Za-z0-9_]+)/g)) {
    nodes.push(document.createTextNode(text.slice(last, match.index)));
    nodes.push(document.createTextNode(match[1]));
    const sub = document.createElement("sub");
    // A second underscore becomes a comma: I_D_new reads as I with "D,new" below.
    sub.textContent = match[2].replace(/_/g, ",");
    nodes.push(sub);
    last = match.index + match[0].length;
  }
  nodes.push(document.createTextNode(text.slice(last)));
  return nodes;
}

/** The kept values an answer used (`Vcc = 9.000 V, R1 = 10.00 kΩ`), each
 *  held together so that a long line breaks between values, not in one. */
function usedNodes(text: string): Node[] {
  return text.split(", ").flatMap((value, index) => {
    const one = document.createElement("span");
    one.className = "used";
    one.append(...mathNodes(value));
    return index ? [document.createTextNode(", "), one] : [one];
  });
}

const spaced = (value: { number: string; unit: string }) => `${value.number} ${value.unit}`.trim();

function line(className: string, ...children: (Node | string)[]): HTMLElement {
  const element = document.createElement("div");
  element.className = className;
  element.append(...children);
  return element;
}

// ---- drawing -------------------------------------------------------------

function render() {
  const { shown, stale, settings } = state;
  const inSettings = state.settingsOpen;
  // A standing message, shown when nothing else is: the shortcut is taken.
  const standing = settings.hotkey_registered
    ? null
    : `${settings.hotkey} is in use by another program, so the shortcut is off. Choose another in settings (Ctrl+,).`;
  const text = state.asking ? "Asking Claude" : (state.message ?? standing);

  // Clear memory is there only while something is kept, and says what.
  forget.hidden = !state.keptNames.length || inSettings;
  forget.title = `Forget the values kept: ${state.keptNames.join(", ")}`;

  // No attribute means the colours follow Windows.
  if (settings.theme === "system") delete document.documentElement.dataset.theme;
  else document.documentElement.dataset.theme = settings.theme;

  renderSettings();

  readout.hidden = !shown || state.asking || inSettings;
  readout.classList.toggle("stale", stale);

  if (shown) {
    num.textContent = shown.number;
    unit.textContent = shown.unit;
    angle.textContent = shown.angle ? `∠ ${shown.angle}` : "";
    angle.hidden = !shown.angle;
    const length = shown.number.length + shown.unit.length + (shown.angle ? shown.angle.length + 3 : 0);
    answer.classList.toggle("long", length > 11);
    answer.setAttribute("aria-label", `Copy ${shown.copy}`);

    // A list shows its answers as rows and no single big number. A
    // comparison shows the two values as quieter rows above the big number,
    // which is the difference, with a line under it saying which is larger.
    const layout = shown.layout ?? "single";
    const isList = layout === "list";
    rows.replaceChildren(
      ...(layout === "single" ? [] : (shown.rows ?? []).flatMap((value, index) => rowNodes(value, index, isList))),
    );
    rows.hidden = layout === "single";
    rows.classList.toggle("items", layout === "compare");
    single.hidden = isList;

    const [first, second] = shown.rows ?? [];
    const compared = layout === "compare" && first && second;
    comparison.replaceChildren(...(compared ? comparisonNodes(first, second, shown.percent ?? 0) : []));
    comparison.hidden = !compared;

    // The Steps control sits on the big number's line, or at the top right
    // of the answer area when there is no big number.
    const host = isList ? readout : single;
    if (toggle.parentElement !== host) host.append(toggle);

    // A complex answer's real and imaginary parts go first, on their own.
    // Then how a power after a value was read, where it had two meanings.
    // Then the kept values an instant answer used, so that a value kept
    // some time ago is never used unseen.
    also.replaceChildren(
      ...(shown.parts ? [line("also-line parts", shown.parts)] : []),
      ...(shown.read ? [line("also-line parts", shown.read)] : []),
      ...(shown.uses ? [line("also-line parts", ...usedNodes(shown.uses))] : []),
      ...(shown.also ?? []).map((value) =>
        line("also-line", ...mathNodes(value.name ?? ""), ` = ${spaced(value)}`),
      ),
    );
    also.hidden = !shown.parts && !shown.read && !shown.uses && !shown.also?.length;

    formula.replaceChildren(...mathNodes(shown.formula ?? ""));
    formula.hidden = !shown.formula;

    // Ordinary assumptions go on one quiet line. Any the answer depends on
    // heavily are called out separately, with what the engine measured.
    const all = shown.assumptions ?? [];
    const quiet = all.filter((item) => !item.sensitive).map((item) => item.text);
    assumed.textContent = quiet.length ? `Assumed ${quiet.join(", ")}` : "";
    assumed.hidden = !quiet.length;

    const heavy = all.filter((item) => item.sensitive);
    sensitive.replaceChildren(
      ...heavy.map((item) => {
        const measured = item.moves ? `: a 10% error moves the answer ${item.moves}%.` : ".";
        return line("sensitive-line", `Depends heavily on the assumed ${item.text}${measured}`);
      }),
    );
    sensitive.hidden = !heavy.length;

    const usual = !shown.standard
      ? ""
      : shown.corrected
        ? ` Corrected to the usual form. What you typed gives ${spaced(shown.standard)}.`
        : ` The usual form gives ${spaced(shown.standard)}.`;
    flag.textContent = shown.flag ? `${shown.flag}${usual}` : "";
    flag.hidden = !shown.flag;

    steps.replaceChildren(
      ...(shown.steps ?? []).map((step) => {
        const working = document.createElement("span");
        working.className = "step-math";
        // A kept value has a name and no working; a formula with no name
        // in front has working and no name.
        working.append(
          ...mathNodes(step.name),
          step.name && step.text ? " = " : "",
          ...mathNodes(tidy(step.text)),
        );
        const note = document.createElement("span");
        note.className = "step-note";
        note.textContent = step.note;
        const value = step.value ?? spaced({ number: step.number ?? "", unit: step.unit ?? "" });
        // An equation with its value put back shows what each side comes to.
        const result = step.other ? `${value} = ${step.other}` : `= ${value}`;
        const row = line("step");
        row.append(line("step-working", working, note), line("step-value", result));
        return row;
      }),
    );
    toggle.hidden = !shown.steps?.length;
  }

  // The full-size picture and the steps share the space above the answer.
  // Settings, when open, has that space to itself.
  preview.hidden = !state.viewing || inSettings;
  if (state.viewing && previewImage.src !== state.viewing.url) previewImage.src = state.viewing.url;

  const open =
    Boolean(shown?.steps?.length) && state.stepsOpen && !state.asking && !state.viewing && !inSettings;
  steps.hidden = !open;
  toggle.setAttribute("aria-expanded", String(open));

  renderPictures();
  input.placeholder = state.pictures.length ? "Add a note, or press Enter" : "Type or paste a problem";

  // While a question is out, the typed text goes quiet: it has been sent.
  bar.classList.toggle("sent", state.asking);

  message.textContent = text ?? "";
  message.classList.toggle("working", state.asking);
  action.hidden = !state.offer || state.asking;
  notice.hidden = !text || inSettings;
}

/** The settings screen: marks the choice in force on each row. */
function renderSettings() {
  const { settings } = state;
  settingsPanel.hidden = !state.settingsOpen;
  gear.setAttribute("aria-expanded", String(state.settingsOpen));
  if (!state.settingsOpen) return;

  hotkeyButton.textContent = state.capturing ? "Press the new keys" : settings.hotkey;
  hotkeyButton.classList.toggle("waiting", state.capturing);

  for (const row of settingsPanel.querySelectorAll<HTMLElement>("[data-setting]")) {
    const current = String(settings[row.dataset.setting as keyof Settings]);
    for (const choice of row.querySelectorAll<HTMLElement>("[data-value]")) {
      choice.setAttribute("aria-pressed", String(choice.dataset.value === current));
    }
  }

  claudeState.textContent = claude.connected ? "Claude Code" : claude.found ? "" : "Claude Code not found";
  claudeAction.textContent = claude.connected ? "Disconnect" : "Use Claude Code";
  claudeAction.hidden = !claude.found;
  claudeHint.hidden = !claude.found || claude.connected;
  // These two only matter once Claude is answering.
  for (const row of settingsPanel.querySelectorAll<HTMLElement>(".needs-claude")) {
    row.hidden = !claude.connected;
  }

  settingsNote.textContent = state.settingsProblem ?? "";
  settingsNote.hidden = !state.settingsProblem;
}

/** The three cells of one row: a mark, the name, and the value. */
function rowNodes(value: Value, index: number, withEquals: boolean): HTMLElement[] {
  const mark = line("lead equals", withEquals ? "=" : "");
  const name = line("row-name", ...mathNodes(value.name ?? ""));
  const number = document.createElement("button");
  number.className = "row-value";
  number.tabIndex = -1;
  number.dataset.row = String(index);
  number.title = "Copy";
  const unitText = document.createElement("span");
  unitText.className = "unit";
  unitText.textContent = value.unit;
  number.append(value.number, unitText);
  return [mark, name, number];
}

/** "I_D1 is 50% higher than I_D2", with the names set properly. */
function comparisonNodes(first: Value, second: Value, percent: number): (Node | string)[] {
  if (percent === 0) return [...mathNodes(first.name ?? ""), " and ", ...mathNodes(second.name ?? ""), " are equal"];
  // Three significant figures, without trailing zeros: 50, 33.3, 0.5.
  const size = Number(Math.abs(percent).toPrecision(3));
  return [
    ...mathNodes(first.name ?? ""),
    ` is ${size}% ${percent > 0 ? "higher" : "lower"} than `,
    ...mathNodes(second.name ?? ""),
  ];
}

/** The row of thumbnails. Rebuilt only when the pictures change. */
let drawnPictures: Picture[] = [];
function renderPictures() {
  pictures.hidden = !state.pictures.length;
  for (const frame of pictures.children) {
    frame.classList.toggle("viewing", (frame as HTMLElement).dataset.url === state.viewing?.url);
  }
  if (drawnPictures.length === state.pictures.length && drawnPictures.every((p, i) => p === state.pictures[i])) {
    return;
  }
  drawnPictures = [...state.pictures];
  pictures.replaceChildren(
    ...state.pictures.map((picture, index) => {
      const image = document.createElement("img");
      image.src = picture.url;
      image.alt = "";

      const view = document.createElement("button");
      view.className = "thumb";
      view.tabIndex = -1;
      view.title = "Show at full size";
      view.setAttribute("aria-label", `Picture ${index + 1}: show at full size`);
      view.append(image);
      view.addEventListener("click", () => {
        state.viewing = state.viewing === picture ? null : picture;
        void update();
        input.focus();
      });

      const remove = document.createElement("button");
      remove.className = "remove";
      remove.tabIndex = -1;
      remove.title = "Remove this picture";
      remove.setAttribute("aria-label", `Remove picture ${index + 1}`);
      remove.textContent = "×";
      remove.addEventListener("click", () => {
        removePicture(picture);
        input.focus();
      });

      const frame = line("picture", view, remove);
      frame.dataset.url = picture.url;
      frame.classList.toggle("viewing", picture === state.viewing);
      return frame;
    }),
  );
}

// ---- visible height ------------------------------------------------------
//
// The OS window never resizes (that is what made the steps flicker). It is a
// fixed transparent rectangle, clipped by the Rust side to the part in use.
// To change the layout without a clipped frame: open the clip fully, render,
// then close the clip down to the new height.

const usedHeight = () =>
  Math.ceil(shell.getBoundingClientRect().height) + SHELL_MARGIN * 2;

const setVisibleHeight = (height: number) => invoke("set_visible_height", { height });

const nextFrame = () => new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));

let lastHeight = 0;
let pending: Promise<void> | null = null;

/** Renders the current state. Safe to call as often as needed: calls made
 *  while one is waiting are folded into it. */
function update(): Promise<void> {
  pending ??= (async () => {
    await setVisibleHeight(WINDOW_HEIGHT);
    pending = null; // changes from here on need a new pass
    render();
    await nextFrame();
    lastHeight = usedHeight();
    await setVisibleHeight(lastHeight);
  })();
  return pending;
}

// Safety net for any size change that did not come from update().
new ResizeObserver(() => {
  if (usedHeight() !== lastHeight) void update();
}).observe(shell);

// ---- Claude --------------------------------------------------------------

type ClaudeStatus = { found: boolean; connected: boolean };
let claude: ClaudeStatus = { found: false, connected: false };

async function refreshClaude() {
  claude = await invoke<ClaudeStatus>("claude_status");
}

/** Counts requests, so a reply to an abandoned question is ignored. */
let question = 0;

function showSolved(solved: Solved) {
  const { result } = solved;
  if (!result) {
    // Blocked says what is needed; failed says what went wrong.
    state.shown = null;
    state.message =
      solved.state === "failed"
        ? `Claude's working did not check out, so no answer is shown. ${solved.message ?? ""} Press Enter to ask again, or reword the problem.`
        : solved.message;
    return;
  }
  // For a comparison the big number is the size of the difference; the
  // line under it says which way round it goes.
  const isComparison = solved.layout === "compare";
  // "Wrong formulas: Correct". A definite error with a usual form to fall
  // back on shows that form's result as the answer, and the flag line says
  // so and gives what was typed. It is never corrected without saying.
  const corrected =
    state.settings.correction === "correct" &&
    solved.state === "flagged" &&
    solved.layout === "single" &&
    solved.standard !== null;
  const main = corrected && solved.standard ? solved.standard : result;
  state.shown = {
    number: isComparison ? main.number.replace(/^−/, "") : main.number,
    unit: main.unit,
    copy: isComparison ? main.copy.replace(/^-/, "") : main.copy,
    layout: solved.layout,
    rows: solved.results,
    percent: solved.percent ?? undefined,
    formula: solved.formula ?? undefined,
    assumptions: solved.assumptions,
    flag: solved.flag ?? undefined,
    standard: (corrected ? result : solved.standard) ?? undefined,
    corrected,
    also: solved.also,
    steps: solved.steps,
  };
  state.stale = false;
  state.fromClaude = true;
  state.stepsOpen = false;
  state.message = null;
}

/** After a question has been sent, typing starts a new one. */
let replaceOnType = false;

async function ask(text: string) {
  const mine = ++question;
  // Show that it has gone: nothing selected, and a long problem back at
  // its first line.
  input.setSelectionRange(input.value.length, input.value.length);
  input.scrollTop = 0;
  replaceOnType = true;
  state.asking = true;
  state.offer = false;
  state.message = null;
  void update();

  let solved: Solved | null = null;
  let problem: string | null = null;
  try {
    const images = state.pictures.map(({ media_type, data }) => ({ media_type, data }));
    solved = await invoke<Solved>("ask_claude", { input: text, images });
  } catch (error) {
    problem = String(error);
  }
  if (mine !== question) return; // the question was abandoned

  state.asking = false;
  if (solved) {
    showSolved(solved);
  } else {
    state.shown = null;
    state.message = problem;
  }
  void update();
}

/** Stops waiting for Claude. Returns false if there was nothing to stop. */
function stopAsking(): boolean {
  if (!state.asking) return false;
  question += 1;
  state.asking = false;
  void invoke("cancel_claude");
  void update();
  return true;
}

/** Enter on something the instant path cannot answer. */
async function needsClaude(text: string) {
  await refreshClaude();
  if (claude.connected) {
    void ask(text);
    return;
  }
  state.shown = null;
  state.stale = false;
  state.offer = claude.found;
  state.message = claude.found
    ? "This needs Claude. Use the Claude Code on this PC? Problems you send this way go to Claude, under your own Claude Code sign-in."
    : "This needs Claude, and Claude Code was not found on this PC. Install it and sign in, then press Enter again.";
  void update();
}

async function connect() {
  try {
    await invoke("connect_claude");
  } catch (error) {
    state.offer = false;
    state.message = String(error);
    void update();
    return;
  }
  await refreshClaude();
  const text = input.value.trim();
  if (text || state.pictures.length) void ask(text);
}

action.textContent = "Use Claude Code";
action.addEventListener("click", () => {
  void connect();
  input.focus();
});

/** Gets Claude Code started while the user is still typing a problem. */
let lastWarmed = 0;
function warmClaude() {
  if (!claude.connected || Date.now() - lastWarmed < 20_000) return;
  lastWarmed = Date.now();
  void invoke("warm_claude");
}

void listen("claude-changed", async () => {
  await refreshClaude();
  void update();
});

// ---- settings ------------------------------------------------------------

async function openSettings() {
  // Ask afresh: "Start with Windows" is kept by Windows and can change
  // outside the app.
  const [settings] = await Promise.all([invoke<Settings>("settings_view"), refreshClaude()]);
  state.settings = settings;
  state.settingsOpen = true;
  state.settingsProblem = null;
  void update();
}

/** Closes the settings screen. Returns false if it was not open. */
function closeSettings(): boolean {
  if (!state.settingsOpen) return false;
  if (state.capturing) void capture(null);
  state.settingsOpen = false;
  void update();
  input.focus();
  return true;
}

const toggleSettings = () => (state.settingsOpen ? void closeSettings() : void openSettings());

/** Makes one change. The Rust side replies with the settings as they now
 *  stand, or says why the change could not be made. */
async function changeSetting(name: string, value: string | number | boolean) {
  const figuresBefore = state.settings.figures;
  try {
    state.settings = await invoke<Settings>("change_setting", { change: { name, value } });
    state.settingsProblem = null;
  } catch (error) {
    state.settingsProblem = String(error);
  }
  // An instant answer on screen is worked out again to the new figures.
  // One from Claude stays as it is until the problem is next sent.
  if (state.settings.figures !== figuresBefore && state.shown && !state.fromClaude) {
    void evaluateInstantly();
  }
  void update();
}

/** Starts waiting for a new shortcut to be pressed. */
async function startCapture() {
  state.capturing = true;
  state.settingsProblem = null;
  void update();
  // The shortcut is switched off meanwhile, or pressing it would hide the
  // window instead of being read.
  await invoke("pause_hotkey", { paused: true });
}

/** Stops waiting: with the keys that were pressed, or with null to leave
 *  the shortcut as it was. */
async function capture(keys: string | null) {
  state.capturing = false;
  if (keys) await changeSetting("hotkey", keys);
  await invoke("pause_hotkey", { paused: false });
  void update();
}

settingsPanel.addEventListener("click", (event) => {
  const button = (event.target as HTMLElement).closest<HTMLButtonElement>("button");
  if (!button) return;
  // After a click with the mouse, typing still goes to the problem. A
  // choice made from the keyboard keeps its place among the choices.
  if (event.detail > 0) input.focus();
  if (button === hotkeyButton) {
    void (state.capturing ? capture(null) : startCapture());
    return;
  }
  if (button === claudeAction) {
    void (async () => {
      try {
        await invoke(claude.connected ? "disconnect_claude" : "connect_claude");
        state.settingsProblem = null;
      } catch (error) {
        state.settingsProblem = String(error);
      }
      await refreshClaude();
      void update();
    })();
    return;
  }
  const name = button.parentElement?.dataset.setting;
  const raw = button.dataset.value;
  if (!name || raw === undefined) return;
  const value = name === "figures" ? Number(raw) : name === "start_with_windows" ? raw === "true" : raw;
  void changeSetting(name, value);
});

gear.addEventListener("click", () => {
  toggleSettings();
  input.focus();
});
void listen("open-settings", () => void openSettings());

// While waiting for a new shortcut, the next key press is the shortcut. It
// is read here, ahead of everything else, and goes no further.
window.addEventListener(
  "keydown",
  (event) => {
    if (!state.capturing) return;
    event.preventDefault();
    event.stopImmediatePropagation();
    if (event.key === "Escape") {
      void capture(null);
      return;
    }
    // A modifier by itself is the start of a shortcut, not the whole of it.
    if (["Control", "Alt", "Shift", "Meta", "AltGraph"].includes(event.key)) return;
    const keys = [
      event.ctrlKey && "Ctrl",
      event.altKey && "Alt",
      event.shiftKey && "Shift",
      event.metaKey && "Super",
      event.code,
    ]
      .filter(Boolean)
      .join("+");
    void capture(keys);
  },
  true,
);

// Leaving the window ends the wait, so the shortcut is not left switched off.
window.addEventListener("blur", () => {
  if (state.capturing) void capture(null);
});

// ---- session history -----------------------------------------------------
//
// Earlier inputs from this session, recalled with the up and down arrows.
// Held in memory only: nothing is written to disk, and it is gone when the
// app quits.

const HISTORY_LIMIT = 100;
const history: string[] = [];
/** Position in `history` while recalling, or null while typing normally. */
let browsing: number | null = null;
/** What was in the box before recalling started. */
let draft = "";

function remember(text: string) {
  const entry = text.trim();
  if (!entry || history[history.length - 1] === entry) return;
  history.push(entry);
  if (history.length > HISTORY_LIMIT) history.shift();
}

/** Remembers what is in the box, if it has a current answer. */
function rememberAnswered() {
  if (state.shown && !state.stale) remember(input.value);
}

/** Moves through history. Returns false if there was nowhere to go. */
function recall(direction: "older" | "newer"): boolean {
  if (direction === "older") {
    if (browsing === null) {
      if (!history.length) return false;
      draft = input.value;
      browsing = history.length;
    }
    // Skip an entry identical to what is already in the box.
    let next = browsing - 1;
    if (next >= 0 && history[next] === input.value.trim()) next -= 1;
    if (next < 0) return true; // already at the oldest
    browsing = next;
  } else {
    if (browsing === null) return false;
    browsing += 1;
    if (browsing >= history.length) browsing = null;
  }
  input.value = browsing === null ? draft : history[browsing];
  input.setSelectionRange(input.value.length, input.value.length);
  fitInput();
  void evaluateInstantly();
  return true;
}

// ---- values kept for the session -----------------------------------------
//
// Enter on an answer with a name in front (`R1 = 10kohm`) keeps it under
// that name, and later inputs may use the name. Held in memory only, like
// the history: nothing is written to disk, and it is gone when the app
// quits. The engine is handed the list with each input and keeps nothing
// itself.

const KEPT_LIMIT = 200;
const kept: Kept[] = [];

/** Enter on a named input: keeps its value, or says why it cannot be kept.
 *  Returns false where there is nothing to keep, and Enter means what it
 *  always did. */
async function keepNamed(text: string, answered: boolean): Promise<boolean> {
  let entry: Kept | null = null;
  let reason: string | null = null;
  try {
    entry = await invoke<Kept | null>("keep", { input: text, kept });
  } catch (error) {
    reason = String(error);
  }
  if (input.value.trim() !== text) return true; // the box has changed meanwhile
  if (!entry && !reason) return false;

  // A formula with its values after it can also be checked by Claude. If
  // it cannot be kept, Enter goes straight on to that, as it did before
  // values could be kept; if it can, Enter again does.
  const checkable = answered && Boolean(state.shown?.given);
  if (checkable) await refreshClaude();
  if (reason && checkable && claude.connected) return false;

  if (entry) {
    const name = entry.name;
    // Kept again, a name's earlier value goes.
    const at = kept.findIndex((other) => other.name === name);
    if (at < 0 && kept.length >= KEPT_LIMIT) {
      reason = `No more values can be kept: there are ${KEPT_LIMIT}. Clear memory, at the top right, forgets them.`;
    } else {
      if (at >= 0) kept.splice(at, 1);
      kept.push(entry);
      const more = checkable && claude.connected ? " Enter again has Claude check the formula." : "";
      reason = `${name} is kept until Slipstick is quit.${more}`;
    }
  }
  // A value that is kept but not shown (`beta = 200`) leaves no old answer
  // behind it.
  if (!answered) {
    state.shown = null;
    state.stale = false;
  }
  state.keptText = text;
  state.keptNames = kept.map((value) => value.name);
  state.offer = false;
  state.message = reason;
  void update();
  return true;
}

/** Clear memory: forgets every value kept for the session, as quitting
 *  does. The input is then read again with nothing kept, so an answer that
 *  used one of them does not stay on screen. An answer from Claude is left
 *  as it is: Claude is never told the kept values. */
async function forgetKept() {
  const count = kept.length;
  if (!count) return;
  kept.length = 0;
  state.keptNames = [];
  state.keptText = null;
  if (state.shown && !state.fromClaude) {
    const ticket = ++evaluation;
    const text = input.value;
    const result = text.trim()
      ? await invoke<(Value & { steps?: Step[] }) | null>("evaluate", { input: text, kept })
      : null;
    if (ticket === evaluation) {
      state.shown = result;
      state.stale = false;
      state.stepsOpen = false;
    }
  }
  state.offer = false;
  state.explained = null;
  state.message = count === 1 ? "1 kept value forgotten." : `${count} kept values forgotten.`;
  input.focus();
  void update();
}

// ---- input ---------------------------------------------------------------

function fitInput() {
  input.style.height = "auto";
  input.style.height = `${input.scrollHeight}px`;
}

/** Guards against replies arriving out of order while typing quickly. */
let evaluation = 0;

/** Done when the instant path has looked at what is in the box now. Until
 *  then the answer on screen may belong to what was there a moment ago. */
let settled: Promise<void> = Promise.resolve();

/** Asks the instant path about the present text and shows what it says. */
function evaluateInstantly(): Promise<void> {
  settled = applyInstantAnswer();
  return settled;
}

async function applyInstantAnswer() {
  const ticket = ++evaluation;
  const text = input.value;
  // With a picture attached the words are for Claude, not for the instant path.
  const hasPictures = state.pictures.length > 0;
  const result =
    text.trim() && !hasPictures
      ? await invoke<(Value & { steps?: Step[] }) | null>("evaluate", { input: text, kept })
      : null;
  if (ticket !== evaluation) return;

  state.message = null;
  state.offer = false;
  state.explained = null;
  state.keptText = null;
  if (result) {
    state.shown = result;
    state.stale = false;
    state.fromClaude = false;
    state.stepsOpen = false;
  } else if (!text.trim() && !hasPictures) {
    state.shown = null;
    state.stale = false;
  } else {
    // Mid-edit (`4.7k *`) or not plain maths: keep the last answer in place
    // but dimmed, so the window does not jump while typing and an old number
    // is never mistaken for a current one.
    if (state.shown) state.stale = true;
    // The steps of an instant answer are of the input as it was, so they
    // close when it changes.
    if (!state.fromClaude) state.stepsOpen = false;
    // It may be on its way to Claude, so get Claude ready.
    if (text.trim().length >= 6 || hasPictures) warmClaude();
  }
  void update();
}

// ---- pictures ------------------------------------------------------------

/** Call after the pictures change: the question is now a different one. */
function picturesChanged() {
  if (state.viewing && !state.pictures.includes(state.viewing)) state.viewing = null;
  stopAsking();
  void evaluateInstantly();
}

function removePicture(picture: Picture) {
  URL.revokeObjectURL(picture.url);
  state.pictures = state.pictures.filter((other) => other !== picture);
  picturesChanged();
}

function removeAllPictures() {
  for (const picture of state.pictures) URL.revokeObjectURL(picture.url);
  state.pictures = [];
  picturesChanged();
}

/** Reads a pasted picture file and attaches it. */
function attach(file: File) {
  const refusal =
    file.size > PICTURE_LIMIT
      ? "That picture is too large. The limit is 5 MB."
      : state.pictures.length >= PICTURE_COUNT
        ? `A problem can carry up to ${PICTURE_COUNT} pictures.`
        : null;
  if (refusal) {
    state.message = refusal;
    void update();
    return;
  }
  const reader = new FileReader();
  reader.onload = () => {
    // A data URL looks like "data:image/png;base64,AAAA…".
    const [, data = ""] = String(reader.result).split(",", 2);
    state.pictures = [...state.pictures, { media_type: file.type, data, url: URL.createObjectURL(file) }];
    picturesChanged();
  };
  reader.readAsDataURL(file);
}

input.addEventListener("paste", (event) => {
  const files = Array.from(event.clipboardData?.items ?? [])
    .filter((item) => item.type.startsWith("image/"))
    .map((item) => item.getAsFile())
    .filter((file): file is File => file !== null);
  if (!files.length) return; // ordinary text: let it paste as usual
  event.preventDefault();
  files.forEach(attach);
});

// Clicking the full-size picture closes it.
preview.addEventListener("click", () => {
  state.viewing = null;
  void update();
  input.focus();
});

const allSelected = () =>
  input.value.length > 0 && input.selectionStart === 0 && input.selectionEnd === input.value.length;

// About to type or paste over everything: keep what was there in history,
// and let go of any pictures, which belonged to the old problem. (Pasting
// another picture is not typing over the text, so it keeps them.)
input.addEventListener("beforeinput", (event) => {
  if (!allSelected()) return;
  rememberAnswered();
  const pastingPicture = Array.from(event.dataTransfer?.items ?? []).some((item) => item.type.startsWith("image/"));
  if (state.pictures.length && !pastingPicture) removeAllPictures();
});

// Clicking into the text means editing it, not replacing it.
input.addEventListener("mousedown", () => {
  replaceOnType = false;
});

input.addEventListener("input", () => {
  browsing = null;
  replaceOnType = false;
  state.settingsOpen = false; // typing a problem puts the settings away
  stopAsking(); // the question has changed
  fitInput();
  void evaluateInstantly();
});

input.addEventListener("keydown", (event) => {
  // The problem in the box has been sent. Typing or pasting now starts a
  // new problem in its place; an editing key means the old one is being
  // changed instead.
  if (replaceOnType) {
    const typing = event.key.length === 1 && !event.ctrlKey && !event.altKey && !event.metaKey;
    const pasting = event.key.toLowerCase() === "v" && event.ctrlKey;
    if (typing || pasting) input.select();
    if (typing || pasting || /^(Arrow|Home|End|Backspace|Delete)/.test(event.key)) replaceOnType = false;
  }

  const plain = !event.shiftKey && !event.ctrlKey && !event.altKey && !event.metaKey;
  if (plain && (event.key === "ArrowUp" || event.key === "ArrowDown")) {
    // Arrows recall history when the caret has nowhere to go: at either
    // end of the text, with everything selected, or mid-recall. Otherwise
    // they move the caret as usual.
    const caret = input.selectionStart === input.selectionEnd ? input.selectionStart : -1;
    const idle = !input.value || caret === 0 || caret === input.value.length || allSelected();
    if ((browsing !== null || idle) && recall(event.key === "ArrowUp" ? "older" : "newer")) {
      event.preventDefault();
    }
    return;
  }

  // Backspace in an empty box removes the last picture.
  const last = state.pictures[state.pictures.length - 1];
  if (event.key === "Backspace" && !input.value && last) {
    event.preventDefault();
    removePicture(last);
    return;
  }

  if (event.key !== "Enter" || event.shiftKey) return;
  event.preventDefault();
  void submit();
});

/** Enter: sends the problem to Claude, unless it is already answered. */
async function submit() {
  // Enter can arrive a moment after the last key, before the instant path
  // has looked at the new text. Deciding then would go by the old answer
  // and do nothing. Wait until it has caught up with the box.
  let seen: Promise<void>;
  do {
    seen = settled;
    await seen;
  } while (seen !== settled);

  const text = input.value.trim();
  remember(text);
  browsing = null;
  // An instant answer does not count when a picture is attached: the
  // picture is the problem, so it still has to go to Claude.
  const answered = state.shown && !state.stale && (state.fromClaude || !state.pictures.length);
  if (!text && !state.pictures.length) {
    state.shown = null;
    state.message = null;
    state.offer = false;
    void update();
  } else if (state.asking) {
    // Already being asked.
  } else if (
    !state.pictures.length &&
    !state.offer &&
    !(answered && state.fromClaude) &&
    state.keptText !== text &&
    (await keepNamed(text, Boolean(answered)))
  ) {
    // A name in front: the value is kept under it for the session, or the
    // message says why it cannot be.
  } else if (answered) {
    // Already answered. A formula with its values after it is worked out
    // exactly as typed, though, so Enter has Claude check it: a formula
    // that is definitely wrong is then flagged. Without Claude connected
    // the instant answer simply stays.
    if (state.shown?.given && !state.fromClaude) {
      await refreshClaude();
      if (claude.connected && input.value.trim() === text) void ask(text);
    }
  } else if (state.keptText === text) {
    // Kept a moment ago, with no answer to show (`beta = 200`). There is
    // nothing more for Enter to do.
  } else if (state.offer) {
    void connect(); // Enter accepts the offer
  } else if (!state.pictures.length && state.explained !== text) {
    // The instant path refused it. If it knows why, say so first; Enter
    // again on the same text sends it to Claude after all.
    const reason = await invoke<string | null>("explain", { input: text, kept });
    if (input.value.trim() !== text) return; // the box has changed meanwhile
    if (!reason) {
      void needsClaude(text);
      return;
    }
    state.explained = text;
    state.shown = null;
    state.stale = false;
    state.message = reason;
    void update();
  } else {
    void needsClaude(text);
  }
}

// ---- copy the plain number -----------------------------------------------

async function copyAnswer() {
  if (!state.shown || state.stale || state.asking) return;
  await navigator.clipboard.writeText(state.shown.copy);
  copied.classList.add("on");
  setTimeout(() => copied.classList.remove("on"), 900);
}

answer.addEventListener("click", () => void copyAnswer());
answer.addEventListener("keydown", (event) => {
  if (event.key === "Enter" || event.key === " ") {
    event.preventDefault();
    void copyAnswer();
  }
});

// Clicking a row copies that row's number.
rows.addEventListener("click", (event) => {
  const cell = (event.target as HTMLElement).closest<HTMLElement>("[data-row]");
  const value = cell && state.shown?.rows?.[Number(cell.dataset.row)];
  if (!cell || !value || state.stale) return;
  void navigator.clipboard.writeText(value.copy);
  cell.classList.add("copied-now");
  setTimeout(() => cell.classList.remove("copied-now"), 900);
  input.focus();
});

toggle.addEventListener("click", () => {
  state.stepsOpen = !state.stepsOpen;
  state.viewing = null;
  void update();
});

minimise.addEventListener("click", () => void invoke("hide_window"));

forget.addEventListener("click", () => void forgetKept());

// ---- window-level keys ---------------------------------------------------

window.addEventListener("keydown", (event) => {
  if (event.key === "Escape") {
    event.preventDefault();
    // Esc stops a question, or closes the settings or the full-size
    // picture; only when there is none of those does it hide the window.
    if (stopAsking()) return;
    if (closeSettings()) return;
    if (state.viewing) {
      state.viewing = null;
      void update();
      return;
    }
    void invoke("hide_window");
    return;
  }
  // Ctrl+comma opens and closes the settings.
  if (event.key === "," && event.ctrlKey && !event.altKey && !event.shiftKey) {
    event.preventDefault();
    toggleSettings();
    return;
  }
  // Ctrl+C with nothing selected copies the answer.
  const nothingSelected =
    input.selectionStart === input.selectionEnd && !window.getSelection()?.toString();
  if (event.key.toLowerCase() === "c" && event.ctrlKey && !event.altKey && nothingSelected) {
    event.preventDefault();
    void copyAnswer();
  }
});

// The window is a tool, not a page: no right-click menu.
window.addEventListener("contextmenu", (event) => event.preventDefault());

// ---- summon: focus the input with its text selected, so typing replaces it

function focusInput() {
  input.focus();
  input.select();
}

void listen("summoned", () => {
  rememberAnswered();
  // Summoned to work something out: the settings are put away.
  closeSettings();
  focusInput();
});

// Whenever the window becomes active, typing must go to the input. Without
// this, focus can land on a button and Enter would press it.
window.addEventListener("focus", () => {
  if (document.activeElement !== input) input.focus();
});

// ---- start-up ------------------------------------------------------------

async function start() {
  state.settings = await invoke<Settings>("settings_view");
  await refreshClaude();
  fitInput();
  // Focus first: the update below waits for a frame to be drawn, which can
  // be a long wait if the window is not visible yet, and by then the user
  // may have typed something that must not be selected and overwritten.
  focusInput();
  await update();
}

void start();
