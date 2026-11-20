// A stand-in for the app's native side, for testing the window in a browser.
//
//   node tools/dev-bridge.mjs        (then `npm run dev`, and open localhost:1420)
//
// The window normally talks to Rust through Tauri. In a plain browser there
// is no Tauri, so `src/dev-bridge.ts` sends the same requests here instead,
// and this passes them to the real engine through the command-line front
// (`src-tauri/examples/solve.rs`). The maths and Claude's answers are
// therefore the real ones; only the window frame is missing.
//
// This exists so the window can be tested without sending key presses or
// mouse clicks to the desktop of someone who is working on the same PC.
// It is a development tool. It is not part of the app and listens only on
// this machine.

import { spawn } from 'node:child_process';
import { createServer } from 'node:http';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const solveExe = join(root, 'src-tauri', 'target', 'debug', 'examples', 'solve.exe');
const PORT = 1430;
const WINDOW_ORIGIN = 'http://localhost:1420';

/** Runs the command-line front with `input` on standard input. */
function solve(args, input) {
  return new Promise((resolve) => {
    const child = spawn(solveExe, args);
    let out = '';
    child.stdout.on('data', (chunk) => (out += chunk));
    child.on('error', () => resolve(''));
    child.on('close', () => resolve(out.trim()));
    child.stdin.end(input);
  });
}

const body = (request) =>
  new Promise((resolve) => {
    let text = '';
    request.on('data', (chunk) => (text += chunk));
    request.on('end', () => resolve(text));
  });

createServer(async (request, response) => {
  response.setHeader('Access-Control-Allow-Origin', WINDOW_ORIGIN);
  response.setHeader('Access-Control-Allow-Headers', 'content-type');
  if (request.method === 'OPTIONS') return response.end();

  const send = (value, type = 'application/json') => {
    response.setHeader('Content-Type', type);
    response.end(value);
  };

  // The two settings that change answers go along with each request.
  const options = ({ figures, deeper }) => [
    ...(Number.isInteger(figures) ? ['--figures', String(figures)] : []),
    ...(deeper ? ['--deeper'] : []),
  ];

  if (request.url === '/evaluate') {
    const asked = JSON.parse(await body(request));
    return send((await solve(['--instant', ...options(asked)], asked.input)) || 'null');
  }
  if (request.url === '/ask') {
    // The whole request (problem and any image) goes through as JSON.
    const text = await body(request);
    const out = await solve(['--ask', ...options(JSON.parse(text))], text);
    const isJson = out.startsWith('{');
    return send(isJson ? out : JSON.stringify({ error: out.replace(/^error: /, '') || 'no answer' }));
  }
  // Test pictures: /fixtures/formula.png and so on. Names only, no paths.
  const fixture = /^\/fixtures\/([\w-]+\.png)$/.exec(request.url ?? '');
  if (fixture) {
    try {
      return send(readFileSync(join(root, 'tests', 'fixtures', fixture[1])), 'image/png');
    } catch {
      // fall through to "not found"
    }
  }
  response.statusCode = 404;
  response.end();
}).listen(PORT, '127.0.0.1', () => console.log(`dev bridge on http://localhost:${PORT}`));
