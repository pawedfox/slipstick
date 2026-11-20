// Runs the reference problems through a real model and grades the answers.
//
//   node tests/reference/run.mjs --model haiku
//   node tests/reference/run.mjs --model sonnet --effort low --only rc-cutoff,divider
//
// For each problem it asks the user's own installed Claude Code (so it uses
// their plan, not an API key) with the app's instructions, runs the reply
// through the app's engine, and compares the result with problems.json.
// Tools are switched off: the model may only write formulas.

import { spawn, spawnSync } from 'node:child_process';
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, '..', '..');
const solveExe = join(root, 'src-tauri', 'target', 'debug', 'examples', 'solve.exe');
const instructions = join(root, 'src-tauri', 'prompts', 'solver.md');

const option = (name, fallback) => {
  const at = process.argv.indexOf(`--${name}`);
  return at >= 0 ? process.argv[at + 1] : fallback;
};
const model = option('model', 'haiku');
const effort = option('effort', null);
const only = option('only', null)?.split(',');
const concurrency = Number(option('concurrency', 4));

// An empty folder, so no project instructions are picked up by Claude Code.
const neutral = join(tmpdir(), 'slipstick-reference');
mkdirSync(neutral, { recursive: true });

const { problems, default_tolerance } = JSON.parse(readFileSync(join(here, 'problems.json'), 'utf8'));

function askModel(input) {
  return new Promise((resolve) => {
    const args = [
      '-p', '--output-format', 'json', '--model', model,
      '--tools', '""', '--strict-mcp-config', '--no-session-persistence',
      '--system-prompt-file', instructions,
    ];
    if (effort) args.push('--effort', effort);
    const started = Date.now();
    const child = spawn('claude', args, { shell: true, cwd: neutral });
    let out = '';
    child.stdout.on('data', (chunk) => (out += chunk));
    child.stderr.on('data', () => {});
    child.on('close', () => {
      const wall = Date.now() - started;
      try {
        const reply = JSON.parse(out);
        resolve({
          text: String(reply.result ?? ''),
          wall,
          apiMs: reply.duration_api_ms,
          turns: reply.num_turns,
          outputTokens: reply.usage?.output_tokens,
          cost: reply.total_cost_usd,
        });
      } catch {
        resolve({ text: '', wall, error: out.slice(0, 200) });
      }
    });
    child.stdin.end(input);
  });
}

/** The instant path's answer, shaped like a solved reply, or null. */
const instant = (input) => {
  const run = spawnSync(solveExe, ['--instant'], { input, encoding: 'utf8' });
  try {
    const found = JSON.parse(run.stdout);
    return found && { state: 'clean', result: { ...found, name: 'instant' }, also: [], steps: [] };
  } catch {
    return null;
  }
};

const solve = (reply) => {
  const run = spawnSync(solveExe, { input: reply, encoding: 'utf8' });
  try {
    return JSON.parse(run.stdout);
  } catch {
    return { state: 'failed', message: 'the engine did not return a result' };
  }
};

const close = (got, want, tolerance) =>
  want === 0 ? Math.abs(got) < 1e-12 : Math.abs(got - want) <= Math.abs(want) * tolerance;

/** Returns a list of what is wrong. Empty means the problem passed. */
function grade(problem, solved) {
  const { expect } = problem;
  const tolerance = expect.tolerance ?? default_tolerance;
  const faults = [];

  if (expect.state === 'blocked') {
    if (solved.state !== 'blocked') faults.push(`gave an answer (${solved.state}) where it should have blocked`);
    return faults;
  }
  if (solved.state === 'blocked') return [`blocked: ${solved.message}`];
  if (solved.state === 'failed') return [`failed: ${solved.message}`];

  if (expect.state === 'flagged' && solved.state !== 'flagged') faults.push('did not flag a definite error');
  if (expect.state !== 'flagged' && solved.state === 'flagged') faults.push(`flagged a valid input: ${solved.flag}`);
  if (expect.state === 'assumed' && solved.state === 'clean') faults.push('did not say what it assumed');

  const given = [solved.result, ...(solved.results ?? []), ...(solved.also ?? [])]
    .filter(Boolean)
    .map((v) => v.si_value);
  for (const want of expect.values ?? []) {
    // A complex value passes if its magnitude is among the results.
    const target = 'value' in want ? want.value : want.magnitude;
    const pool = expect.state === 'flagged' ? [solved.result?.si_value] : given;
    if (!pool.some((got) => typeof got === 'number' && close(got, target, tolerance))) {
      faults.push(`${want.label}: wanted ${target.toPrecision(5)}, got ${pool.map((g) => g?.toPrecision?.(5)).join(', ')}`);
    }
  }
  if (expect.standard) {
    const got = solved.standard?.si_value;
    if (typeof got !== 'number' || !close(got, expect.standard.value, tolerance)) {
      faults.push(`standard form: wanted ${expect.standard.value.toPrecision(5)}, got ${got?.toPrecision?.(5)}`);
    }
  }
  return faults;
}

// Memory is not built yet, so those problems are left out.
const selected = problems.filter((p) => p.kind !== 'memory' && (!only || only.includes(p.id)));
const results = new Array(selected.length);
let next = 0;

async function worker() {
  while (next < selected.length) {
    const index = next++;
    const problem = selected[index];
    // The app answers plain maths itself and only asks Claude for the rest.
    const local = instant(problem.input);
    const asked = local ? { wall: 0, local: true } : await askModel(problem.input);
    const solved = local ?? (asked.error ? { state: 'failed', message: `no reply: ${asked.error}` } : solve(asked.text));
    const faults = grade(problem, solved);
    // An extra, unneeded assumption note is worth knowing about but is not wrong.
    const extraNote = problem.expect.state === 'clean' && solved.state === 'assumed';
    results[index] = { problem, asked, solved, faults, extraNote };
    const shown = solved.result ? `${solved.result.number} ${solved.result.unit}`.trim() : solved.state;
    const how = asked.local ? 'instant' : `${String(asked.wall).padStart(5)} ms`;
    console.log(`${faults.length ? 'FAIL' : 'pass'}  ${problem.id.padEnd(28)} ${how.padStart(8)}  ${shown}`);
    for (const fault of faults) console.log(`        ${fault}`);
  }
}

console.log(`model: ${model}${effort ? `, effort ${effort}` : ''}, ${selected.length} problems\n`);
await Promise.all(Array.from({ length: concurrency }, worker));

const passed = results.filter((r) => r.faults.length === 0);
const times = results.map((r) => r.asked.apiMs).filter(Number.isFinite).sort((a, b) => a - b);
const sum = (list) => list.reduce((total, n) => total + (n ?? 0), 0);
const median = times[Math.floor(times.length / 2)];
const byKind = {};
for (const r of results) {
  const kind = (byKind[r.problem.kind] ??= { passed: 0, total: 0 });
  kind.total += 1;
  if (r.faults.length === 0) kind.passed += 1;
}

console.log(`\n${passed.length} of ${results.length} passed`);
for (const [kind, count] of Object.entries(byKind)) console.log(`  ${kind}: ${count.passed}/${count.total}`);
console.log(`answered instantly, without the model: ${results.filter((r) => r.asked.local).length}`);
console.log(`extra assumption notes on clean problems: ${results.filter((r) => r.extraNote).length}`);
console.log(`model time per problem: median ${median} ms, slowest ${times.at(-1)} ms`);
console.log(`output tokens per problem: ${Math.round(sum(results.map((r) => r.asked.outputTokens)) / results.length)}`);
console.log(`cost at API prices, as reported by Claude Code: $${sum(results.map((r) => r.asked.cost)).toFixed(4)} for the run`);
console.log(`more than one turn (should be 0): ${results.filter((r) => r.asked.turns > 1).length}`);

const outDir = join(here, 'results');
mkdirSync(outDir, { recursive: true });
const outFile = join(outDir, `${model}${effort ? `-${effort}` : ''}.json`);
writeFileSync(
  outFile,
  JSON.stringify(
    results.map((r) => ({
      id: r.problem.id,
      kind: r.problem.kind,
      input: r.problem.input,
      passed: r.faults.length === 0,
      faults: r.faults,
      reply: r.asked.text,
      solved: r.solved,
      apiMs: r.asked.apiMs,
    })),
    null,
    2,
  ),
);
console.log(`details: ${outFile}`);
