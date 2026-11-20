// A stand-in for the app's native side, used only when the window is opened
// in a plain browser during development (see tools/dev-bridge.mjs).
//
// Inside the real app Tauri is present and this does nothing. It is left
// out of release builds altogether.

const BRIDGE = "http://localhost:1430";

type Arguments = Record<string, unknown>;

async function post(path: string, payload: unknown): Promise<unknown> {
  const response = await fetch(`${BRIDGE}${path}`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(payload),
  });
  return response.json();
}

export function installDevBridge() {
  const target = window as unknown as { __TAURI_INTERNALS__?: unknown };
  if (target.__TAURI_INTERNALS__) return; // running inside the real app

  // Whether the user has "connected" Claude in this browser session.
  let connected = false;

  // The settings, for this browser session only. The engine and Claude are
  // real, so significant figures and the model really take effect; the
  // shortcut and "Start with Windows" are only remembered here.
  const settings = {
    hotkey: "Ctrl+Alt+C",
    hotkey_registered: true,
    start_with_windows: false,
    theme: "system",
    figures: 4,
    correction: "flag",
    model: "fast",
  };

  /** `Ctrl+Alt+KeyC` as people write it: `Ctrl+Alt+C`. */
  const label = (keys: string) =>
    keys
      .split("+")
      .map((part) => part.replace(/^(Key|Digit)/, ""))
      .join("+");

  const commands: Record<string, (args: Arguments) => unknown> = {
    set_visible_height: () => null,
    hide_window: () => null,
    evaluate: (args) => post("/evaluate", { input: args.input, figures: settings.figures }),
    claude_status: () => ({ found: true, connected }),
    connect_claude: () => {
      connected = true;
      return null;
    },
    disconnect_claude: () => {
      connected = false;
      return null;
    },
    warm_claude: () => null,
    cancel_claude: () => null,
    ask_claude: async (args) => {
      const reply = (await post("/ask", {
        ...args,
        figures: settings.figures,
        deeper: settings.model === "deeper",
      })) as { error?: string };
      if (reply.error) throw reply.error;
      return reply;
    },
    settings_view: () => ({ ...settings }),
    pause_hotkey: () => null,
    change_setting: (args) => {
      const { name, value } = args.change as { name: keyof typeof settings; value: never };
      if (name === "hotkey") {
        const keys = String(value);
        // The same two refusals as the real thing. Ctrl+Alt+T stands for a
        // shortcut another program holds, so that message can be seen.
        const held = keys.split("+").filter((part) => ["Ctrl", "Alt", "Shift", "Super"].includes(part));
        if (held.length < 2 || !/\b(Ctrl|Alt)\b/.test(keys)) {
            throw "A shortcut needs two of Ctrl, Alt and Shift, such as Ctrl+Alt+C.";
        }
        if (keys === "Ctrl+Alt+KeyT") throw "That shortcut is in use by another program.";
        settings.hotkey = label(keys);
      } else {
        settings[name] = value;
      }
      return { ...settings };
    },
    // Listening for events from the native side: there are none here.
    "plugin:event|listen": () => 0,
    "plugin:event|unlisten": () => null,
  };

  target.__TAURI_INTERNALS__ = {
    invoke: async (command: string, args: Arguments = {}) => {
      const handler = commands[command];
      if (!handler) throw `dev bridge: no stand-in for "${command}"`;
      return handler(args);
    },
    transformCallback: () => 0,
  };
}
