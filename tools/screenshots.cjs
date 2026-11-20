// Takes the README's screenshots (docs/*.png) from the real app, started with
// its window hidden. Each picture is the shell itself with a transparent background,
// at twice the size, so it sits cleanly on a light or a dark page.
// Nothing is shown and nothing takes the keyboard, mouse or focus.
//
// Claude is asked for real, through the Claude Code on this PC.
//
// The app allows one running copy per identifier, so this uses a build made
// under a test identifier, which can run beside an installed Slipstick:
//
//   npm run tauri build -- --no-bundle --config tools/shots.conf.json
//   node tools/screenshots.cjs
//
// Build normally again afterwards (npm run tauri build) before making an
// installer: the test build must never be the one that is released.
const { spawn, execFileSync } = require("child_process");
const fs = require("fs");
const path = require("path");

const exe = path.join(__dirname, "..", "src-tauri", "target", "release", "slipstick.exe");
const out = path.join(__dirname, "..", "docs");
const PORT = 9335;
const wait = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

const folder = path.join(process.env.APPDATA, "io.github.pawedfox.slipstick.shots");
const file = path.join(folder, "settings.json");
const had = fs.existsSync(file) ? fs.readFileSync(file) : null;
fs.mkdirSync(folder, { recursive: true });
fs.writeFileSync(file, JSON.stringify({
  claude: "claude_code", hotkey: "Ctrl+Alt+Shift+F21", theme: "dark", figures: 4,
}));
fs.mkdirSync(out, { recursive: true });

const child = spawn(exe, ["--hidden"], {
  env: { ...process.env, WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${PORT}` },
  stdio: "ignore",
});
const tidy = () => {
  try { execFileSync("taskkill", ["/PID", String(child.pid), "/T", "/F"], { stdio: "ignore" }); } catch {}
  if (had) fs.writeFileSync(file, had); else fs.rmSync(folder, { recursive: true, force: true });
  fs.rmSync(path.join(process.env.LOCALAPPDATA, "io.github.pawedfox.slipstick.shots"), { recursive: true, force: true });
};

(async () => {
  let target;
  for (let i = 0; i < 60 && !target; i++) {
    await wait(500);
    try {
      const list = await (await fetch(`http://127.0.0.1:${PORT}/json`)).json();
      target = list.find((entry) => entry.type === "page");
    } catch {}
  }
  if (!target) throw new Error("the web view never appeared on the debugging port");

  const socket = new WebSocket(target.webSocketDebuggerUrl);
  await new Promise((resolve, reject) => { socket.onopen = resolve; socket.onerror = reject; });
  let next = 0;
  const waiting = new Map();
  socket.onmessage = ({ data }) => {
    const message = JSON.parse(data);
    if (message.id && waiting.has(message.id)) { waiting.get(message.id)(message); waiting.delete(message.id); }
  };
  const send = (method, params = {}) =>
    new Promise((resolve, reject) => {
      const id = ++next;
      const timer = setTimeout(() => reject(new Error(method + " did not answer")), 20000);
      waiting.set(id, (message) => { clearTimeout(timer); resolve(message); });
      socket.send(JSON.stringify({ id, method, params }));
    });
  const run = async (expression) => {
    const reply = await send("Runtime.evaluate", { expression, awaitPromise: true, returnByValue: true });
    if (reply.result?.exceptionDetails) throw new Error(reply.result.exceptionDetails.exception?.description ?? "page error");
    return reply.result?.result?.value;
  };

  let address = "";
  for (let i = 0; i < 40 && !address.startsWith("http"); i++) { await wait(500); address = (await run("location.href")) || ""; }
  await wait(2500);
  await send("Emulation.setDefaultBackgroundColorOverride", { color: { r: 0, g: 0, b: 0, a: 0 } });

  // Types into the box the way a person would, as far as the page can tell.
  const type = (text) => run(`(() => { const box = document.querySelector("#input"); box.value = ${JSON.stringify(text)};
    box.dispatchEvent(new Event("input", { bubbles: true })); })()`);
  const enter = () => run(`document.querySelector("#input").dispatchEvent(
    new KeyboardEvent("keydown", { key: "Enter", bubbles: true, cancelable: true }))`);
  const answered = async (seconds) => {
    for (let i = 0; i < seconds * 2; i++) {
      await wait(500);
      const state = await run(`(() => { const r = document.querySelector("#readout"), m = document.querySelector("#message");
        return r.hidden ? (m.textContent && !m.classList.contains("working") ? "message: " + m.textContent : "") : "answer"; })()`);
      if (state) return state;
    }
    return "timed out";
  };
  const shot = async (name) => {
    await wait(400);
    const box = await run(`(() => { const r = document.querySelector("#shell").getBoundingClientRect();
      return { x: r.x, y: r.y, width: r.width, height: r.height }; })()`);
    const margin = 12; // room for the shadow
    const reply = await send("Page.captureScreenshot", {
      format: "png",
      clip: { x: box.x - margin, y: box.y - margin, width: box.width + margin * 2, height: box.height + margin * 2, scale: 2 },
    });
    if (!reply.result?.data) throw new Error("no picture came back for " + name);
    fs.writeFileSync(path.join(out, name), Buffer.from(reply.result.data, "base64"));
    return `${name}: ${Math.round(box.width)}x${Math.round(box.height)} shell, ${Math.round(Buffer.byteLength(reply.result.data, "base64") / 1024)} KB`;
  };

  const report = [];
  await type("1/(2*pi*4.7kohm*100nF)");
  await wait(800);
  report.push(await shot("instant.png"));

  await type("P = V / R, V = 12 V, R = 8 ohm");
  await enter();
  report.push("flagged -> " + (await answered(45)));
  report.push(await shot("flagged.png"));

  await type("9 V across a divider of 10k over 4.7k, loaded by 22k. Voltage at the tap?");
  await enter();
  report.push("steps -> " + (await answered(45)));
  await run(`(() => { const t = document.querySelector("#toggle"); if (!t.hidden) t.click(); })()`);
  report.push(await shot("steps.png"));

  console.log(report.join("\n"));
  socket.close();
})()
  .catch((error) => console.log("FAILED:", error.message))
  .finally(() => { tidy(); process.exit(0); });
