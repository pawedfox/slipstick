// Reference problem set for Slipstick.
//
// This file is the single source of truth. Every expected number is computed
// here by code, never typed in by hand. Run:
//
//   node tests/reference/build.mjs
//
// to regenerate problems.json, which the app's test runner will read.
//
// Values are stored in SI base units (volts, amps, ohms, hertz, farads,
// seconds, watts, siemens). The runner compares on value, not on formatting.

import { writeFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';

// --- constants and helpers -------------------------------------------------

const K_B = 1.380649e-23; // Boltzmann constant, J/K
const Q_E = 1.602176634e-19; // elementary charge, C
const vt = (kelvin) => (K_B * kelvin) / Q_E; // thermal voltage
const VT300 = vt(300);

const TAU = 2 * Math.PI;
const par = (...rs) => 1 / rs.reduce((sum, r) => sum + 1 / r, 0);
const db20 = (x) => 20 * Math.log10(x);
const db10 = (x) => 10 * Math.log10(x);

// Minimal complex arithmetic: [re, im]
const cx = {
  add: (a, b) => [a[0] + b[0], a[1] + b[1]],
  mul: (a, b) => [a[0] * b[0] - a[1] * b[1], a[0] * b[1] + a[1] * b[0]],
  div: (a, b) => {
    const d = b[0] * b[0] + b[1] * b[1];
    return [(a[0] * b[0] + a[1] * b[1]) / d, (a[1] * b[0] - a[0] * b[1]) / d];
  },
  mag: (a) => Math.hypot(a[0], a[1]),
  deg: (a) => (Math.atan2(a[1], a[0]) * 180) / Math.PI,
};
const complexValue = (label, z, unit) => ({
  label,
  unit,
  re: z[0],
  im: z[1],
  magnitude: cx.mag(z),
  angle_deg: cx.deg(z),
});
const val = (label, value, unit) => ({ label, value, unit });

// Newton's method for f(x) = 0
function solve(f, x0) {
  let x = x0;
  for (let i = 0; i < 200; i++) {
    const h = Math.abs(x) * 1e-7 || 1e-12;
    const slope = (f(x + h) - f(x - h)) / (2 * h);
    const next = x - f(x) / slope;
    if (Math.abs(next - x) <= Math.abs(next) * 1e-13) return next;
    x = next;
  }
  throw new Error('solve() did not converge');
}

// --- problems --------------------------------------------------------------
//
// kind:
//   answer         a correct number is expected
//   must_not_flag  valid but non-standard input; any flag is a failure
//   must_flag      a definite error; the as-typed result AND the standard
//                  result are both expected
//   must_block     no number may be shown
//   memory         run with the listed memory notes switched on
//
// expect.state is the answer state from SPEC.md:
//   clean | assumed | flagged | blocked

const problems = [];
const add = (p) => problems.push(p);

// ---- basics ---------------------------------------------------------------

add({
  id: 'ohms-law',
  category: 'basics',
  kind: 'answer',
  input: 'V = 2.2mA * 4.7k',
  expect: { state: 'clean', values: [val('V', 2.2e-3 * 4.7e3, 'V')] },
});

add({
  id: 'divider',
  category: 'basics',
  kind: 'answer',
  input: 'voltage divider, 9V in, R1 = 10k on top, R2 = 4.7k to ground. vout?',
  expect: { state: 'clean', values: [val('Vout', (9 * 4.7e3) / (10e3 + 4.7e3), 'V')] },
});

add({
  id: 'parallel-three',
  category: 'basics',
  kind: 'answer',
  input: '10k || 22k || 47k',
  expect: { state: 'clean', values: [val('R', par(10e3, 22e3, 47e3), 'Ω')] },
});

add({
  id: 'led-resistor',
  category: 'basics',
  kind: 'answer',
  input: 'resistor for an LED, 9V supply, Vf = 2.0V, 5mA',
  expect: { state: 'clean', values: [val('R', (9 - 2.0) / 5e-3, 'Ω')] },
});

add({
  id: 'speaker-power',
  category: 'basics',
  kind: 'answer',
  input: 'power into an 8 ohm speaker from a 20V peak sine wave',
  expect: { state: 'clean', values: [val('P', (20 / Math.SQRT2) ** 2 / 8, 'W')] },
});

add({
  id: 'unit-convert-cap',
  category: 'basics',
  kind: 'answer',
  input: '0.047uF in nF',
  expect: { state: 'clean', values: [val('C', 0.047e-6, 'F')] },
});

// ---- filters and reactance ------------------------------------------------

add({
  id: 'rc-cutoff',
  category: 'filters',
  kind: 'answer',
  input: 'cutoff of an RC lowpass, R = 4.7k, C = 100nF',
  expect: { state: 'clean', values: [val('fc', 1 / (TAU * 4.7e3 * 100e-9), 'Hz')] },
});

add({
  id: 'rc-time-constant',
  category: 'filters',
  kind: 'answer',
  input: 'tau for 1M and 22uF',
  expect: { state: 'clean', values: [val('τ', 1e6 * 22e-6, 's')] },
});

add({
  id: 'coupling-cap',
  category: 'filters',
  kind: 'answer',
  input: 'coupling cap for a 10Hz highpass into 1M',
  expect: { state: 'clean', values: [val('C', 1 / (TAU * 10 * 1e6), 'F')] },
});

add({
  id: 'lc-resonance-pickup',
  category: 'filters',
  kind: 'answer',
  input: 'resonant frequency of a pickup, L = 2.5H, with 470pF of cable capacitance',
  expect: { state: 'clean', values: [val('f0', 1 / (TAU * Math.sqrt(2.5 * 470e-12)), 'Hz')] },
});

add({
  id: 'cap-reactance',
  category: 'filters',
  kind: 'answer',
  input: 'Xc of 22nF at 1kHz',
  expect: { state: 'clean', values: [val('Xc', 1 / (TAU * 1e3 * 22e-9), 'Ω')] },
});

add({
  id: 'rc-lowpass-attenuation',
  category: 'filters',
  kind: 'answer',
  input: 'gain in dB of an RC lowpass (R = 10k, C = 10nF) at 5kHz',
  expect: {
    state: 'clean',
    values: [val('|H|', db20(1 / Math.hypot(1, 5e3 / (1 / (TAU * 10e3 * 10e-9)))), 'dB')],
  },
});

// ---- complex impedance ----------------------------------------------------

add({
  id: 'series-rc-impedance',
  category: 'impedance',
  kind: 'answer',
  input: 'impedance of 10k in series with 100nF at 1kHz',
  expect: {
    state: 'clean',
    values: [complexValue('Z', [10e3, -1 / (TAU * 1e3 * 100e-9)], 'Ω')],
  },
});

add({
  id: 'parallel-rl-impedance',
  category: 'impedance',
  kind: 'answer',
  input: 'Z of 1k in parallel with 100mH at 2kHz',
  expect: {
    state: 'clean',
    values: [
      complexValue(
        'Z',
        (() => {
          const r = [1e3, 0];
          const xl = [0, TAU * 2e3 * 100e-3];
          return cx.div(cx.mul(r, xl), cx.add(r, xl));
        })(),
        'Ω',
      ),
    ],
  },
});

// ---- decibels and levels --------------------------------------------------

add({
  id: 'gain-to-db',
  category: 'decibels',
  kind: 'answer',
  input: 'voltage gain of 47 in dB',
  expect: { state: 'clean', values: [val('gain', db20(47), 'dB')] },
});

add({
  id: 'mw-to-dbm',
  category: 'decibels',
  kind: 'answer',
  input: '250mW in dBm',
  expect: { state: 'clean', values: [val('level', db10(250), 'dBm')] },
});

add({
  id: 'dbu-to-volts',
  category: 'decibels',
  kind: 'answer',
  input: '+4 dBu in volts rms',
  expect: { state: 'clean', values: [val('V', Math.sqrt(0.6) * 10 ** (4 / 20), 'V')] },
});

// ---- diodes ---------------------------------------------------------------

add({
  id: 'thermal-voltage',
  category: 'diodes',
  kind: 'answer',
  input: 'thermal voltage at 300K',
  expect: { state: 'clean', values: [val('Vt', VT300, 'V')] },
});

add({
  id: 'shockley-current',
  category: 'diodes',
  kind: 'answer',
  input: 'diode current: Is = 1e-14 A, Vd = 0.65V, n = 1, T = 300K',
  expect: {
    state: 'clean',
    values: [val('Id', 1e-14 * (Math.exp(0.65 / VT300) - 1), 'A')],
  },
});

add({
  id: 'shockley-voltage',
  category: 'diodes',
  kind: 'answer',
  input: 'forward voltage of a diode at 1mA, Is = 2.52nA, n = 1.752, 300K',
  expect: {
    state: 'clean',
    values: [val('Vd', 1.752 * VT300 * Math.log(1e-3 / 2.52e-9 + 1), 'V')],
  },
});

add({
  id: 'diode-resistor-loop',
  category: 'diodes',
  kind: 'answer',
  input:
    '5V source, 1k resistor in series with a diode (Is = 1e-14 A, n = 1, 300K). Solve for the current, no 0.7V approximation.',
  expect: (() => {
    const current = solve(
      (i) => 5 - i * 1e3 - VT300 * Math.log(i / 1e-14 + 1),
      4.3e-3,
    );
    // Only the current is asked for, so only the current is required.
    return { state: 'clean', values: [val('I', current, 'A')] };
  })(),
});

// ---- BJT ------------------------------------------------------------------

add({
  id: 'bjt-divider-bias',
  category: 'bjt',
  kind: 'answer',
  input:
    'NPN voltage-divider bias: Vcc = 9V, R1 = 100k, R2 = 22k, Re = 1k, Rc = 4.7k, beta = 200, Vbe = 0.65V. Find Ic and the collector voltage.',
  expect: (() => {
    const vth = (9 * 22e3) / (100e3 + 22e3);
    const rth = par(100e3, 22e3);
    const ib = (vth - 0.65) / (rth + 201 * 1e3);
    const ic = 200 * ib;
    return {
      state: 'clean',
      values: [val('Ic', ic, 'A'), val('Vc', 9 - ic * 4.7e3, 'V')],
    };
  })(),
});

add({
  id: 'bjt-gm-temp-given',
  category: 'bjt',
  kind: 'answer',
  input: 'gm of a BJT at Ic = 1mA, 300K',
  expect: { state: 'clean', values: [val('gm', 1e-3 / VT300, 'S')] },
});

add({
  id: 'bjt-gm-temp-assumed',
  category: 'bjt',
  kind: 'answer',
  input: 'gm of a BJT at Ic = 1mA',
  expect: {
    state: 'assumed',
    assumes: ['temperature (300 K, or a stated room-temperature value)'],
    values: [val('gm', 1e-3 / VT300, 'S')],
    tolerance: 0.04, // room temperature may be taken as 290–300 K
  },
});

add({
  id: 'ce-gain-unbypassed',
  category: 'bjt',
  kind: 'answer',
  input:
    'common emitter voltage gain, Rc = 4.7k, Re = 1k unbypassed, Ic = 0.89mA, 300K',
  expect: {
    state: 'clean',
    values: [val('Av', -4.7e3 / (VT300 / 0.89e-3 + 1e3), 'V/V')],
  },
});

add({
  id: 'miller-capacitance',
  category: 'bjt',
  kind: 'answer',
  input: 'Miller input capacitance, Cgd = 3pF, stage gain = -50',
  expect: { state: 'clean', values: [val('Cin', 3e-12 * (1 + 50), 'F')] },
});

// ---- JFET -----------------------------------------------------------------

add({
  id: 'jfet-square-law',
  category: 'jfet',
  kind: 'answer',
  input: 'JFET drain current, Idss = 8mA, Vp = -3V, Vgs = -1V',
  expect: { state: 'clean', values: [val('Id', 8e-3 * (1 - -1 / -3) ** 2, 'A')] },
});

add({
  id: 'jfet-gm',
  category: 'jfet',
  kind: 'answer',
  input: 'gm of a JFET with Idss = 6mA, Vgs(off) = -2.5V, running at Id = 1.2mA',
  expect: {
    state: 'clean',
    values: [val('gm', (2 / 2.5) * Math.sqrt(6e-3 * 1.2e-3), 'S')],
  },
});

add({
  id: 'jfet-self-bias',
  category: 'jfet',
  kind: 'answer',
  input:
    'source resistor to self-bias a JFET at Id = 0.2mA, Idss = 0.6mA, Vgs(off) = -0.8V',
  expect: (() => {
    const vgs = -0.8 * (1 - Math.sqrt(0.2e-3 / 0.6e-3));
    // Only the resistor is asked for, so only the resistor is required.
    return { state: 'clean', values: [val('Rs', -vgs / 0.2e-3, 'Ω')] };
  })(),
});

// ---- MOSFET ---------------------------------------------------------------

add({
  id: 'mosfet-saturation',
  category: 'mosfet',
  kind: 'answer',
  input: 'NMOS in saturation, Id = 0.5 * k * (Vgs - Vth)^2, k = 2 mA/V^2, Vgs = 2.5V, Vth = 1V',
  expect: { state: 'clean', values: [val('Id', 0.5 * 2e-3 * (2.5 - 1) ** 2, 'A')] },
});

add({
  id: 'mosfet-gm',
  category: 'mosfet',
  kind: 'answer',
  input: 'gm of that kind of NMOS: k = 2 mA/V^2 (Id = 0.5 k Vov^2), Id = 2.25mA',
  expect: { state: 'clean', values: [val('gm', Math.sqrt(2 * 2e-3 * 2.25e-3), 'S')] },
});

// ---- op-amps and noise ----------------------------------------------------

add({
  id: 'noninverting-gain',
  category: 'opamp',
  kind: 'answer',
  input: 'non-inverting gain, Rf = 100k, Rg = 2.2k, in dB',
  expect: { state: 'clean', values: [val('gain', db20(1 + 100e3 / 2.2e3), 'dB')] },
});

add({
  id: 'thermal-noise',
  category: 'noise',
  kind: 'answer',
  input: 'thermal noise voltage of a 10k resistor over 20kHz bandwidth at 300K',
  expect: {
    state: 'clean',
    values: [val('en', Math.sqrt(4 * K_B * 300 * 10e3 * 20e3), 'V')],
  },
});

// ---- must not flag: valid, non-standard ----------------------------------

add({
  id: 'nf-jfet-overdrive',
  category: 'jfet',
  kind: 'must_not_flag',
  input:
    'V_ov = |VGS(off)| × √(ID ÷ IDSS), VGS(off) = -2.5V, ID = 1.2mA, IDSS = 6mA',
  note: "The user's own rearrangement of the square law. It is correct.",
  expect: { state: 'clean', values: [val('V_ov', 2.5 * Math.sqrt(1.2e-3 / 6e-3), 'V')] },
});

add({
  id: 'nf-units-as-words',
  category: 'basics',
  kind: 'must_not_flag',
  input: 'V = A x ohms, 0.0022 A x 4700 ohms',
  note: 'Unit names used in place of symbols.',
  expect: { state: 'clean', values: [val('V', 0.0022 * 4700, 'V')] },
});

add({
  id: 'nf-25mv-approximation',
  category: 'bjt',
  kind: 'must_not_flag',
  input: 're = 25mV / Ic, Ic = 2mA',
  note: 'The 25 mV rule of thumb is a normal approximation. Compute it as typed.',
  expect: { state: 'clean', values: [val('re', 25e-3 / 2e-3, 'Ω')] },
});

add({
  id: 'nf-shortcut-constant',
  category: 'filters',
  kind: 'must_not_flag',
  input: 'Xc = 159155 / (f * C) with f in Hz and C in uF. f = 1000, C = 0.022',
  note: '159155 is 1e6 / 2π. A bench shortcut, not an error.',
  expect: { state: 'clean', values: [val('Xc', 159155 / (1000 * 0.022), 'Ω')] },
});

add({
  id: 'nf-gain-magnitude',
  category: 'jfet',
  kind: 'must_not_flag',
  input: 'gain = Rd * gm, Rd = 10k, gm = 2.1mS',
  note: 'Magnitude only, written in an unusual order. Still valid.',
  expect: { state: 'clean', values: [val('gain', 10e3 * 2.1e-3, 'V/V')] },
});

// ---- must flag: definite errors ------------------------------------------

add({
  id: 'fl-cutoff-inverted',
  category: 'filters',
  kind: 'must_flag',
  input: 'fc = 2πRC, R = 4.7k, C = 100nF',
  note: 'Units come out in seconds, not hertz.',
  expect: {
    state: 'flagged',
    reason: 'units',
    values: [val('as typed', TAU * 4.7e3 * 100e-9, 's')],
    standard: val('fc', 1 / (TAU * 4.7e3 * 100e-9), 'Hz'),
  },
});

add({
  id: 'fl-power-formula',
  category: 'basics',
  kind: 'must_flag',
  input: 'P = V / R, V = 12V, R = 8 ohms',
  note: 'V/R is current, not power.',
  expect: {
    state: 'flagged',
    reason: 'units',
    values: [val('as typed', 12 / 8, 'A')],
    standard: val('P', 12 ** 2 / 8, 'W'),
  },
});

add({
  id: 'fl-jfet-missing-square',
  category: 'jfet',
  kind: 'must_flag',
  input: 'Id = Idss * (1 - Vgs/Vp), Idss = 8mA, Vp = -3V, Vgs = -1V',
  note: 'The square is missing. Units balance, but it is not the square law.',
  expect: {
    state: 'flagged',
    reason: 'not equivalent',
    values: [val('as typed', 8e-3 * (1 - -1 / -3), 'A')],
    standard: val('Id', 8e-3 * (1 - -1 / -3) ** 2, 'A'),
  },
});

add({
  id: 'fl-impossible-collector',
  category: 'bjt',
  kind: 'must_flag',
  input: 'Vc = Vcc - Ic * Rc, Vcc = 9V, Ic = 5mA, Rc = 4.7k',
  note: 'Gives a collector below ground on a 9 V supply: the transistor would be saturated.',
  expect: {
    state: 'flagged',
    reason: 'physically impossible',
    values: [val('as typed', 9 - 5e-3 * 4.7e3, 'V')],
  },
});

// ---- must block -----------------------------------------------------------

add({
  id: 'bl-missing-capacitor',
  category: 'filters',
  kind: 'must_block',
  input: "what's the cutoff with a 10k resistor",
  expect: { state: 'blocked', needs: 'the capacitor (or inductor) value' },
});

add({
  id: 'bl-no-values',
  category: 'bjt',
  kind: 'must_block',
  input: 'gain of the stage',
  expect: { state: 'blocked', needs: 'the circuit and its values' },
});

add({
  id: 'bl-volts-plus-amps',
  category: 'basics',
  kind: 'must_block',
  input: '5V + 3mA',
  expect: { state: 'blocked', needs: 'quantities that can be added' },
});

add({
  id: 'bl-underspecified-bias',
  category: 'bjt',
  kind: 'must_block',
  input: 'bias point for a 2N3904 with R1 = 47k',
  expect: { state: 'blocked', needs: 'the supply voltage and the other resistors' },
});

// ---- memory ---------------------------------------------------------------

add({
  id: 'mem-default-vbe',
  category: 'bjt',
  kind: 'memory',
  memory_notes: ['Use V_BE = 0.6 V for silicon BJTs unless I give one.'],
  input:
    'NPN voltage-divider bias: Vcc = 9V, R1 = 100k, R2 = 22k, Re = 1k, Rc = 4.7k, beta = 200. Ic?',
  expect: (() => {
    const vth = (9 * 22e3) / (100e3 + 22e3);
    const rth = par(100e3, 22e3);
    return {
      state: 'clean',
      values: [val('Ic', (200 * (vth - 0.6)) / (rth + 201 * 1e3), 'A')],
    };
  })(),
});

add({
  id: 'mem-note-cannot-hide-error',
  category: 'filters',
  kind: 'memory',
  memory_notes: ['I write the RC cutoff as fc = 2πRC.'],
  input: 'fc = 2πRC, R = 4.7k, C = 100nF',
  note: 'A memory note must never suppress a definite-error flag.',
  expect: {
    state: 'flagged',
    reason: 'units',
    values: [val('as typed', TAU * 4.7e3 * 100e-9, 's')],
    standard: val('fc', 1 / (TAU * 4.7e3 * 100e-9), 'Hz'),
  },
});

// --- write -----------------------------------------------------------------

const ids = new Set();
for (const p of problems) {
  if (ids.has(p.id)) throw new Error(`duplicate id: ${p.id}`);
  ids.add(p.id);
  for (const v of [...(p.expect.values ?? []), ...(p.expect.standard ? [p.expect.standard] : [])]) {
    const numbers = 'value' in v ? [v.value] : [v.re, v.im];
    if (!numbers.every(Number.isFinite)) throw new Error(`non-finite value in ${p.id}`);
  }
}

const out = {
  description:
    'Generated by build.mjs. Do not edit by hand. Values are in SI base units.',
  default_tolerance: 0.002,
  count: problems.length,
  problems,
};

const here = dirname(fileURLToPath(import.meta.url));
writeFileSync(join(here, 'problems.json'), JSON.stringify(out, null, 2) + '\n');

const byKind = {};
for (const p of problems) byKind[p.kind] = (byKind[p.kind] ?? 0) + 1;
console.log(`wrote ${problems.length} problems`, byKind);
for (const p of problems) {
  const shown = (p.expect.values ?? [])
    .map((v) =>
      'value' in v
        ? `${v.label} = ${v.value.toPrecision(4)} ${v.unit}`
        : `${v.label} = ${v.re.toPrecision(4)} ${v.im < 0 ? '-' : '+'} j${Math.abs(v.im).toPrecision(4)} ${v.unit} (${v.magnitude.toPrecision(4)} ∠ ${v.angle_deg.toFixed(2)}°)`,
    )
    .join('; ');
  console.log(`${p.id.padEnd(28)} ${p.expect.state.padEnd(8)} ${shown}`);
}
