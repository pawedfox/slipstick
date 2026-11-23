//! The instant path: plain maths, answered locally as the user types.
//!
//! Handles numbers with SI prefixes (`4.7k`, `100n`, `4k7`), electrical and
//! mechanical units with dimension checking (`2.2mA * 4.7kohm` is volts,
//! `6 bar * pi * (25mm)^2` is newtons), conversions (`3.5 bar to psi`), the
//! usual operators, `||` for parallel, and a few functions.
//!
//! It is deliberately strict. Anything it does not fully understand returns
//! `None`, and the input is left for Claude. It never guesses: a wrong number
//! shown instantly would be worse than no number.

use serde::Serialize;

/// How many significant figures answers are shown to, unless the user has
/// chosen otherwise.
pub const DEFAULT_SIG_FIGS: u8 = 4;
/// The fewest and the most the user may choose.
pub const SIG_FIGS_RANGE: std::ops::RangeInclusive<u8> = 3..=6;

thread_local! {
    /// The significant figures in force on this thread. Kept per thread so
    /// that one piece of work cannot change another's answers part-way.
    static SIG_FIGS: std::cell::Cell<i32> = const { std::cell::Cell::new(DEFAULT_SIG_FIGS as i32) };
}

fn sig_figs() -> i32 {
    SIG_FIGS.get()
}

/// Runs `work` with answers shown, and copied, to `figures` significant
/// figures. The values carried from one step to the next are never rounded.
pub fn with_sig_figs<T>(figures: u8, work: impl FnOnce() -> T) -> T {
    let figures = figures.clamp(*SIG_FIGS_RANGE.start(), *SIG_FIGS_RANGE.end());
    let before = SIG_FIGS.replace(figures as i32);
    let result = work();
    SIG_FIGS.set(before);
    result
}

/// Exponents of: kilogram, metre, second, ampere, angle, kelvin.
///
/// Angle is tracked as its own dimension so that `rpm` (an angular speed)
/// cannot be silently mixed up with `Hz`; the two differ by 2π.
type Dim = [i8; 6];
const NO_DIM: Dim = [0; 6];
const ANGLE: usize = 4;

const LENGTH: Dim = [0, 1, 0, 0, 0, 0];
const AREA: Dim = [0, 2, 0, 0, 0, 0];
const VOLUME: Dim = [0, 3, 0, 0, 0, 0];
const MASS: Dim = [1, 0, 0, 0, 0, 0];
const TIME: Dim = [0, 0, 1, 0, 0, 0];
const FREQUENCY: Dim = [0, 0, -1, 0, 0, 0];
const SPEED: Dim = [0, 1, -1, 0, 0, 0];
const ACCELERATION: Dim = [0, 1, -2, 0, 0, 0];
const FLOW: Dim = [0, 3, -1, 0, 0, 0];
const FORCE: Dim = [1, 1, -2, 0, 0, 0];
const PRESSURE: Dim = [1, -1, -2, 0, 0, 0];
const ENERGY: Dim = [1, 2, -2, 0, 0, 0];
const POWER: Dim = [1, 2, -3, 0, 0, 0];
const CURRENT: Dim = [0, 0, 0, 1, 0, 0];
const CHARGE: Dim = [0, 0, 1, 1, 0, 0];
const VOLTAGE: Dim = [1, 2, -3, -1, 0, 0];
const RESISTANCE: Dim = [1, 2, -3, -2, 0, 0];
const CONDUCTANCE: Dim = [-1, -2, 3, 2, 0, 0];
const CAPACITANCE: Dim = [-1, -2, 4, 2, 0, 0];
const INDUCTANCE: Dim = [1, 2, -2, -2, 0, 0];
const MAGNETIC_FLUX: Dim = [1, 2, -2, -1, 0, 0];
const FLUX_DENSITY: Dim = [1, 0, -2, -1, 0, 0];
const ANGLE_DIM: Dim = [0, 0, 0, 0, 1, 0];
const ANGULAR_SPEED: Dim = [0, 0, -1, 0, 1, 0];
/// A difference in temperature, in kelvin.
const TEMPERATURE: Dim = [0, 0, 0, 0, 0, 1];
/// Kelvin per watt.
const THERMAL_RESISTANCE: Dim = [-1, -2, 3, 0, 0, 1];

#[derive(Clone, Copy, Debug)]
struct Quantity {
    /// In SI base units.
    value: f64,
    dim: Dim,
    /// Written with a `%`. Adding a percentage to a plain number is
    /// ambiguous (`200 + 15%`), so that combination is refused.
    percent: bool,
    /// Set when this is a reading on the Celsius or Fahrenheit scale, such
    /// as `25 C`, and not an amount of temperature. `value` is then the
    /// reading in kelvin-sized steps from that scale's own zero, so `25 C`
    /// is 25 and `-40 °F` is -22.2. A reading can have a difference added
    /// or taken away, and two readings can be subtracted. Nothing else.
    reading: Option<Scale>,
    /// The imaginary part, in the same units as `value`, which is then
    /// the real part. Zero for everything but a complex number, and always
    /// zero in a program: only typed input has a `j`.
    im: f64,
}

impl Quantity {
    fn plain(value: f64) -> Self {
        Self::of(value, NO_DIM)
    }

    fn of(value: f64, dim: Dim) -> Self {
        Self::complex(value, 0.0, dim)
    }

    fn complex(value: f64, im: f64, dim: Dim) -> Self {
        Self { value, dim, percent: false, reading: None, im }
    }

    /// The size of a complex number, or of a real one without its sign.
    fn magnitude(&self) -> f64 {
        self.value.hypot(self.im)
    }

    /// The magnitude of a complex number; a real one as it is, sign and all.
    fn magnitude_or_value(&self) -> f64 {
        if self.im == 0.0 { self.value } else { self.magnitude() }
    }

    /// With a part that is only rounding noise beside the other set to
    /// zero, so that `10 @ 90°` is j10 and `(1 + j)(1 - j)` is 2.
    fn settled(mut self) -> Self {
        let noise = self.magnitude() * 1e-12;
        if self.im.abs() <= noise {
            self.im = 0.0;
        } else if self.value.abs() <= noise {
            self.value = 0.0;
        }
        self
    }

    /// The reading as an absolute temperature in kelvin.
    fn kelvin(&self) -> Option<f64> {
        self.reading.map(|scale| self.value + scale.zero())
    }
}

/// What the window shows and what a copy puts on the clipboard.
#[derive(Debug, PartialEq, Serialize)]
pub struct Answer {
    /// The number, for example `338.6`. For a complex answer, its
    /// magnitude.
    pub number: String,
    /// SI prefix plus unit symbol, for example `kΩ`. May be empty.
    pub unit: String,
    /// A plain number for pasting. See `SPEC.md` for which value this is.
    pub copy: String,
    /// For a complex answer: its angle, for example `−9.043°`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub angle: Option<String>,
    /// For a complex answer: its real and imaginary parts, for example
    /// `10.00 − j1.592 kΩ`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parts: Option<String>,
    /// The input was a formula with its values after it. It is worked out
    /// as typed, right or wrong, so the window lets Enter send it on to be
    /// checked.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub given: bool,
}

impl Answer {
    fn real(number: String, unit: String, copy: String) -> Self {
        Self { number, unit, copy, angle: None, parts: None, given: false }
    }
}

// ---- units ------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq)]
enum Family {
    Neutral,
    Electrical,
    Mechanical,
}

struct Unit {
    /// Accepted spellings. The first is the one shown in answers.
    names: &'static [&'static str],
    /// Multiply by this to get SI base units.
    factor: f64,
    dim: Dim,
    /// Takes SI prefixes (`mV`, `kN`) and is shown with an automatic one.
    prefixable: bool,
    family: Family,
    /// Seeing this unit suggests an energy result is joules, not newton
    /// metres. The two have the same dimensions.
    joules: bool,
}

const fn unit(
    names: &'static [&'static str],
    factor: f64,
    dim: Dim,
    prefixable: bool,
    family: Family,
) -> Unit {
    Unit { names, factor, dim, prefixable, family, joules: false }
}

const fn energy_hint(mut u: Unit) -> Unit {
    u.joules = true;
    u
}

use Family::{Electrical, Mechanical, Neutral};

const PI: f64 = std::f64::consts::PI;
const INCH: f64 = 0.0254;
const FOOT: f64 = 0.3048;
const POUND: f64 = 0.453_592_37;
const POUND_FORCE: f64 = 4.448_221_615_260_5;
const US_GALLON: f64 = 3.785_411_784e-3;

/// `m` (metre) must stay first: `METRE` below is its index.
static UNITS: &[Unit] = &[
    unit(&["m", "meter", "metre", "meters", "metres"], 1.0, LENGTH, true, Mechanical),
    // Electrical
    energy_hint(unit(&["V", "volt", "volts"], 1.0, VOLTAGE, true, Electrical)),
    energy_hint(unit(&["A", "amp", "amps"], 1.0, CURRENT, true, Electrical)),
    energy_hint(unit(&["Ω", "\u{2126}", "ohm", "ohms", "Ohm", "Ohms"], 1.0, RESISTANCE, true, Electrical)),
    energy_hint(unit(&["F"], 1.0, CAPACITANCE, true, Electrical)),
    energy_hint(unit(&["H"], 1.0, INDUCTANCE, true, Electrical)),
    energy_hint(unit(&["S"], 1.0, CONDUCTANCE, true, Electrical)),
    unit(&["Wb"], 1.0, MAGNETIC_FLUX, true, Electrical),
    // Written out, because a lone T is the prefix tera.
    unit(&["tesla", "teslas"], 1.0, FLUX_DENSITY, false, Electrical),
    // Time, frequency, power, energy
    unit(&["s", "sec", "secs"], 1.0, TIME, true, Neutral),
    unit(&["min", "mins"], 60.0, TIME, false, Neutral),
    unit(&["h", "hr", "hrs"], 3600.0, TIME, false, Neutral),
    unit(&["Hz"], 1.0, FREQUENCY, true, Neutral),
    energy_hint(unit(&["W"], 1.0, POWER, true, Neutral)),
    energy_hint(unit(&["J"], 1.0, ENERGY, true, Neutral)),
    energy_hint(unit(&["Wh"], 3600.0, ENERGY, true, Neutral)),
    energy_hint(unit(&["hp"], 745.699_871_582_27, POWER, false, Mechanical)),
    energy_hint(unit(&["PS"], 735.498_75, POWER, false, Mechanical)),
    // Length
    unit(&["cm"], 0.01, LENGTH, false, Mechanical),
    unit(&["in", "inch", "inches"], INCH, LENGTH, false, Mechanical),
    unit(&["ft", "foot", "feet"], FOOT, LENGTH, false, Mechanical),
    unit(&["thou", "mil", "mils"], INCH / 1000.0, LENGTH, false, Mechanical),
    // Mass
    unit(&["g", "gram", "grams"], 1e-3, MASS, true, Mechanical),
    unit(&["lb", "lbs", "lbm"], POUND, MASS, false, Mechanical),
    unit(&["oz"], POUND / 16.0, MASS, false, Mechanical),
    // Force
    unit(&["N"], 1.0, FORCE, true, Mechanical),
    unit(&["lbf"], POUND_FORCE, FORCE, false, Mechanical),
    unit(&["kgf"], 9.806_65, FORCE, false, Mechanical),
    // Pressure
    unit(&["Pa"], 1.0, PRESSURE, true, Mechanical),
    unit(&["bar"], 1e5, PRESSURE, false, Mechanical),
    unit(&["mbar"], 100.0, PRESSURE, false, Mechanical),
    unit(&["psi"], POUND_FORCE / (INCH * INCH), PRESSURE, false, Mechanical),
    unit(&["atm"], 101_325.0, PRESSURE, false, Mechanical),
    // Torque
    unit(&["N·m", "Nm"], 1.0, ENERGY, true, Mechanical),
    unit(&["lb·ft", "ft·lb", "lbft", "ftlb", "ftlbs", "lbf·ft"], POUND_FORCE * FOOT, ENERGY, false, Mechanical),
    unit(&["lb·in", "in·lb", "lbin", "inlb", "inlbs", "lbf·in"], POUND_FORCE * INCH, ENERGY, false, Mechanical),
    unit(&["oz·in", "ozin"], POUND_FORCE * INCH / 16.0, ENERGY, false, Mechanical),
    // Angle and rotation
    unit(&["rad"], 1.0, ANGLE_DIM, false, Mechanical),
    unit(&["deg", "°", "degree", "degrees"], PI / 180.0, ANGLE_DIM, false, Mechanical),
    unit(&["rev", "revs", "turn", "turns"], 2.0 * PI, ANGLE_DIM, false, Mechanical),
    unit(&["rpm", "RPM"], 2.0 * PI / 60.0, ANGULAR_SPEED, false, Mechanical),
    // Volume and flow
    unit(&["L", "l", "liter", "litre", "liters", "litres"], 1e-3, VOLUME, true, Mechanical),
    unit(&["gal"], US_GALLON, VOLUME, false, Mechanical),
    unit(&["cc"], 1e-6, VOLUME, false, Mechanical),
    unit(&["L/min", "lpm", "LPM"], 1e-3 / 60.0, FLOW, false, Mechanical),
    unit(&["gal/min", "gpm", "GPM"], US_GALLON / 60.0, FLOW, false, Mechanical),
    unit(&["cfm", "CFM"], FOOT * FOOT * FOOT / 60.0, FLOW, false, Mechanical),
    // Speed
    unit(&["mph"], 0.447_04, SPEED, false, Mechanical),
    unit(&["km/h", "kph"], 1.0 / 3.6, SPEED, false, Mechanical),
];

const METRE: usize = 0;

fn unit_index(name: &str) -> Option<usize> {
    UNITS.iter().position(|u| u.names.contains(&name))
}

/// Case matters: `m` is milli and `M` is mega. `K` is accepted as kilo
/// because it is so commonly typed that way.
fn prefix(c: char) -> Option<f64> {
    Some(match c {
        'f' => 1e-15,
        'p' => 1e-12,
        'n' => 1e-9,
        'u' | 'µ' | 'μ' => 1e-6,
        'm' => 1e-3,
        'k' | 'K' => 1e3,
        'M' => 1e6,
        'G' => 1e9,
        'T' => 1e12,
        _ => return None,
    })
}

fn prefix_symbol(scale: f64) -> &'static str {
    match scale.log10().round() as i32 {
        -15 => "f",
        -12 => "p",
        -9 => "n",
        -6 => "µ",
        -3 => "m",
        3 => "k",
        6 => "M",
        9 => "G",
        12 => "T",
        _ => "",
    }
}

/// How a word that follows a number can be read.
enum Suffix {
    /// A lone `m`: milli or metres, depending on the rest of the input.
    BareM,
    /// A prefix with no unit, as in `4.7k`.
    BarePrefix(f64),
    /// A unit, with the scale of any prefix in front of it.
    Unit { scale: f64, index: usize },
}

fn lookup(word: &str) -> Option<Suffix> {
    if word == "m" {
        return Some(Suffix::BareM);
    }
    if let Some(index) = unit_index(word) {
        return Some(Suffix::Unit { scale: 1.0, index });
    }
    if word.eq_ignore_ascii_case("meg") {
        return Some(Suffix::BarePrefix(1e6));
    }
    let prefixed = |scale: f64, rest: &str| {
        unit_index(rest)
            .filter(|&index| UNITS[index].prefixable)
            .map(|index| Suffix::Unit { scale, index })
    };
    if word.len() > 3 && word.is_char_boundary(3) && word[..3].eq_ignore_ascii_case("meg") {
        return prefixed(1e6, &word[3..]);
    }
    let mut chars = word.chars();
    let scale = prefix(chars.next()?)?;
    let rest = chars.as_str();
    if rest.is_empty() {
        Some(Suffix::BarePrefix(scale))
    } else {
        prefixed(scale, rest)
    }
}

fn is_dimensioned_unit(word: &str) -> bool {
    matches!(lookup(word), Some(Suffix::BareM | Suffix::Unit { .. }))
}

// ---- lexer ------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
enum Token {
    Number(f64),
    Word(String),
    /// A named value from an earlier step of a program, such as `V_th`.
    Name(String),
    Plus,
    Minus,
    Star,
    Slash,
    Caret,
    /// A power written as `²` or `³`, or as a digit on a unit (`mm2`).
    Super(i32),
    Percent,
    Parallel,
    Root,
    /// Between a magnitude and its angle: `10 ∠ 30°`, or `10 @ 30 deg`.
    Angle,
    Open,
    Close,
}

fn lex(text: &str, names: &[(String, Quantity)]) -> Option<Vec<Token>> {
    let chars: Vec<char> = text.chars().collect();
    let mut tokens = Vec::new();
    let mut i = 0;

    while i < chars.len() {
        let c = chars[i];
        let single = match c {
            '+' => Some(Token::Plus),
            '-' | '−' | '–' => Some(Token::Minus),
            '×' | '·' | '⋅' => Some(Token::Star),
            '/' | '÷' => Some(Token::Slash),
            '^' => Some(Token::Caret),
            '²' => Some(Token::Super(2)),
            '³' => Some(Token::Super(3)),
            '%' => Some(Token::Percent),
            '∥' => Some(Token::Parallel),
            '√' => Some(Token::Root),
            '∠' | '@' => Some(Token::Angle),
            '(' => Some(Token::Open),
            ')' => Some(Token::Close),
            _ => None,
        };
        if let Some(token) = single {
            tokens.push(token);
            i += 1;
        } else if c.is_whitespace() {
            i += 1;
        } else if c == '*' {
            let power = chars.get(i + 1) == Some(&'*');
            tokens.push(if power { Token::Caret } else { Token::Star });
            i += if power { 2 } else { 1 };
        } else if c == '|' && chars.get(i + 1) == Some(&'|') {
            tokens.push(Token::Parallel);
            i += 2;
        } else if c.is_ascii_digit() || c == '.' {
            i = lex_number(&chars, i, &mut tokens)?;
        } else if let Some(end) = name_end(&chars, i, names, matches!(tokens.last(), Some(Token::Number(_)))) {
            tokens.push(Token::Name(chars[i..end].iter().collect()));
            i = end;
        } else if c.is_alphabetic() || c == '°' {
            i = lex_word(&chars, i, &mut tokens);
        } else {
            return None;
        }
    }
    Some(tokens)
}

/// The end of the run of letters, digits and underscores starting at
/// `start`, if it starts with a letter.
fn identifier_end(chars: &[char], start: usize) -> Option<usize> {
    if !chars.get(start)?.is_alphabetic() {
        return None;
    }
    let mut end = start;
    while chars.get(end).is_some_and(|c| c.is_alphanumeric() || *c == '_') {
        end += 1;
    }
    Some(end)
}

/// If the name of a step begins at `start`, returns the index after it.
///
/// Anything with an underscore is a name, because no unit has one. A word
/// without an underscore is a name only if a step defined it, and never
/// straight after a number: `5V` is always five volts, even in a program
/// that has a step called `V`.
fn name_end(
    chars: &[char],
    start: usize,
    names: &[(String, Quantity)],
    after_number: bool,
) -> Option<usize> {
    let end = identifier_end(chars, start)?;
    let text: String = chars[start..end].iter().collect();
    let is_name = text.contains('_') || (!after_number && names.iter().any(|(known, _)| *known == text));
    is_name.then_some(end)
}

/// Whether `text` can be used as the name of a step in a program.
pub fn is_valid_name(text: &str) -> bool {
    let chars: Vec<char> = text.chars().collect();
    identifier_end(&chars, 0) == Some(chars.len())
        && !is_function(text)
        && !matches!(text, "pi" | "e" | "to" | "in" | "x" | "X")
}

/// Reads a word starting at `start` and returns the index after it. A dot
/// between letters stays in the word when it spells a unit (`N·m`).
fn lex_word(chars: &[char], start: usize, tokens: &mut Vec<Token>) -> usize {
    let is_letter = |i: usize| chars.get(i).is_some_and(|c| c.is_alphabetic() || *c == '°');
    let mut i = start;
    while is_letter(i) || (matches!(chars.get(i), Some('·' | '⋅')) && i > start && is_letter(i + 1)) {
        i += 1;
    }
    let word: String = chars[start..i].iter().map(|&c| if c == '⋅' { '·' } else { c }).collect();

    if word == "x" || word == "X" {
        tokens.push(Token::Star);
        return i;
    }
    if word.contains('·') && unit_index(&word).is_none() {
        // Not a unit after all: the dots were multiplication signs.
        for (n, part) in word.split('·').enumerate() {
            if n > 0 {
                tokens.push(Token::Star);
            }
            tokens.push(Token::Word(part.to_string()));
        }
    } else {
        tokens.push(Token::Word(word));
    }

    // A power written as a digit straight after a unit: mm2, m3.
    if let Some(Token::Word(last)) = tokens.last()
        && is_dimensioned_unit(last)
        && let Some(&digit @ ('2' | '3')) = chars.get(i)
        && !chars.get(i + 1).is_some_and(|c| c.is_alphanumeric() || *c == '.')
    {
        tokens.push(Token::Super(if digit == '2' { 2 } else { 3 }));
        i += 1;
    }
    i
}

/// Reads a number starting at `start` and returns the index after it.
/// Also understands component notation, where a prefix stands in for the
/// decimal point: `4k7` is 4.7 k, `2n2` is 2.2 n, `4R7` is 4.7 Ω.
fn lex_number(chars: &[char], start: usize, tokens: &mut Vec<Token>) -> Option<usize> {
    let digit_at = |i: usize| chars.get(i).is_some_and(|c| c.is_ascii_digit());
    let mut i = start;
    let mut text = String::new();
    let mut seen_point = false;

    while let Some(&c) = chars.get(i) {
        if c.is_ascii_digit() {
            text.push(c);
        } else if c == '.' && !seen_point {
            seen_point = true;
            text.push(c);
        } else {
            break;
        }
        i += 1;
    }

    // Component notation: digits, one letter, digits.
    if !seen_point
        && let Some(&letter) = chars.get(i)
        && (prefix(letter).is_some() || letter == 'R')
        && digit_at(i + 1)
    {
        i += 1;
        text.push('.');
        while digit_at(i) {
            text.push(chars[i]);
            i += 1;
        }
        if chars.get(i).is_some_and(|c| c.is_alphabetic()) {
            return None;
        }
        tokens.push(Token::Number(text.parse().ok()?));
        let word = if letter == 'R' { "Ω".to_string() } else { letter.to_string() };
        tokens.push(Token::Word(word));
        return Some(i);
    }

    // Scientific notation: e or E, an optional sign, then digits.
    if matches!(chars.get(i), Some('e' | 'E')) {
        let signed = matches!(chars.get(i + 1), Some('+' | '-'));
        let digits_from = if signed { i + 2 } else { i + 1 };
        if digit_at(digits_from) {
            text.push('e');
            if signed {
                text.push(chars[i + 1]);
            }
            i = digits_from;
            while digit_at(i) {
                text.push(chars[i]);
                i += 1;
            }
        }
    }

    tokens.push(Token::Number(text.parse().ok()?));
    Some(i)
}

// ---- parser -----------------------------------------------------------------

/// How to read a lone `m` after a number.
#[derive(Clone, Copy, PartialEq)]
enum BareM {
    Milli,
    Metre,
    /// In a program written by Claude a lone `m` always means metres.
    /// Claude is told to write milli with a unit (`mA`) or as `e-3`.
    Program,
}

struct Parser<'a> {
    tokens: &'a [Token],
    pos: usize,
    bare_m: BareM,
    /// Named values from earlier steps of a program. Empty for typed input.
    names: &'a [(String, Quantity)],
    /// False while the input is only a bare number, which is not worth
    /// answering.
    interesting: bool,
    /// The input contains a number or a constant, not only unit names.
    has_value: bool,
    /// What the rest of the input suggests, used to settle a lone `m`.
    saw_bare_m: bool,
    electrical: bool,
    mechanical: bool,
    joules: bool,
    /// A number was given a prefix and no unit, as in `4.7k`.
    bare_prefix: bool,
    /// Units the user typed, with the power each was raised to.
    typed: Vec<(usize, i32)>,
    /// How many signs, brackets, roots and powers deep the parser is now.
    depth: usize,
    /// Read a lone `K` as kelvin. Otherwise it is the prefix kilo.
    kelvin: bool,
    /// The Celsius and Fahrenheit words the user typed.
    celsius: bool,
    fahrenheit: bool,
    /// A lone `K` came straight before a `/` and a unit, as in `40 K/W`.
    k_rate: bool,
    /// The input has a π in it.
    saw_pi: bool,
    /// Why the parse stopped, where that is known for certain. See `Why`.
    why: Option<Why>,
    /// The first value typed with a prefix and no unit, as typed: `4.7k`.
    bare_text: Option<String>,
}

/// The deepest nesting the parser will follow: about a hundred brackets
/// inside one another. Each level uses some of the thread's stack, and
/// running out of stack ends the whole app, so anything deeper is refused.
const MAX_DEPTH: usize = 200;

impl<'a> Parser<'a> {
    /// Runs one nested part of the grammar, refusing if it is too deep.
    fn nested(&mut self, part: fn(&mut Self) -> Option<Quantity>) -> Option<Quantity> {
        if self.depth >= MAX_DEPTH {
            return None;
        }
        self.depth += 1;
        let result = part(self);
        self.depth -= 1;
        result
    }

    fn new(tokens: &'a [Token], bare_m: BareM, names: &'a [(String, Quantity)]) -> Self {
        Self {
            tokens,
            pos: 0,
            bare_m,
            names,
            interesting: false,
            has_value: false,
            saw_bare_m: false,
            electrical: false,
            mechanical: false,
            joules: false,
            bare_prefix: false,
            typed: Vec::new(),
            depth: 0,
            kelvin: false,
            celsius: false,
            fahrenheit: false,
            k_rate: false,
            saw_pi: false,
            why: None,
            bare_text: None,
        }
    }

    /// Refuses, noting why. Only the first reason is kept: nothing is tried
    /// again after a refusal, so it is the one that stopped the parse.
    fn refuse<T>(&mut self, why: Why) -> Option<T> {
        self.why.get_or_insert(why);
        None
    }

    /// A closing bracket was expected. If the input simply ran out, that is
    /// the reason; if something else is there, the reason is not known.
    fn unclosed<T>(&mut self) -> Option<T> {
        if self.peek().is_none() { self.refuse(Why::Unclosed) } else { None }
    }

    fn times(&mut self, a: Quantity, b: Quantity) -> Option<Quantity> {
        multiply(a, b).or_else(|| self.refuse(Why::TemperatureScaled))
    }

    fn over(&mut self, a: Quantity, b: Quantity) -> Option<Quantity> {
        divide(a, b).or_else(|| {
            let scaled = a.reading.is_some() || b.reading.is_some();
            self.refuse(if scaled { Why::TemperatureScaled } else { Why::DividedByZero })
        })
    }

    fn to_power(&mut self, base: Quantity, exponent: Quantity) -> Option<Quantity> {
        raise(base, exponent).or_else(|| {
            if base.reading.is_some() {
                self.refuse(Why::TemperatureScaled)
            } else if exponent.dim != NO_DIM {
                self.refuse(Why::PowerWithUnit)
            } else {
                None
            }
        })
    }

    /// The square root. A negative number has none among the real numbers,
    /// and working it out regardless would give no answer and no reason.
    fn root(&mut self, operand: Quantity) -> Option<Quantity> {
        if operand.im == 0.0 && operand.value < 0.0 && operand.reading.is_none() {
            return self.refuse(Why::OutOfRange("sqrt".into()));
        }
        self.to_power(operand, Quantity::plain(0.5))
    }

    /// Puts `argument` through the function `name`.
    fn call(&mut self, name: &str, argument: Quantity) -> Option<Quantity> {
        if name == "sqrt" {
            return self.root(argument);
        }
        match apply(name, argument) {
            Some(result) if !result.value.is_finite() && matches!(name, "asin" | "acos" | "ln" | "log") => {
                self.refuse(Why::OutOfRange(name.into()))
            }
            Some(result) => Some(result),
            None => match why_not_applied(name, argument) {
                Some(why) => self.refuse(why),
                None => None,
            },
        }
    }

    /// Whether a result in "per second" is known to count cycles, and so
    /// can be called hertz: the user typed Hz; or the formula has a π in
    /// it, as `1/(2π·R·C)` does; or a time was typed, as in `1/(20 ms)`.
    ///
    /// Without one of these it may as well be radians a second. `1/(R·C)`
    /// is: it is 2π times the cutoff frequency.
    fn counts_cycles(&self) -> bool {
        self.saw_pi
            || self.typed.iter().any(|&(index, _)| UNITS[index].names[0] == "Hz" || UNITS[index].dim == TIME)
    }

    /// Reads a word as a temperature scale, where one can stand.
    ///
    /// `F` is the farad, so Fahrenheit is `°F` or `degF`. A lone `K` is
    /// kelvin only in the reading that takes it so. Programs written by
    /// Claude have no temperature units at all.
    fn temperature_word(&mut self, word: &str) -> Option<Scale> {
        if self.bare_m == BareM::Program {
            return None;
        }
        let scale = match word {
            "C" | "°C" | "degC" | "celsius" | "Celsius" => Scale::Celsius,
            "°F" | "degF" | "fahrenheit" | "Fahrenheit" => Scale::Fahrenheit,
            "kelvin" | "Kelvin" => Scale::Kelvin,
            "K" if self.kelvin => Scale::Kelvin,
            _ => return None,
        };
        self.celsius |= scale == Scale::Celsius;
        self.fahrenheit |= scale == Scale::Fahrenheit;
        Some(scale)
    }

    /// The one scale the user typed, out of Celsius and Fahrenheit, if they
    /// typed only one. Kelvin does not count: it is the unit of difference.
    fn typed_scale(&self) -> Option<Scale> {
        match (self.celsius, self.fahrenheit) {
            (true, false) => Some(Scale::Celsius),
            (false, true) => Some(Scale::Fahrenheit),
            _ => None,
        }
    }

    /// Whether the next two tokens are a `/` and a unit with dimensions.
    fn at_rate(&self) -> bool {
        self.peek() == Some(&Token::Slash)
            && matches!(self.tokens.get(self.pos + 1), Some(Token::Word(word)) if is_dimensioned_unit(word))
    }

    /// A number in front of a temperature word, `value` of them.
    fn temperature(&mut self, value: f64, scale: Scale) -> Option<Quantity> {
        let amount = Quantity::of(value * scale.degree(), TEMPERATURE);
        // `50 C/W` is one quantity, a rise for each watt, however it is then
        // used: `125 C / 62 C/W` divides by all of it. So the unit after the
        // `/` is taken with the number here, and what is left is a
        // difference.
        if self.at_rate() {
            self.pos += 1;
            let Some(Token::Word(word)) = self.peek().cloned() else { return None };
            self.pos += 1;
            let (per, dim, index) = self.resolve(&word)?;
            if dim == NO_DIM {
                return None;
            }
            let (per, dim) = self.finish_unit(per, dim, index);
            return divide(amount, Quantity::of(per, dim));
        }
        // Kelvin has no zero of its own to allow for, so a number of
        // kelvin is an ordinary quantity.
        Some(match scale {
            Scale::Kelvin => amount,
            scale => Quantity { reading: Some(scale), ..amount },
        })
    }

    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    fn take(&mut self, token: Token) -> bool {
        if self.peek() == Some(&token) {
            self.pos += 1;
            self.interesting = true;
            true
        } else {
            false
        }
    }

    /// Lowest precedence: addition and subtraction.
    fn sum(&mut self) -> Option<Quantity> {
        let mut left = self.parallel()?;
        loop {
            let sign = if self.take(Token::Plus) {
                1.0
            } else if self.take(Token::Minus) {
                -1.0
            } else {
                return Some(left);
            };
            let right = self.parallel()?;
            left = match add(left, right, sign) {
                Some(total) => total,
                None => match why_not_added(left, right, sign) {
                    Some(why) => return self.refuse(why),
                    None => return None,
                },
            };
        }
    }

    /// `a || b`, the parallel combination. Binds tighter than `+`.
    fn parallel(&mut self) -> Option<Quantity> {
        let mut left = self.product()?;
        while self.take(Token::Parallel) {
            let right = self.product()?;
            if left.dim != right.dim {
                return self.refuse(Why::Unlike { left: left.dim, right: right.dim, how: Joined::Parallel });
            }
            if left.reading.is_some() || right.reading.is_some() {
                return self.refuse(Why::TemperatureScaled);
            }
            // The product over the sum, which holds for complex values too.
            let sum = Quantity::complex(left.value + right.value, left.im + right.im, left.dim);
            left = Quantity { dim: left.dim, ..divide(multiply(left, right)?, sum)? };
        }
        Some(left)
    }

    fn product(&mut self) -> Option<Quantity> {
        let mut left = self.unary()?;
        let mut after_division = false;
        loop {
            if self.take(Token::Star) {
                let right = self.unary()?;
                left = self.times(left, right)?;
                after_division = false;
            } else if self.take(Token::Slash) {
                let right = self.unary()?;
                left = self.over(left, right)?;
                after_division = true;
            } else if matches!(
                self.peek(),
                Some(Token::Open | Token::Word(_) | Token::Name(_) | Token::Root)
            ) {
                // Implied multiplication, as in `2pi` or `2(3)`. After a
                // division it is ambiguous (`1/2pi`), so it is refused.
                if after_division {
                    return self.refuse(Why::AfterDivision);
                }
                self.interesting = true;
                let right = self.power()?;
                left = self.times(left, right)?;
            } else {
                return Some(left);
            }
        }
    }

    fn unary(&mut self) -> Option<Quantity> {
        self.nested(Self::signed)
    }

    fn signed(&mut self) -> Option<Quantity> {
        if self.take(Token::Minus) {
            let mut operand = self.unary()?;
            operand.value = -operand.value;
            operand.im = -operand.im;
            Some(operand)
        } else if self.take(Token::Plus) {
            self.unary()
        } else {
            self.power()
        }
    }

    /// Right-associative, and tighter than a leading minus: `-2^2` is -4.
    fn power(&mut self) -> Option<Quantity> {
        let mut base = self.primary()?;
        if self.typed_input() && self.take(Token::Angle) {
            base = self.polar(base)?;
        }
        while self.take(Token::Percent) {
            if base.dim != NO_DIM || base.im != 0.0 {
                return None;
            }
            base = Quantity { percent: true, ..Quantity::plain(base.value / 100.0) };
        }
        if let Some(&Token::Super(n)) = self.peek() {
            self.pos += 1;
            self.interesting = true;
            base = self.to_power(base, Quantity::plain(f64::from(n)))?;
        }
        if self.take(Token::Caret) {
            let exponent = self.unary()?;
            self.to_power(base, exponent)
        } else {
            Some(base)
        }
    }

    /// Complex numbers, like temperatures and decibels, are for typed
    /// input. The language Claude writes its formulas in has none.
    fn typed_input(&self) -> bool {
        self.bare_m != BareM::Program
    }

    /// What follows the angle sign: `10 ∠ 30°` is ten at thirty degrees.
    /// The angle must say what it is in (`deg`, `°`, `rad`), since a bare
    /// 30 could be either, and the magnitude must be an ordinary value.
    fn polar(&mut self, size: Quantity) -> Option<Quantity> {
        let negative = self.take(Token::Minus);
        if !negative {
            self.take(Token::Plus);
        }
        let angle = self.primary()?;
        let ordinary = angle.im == 0.0 && size.im == 0.0 && size.reading.is_none() && !size.percent;
        if ordinary && angle.dim == NO_DIM && !angle.percent {
            return self.refuse(Why::AngleNeedsUnit);
        }
        if angle.dim != ANGLE_DIM || !ordinary {
            return None;
        }
        let turn = if negative { -angle.value } else { angle.value };
        Some(Quantity::complex(size.value * turn.cos(), size.value * turn.sin(), size.dim).settled())
    }

    /// Reads a word as a unit or prefix, noting what it says about the
    /// input. Returns the scale to SI base units, the dimensions, and the
    /// unit's index if it is one.
    fn resolve(&mut self, word: &str) -> Option<(f64, Dim, Option<usize>)> {
        Some(match lookup(word)? {
            Suffix::BareM => {
                self.saw_bare_m = true;
                match self.bare_m {
                    BareM::Milli => {
                        self.bare_prefix = true;
                        (1e-3, NO_DIM, None)
                    }
                    BareM::Metre | BareM::Program => (1.0, LENGTH, Some(METRE)),
                }
            }
            Suffix::BarePrefix(scale) => {
                self.electrical = true;
                self.bare_prefix = true;
                (scale, NO_DIM, None)
            }
            Suffix::Unit { scale, index } => {
                let unit = &UNITS[index];
                match unit.family {
                    Electrical => self.electrical = true,
                    Mechanical => self.mechanical = true,
                    Neutral => {}
                }
                self.joules |= unit.joules;
                (scale * unit.factor, unit.dim, Some(index))
            }
        })
    }

    /// A power attached to a unit, as in `mm²`, `m^3` or `s^-2`. It applies
    /// to the unit alone: `5 m^2` is five square metres, not twenty-five.
    fn unit_power(&mut self) -> i32 {
        if let Some(&Token::Super(n)) = self.peek() {
            self.pos += 1;
            return n;
        }
        if self.peek() != Some(&Token::Caret) {
            return 1;
        }
        let negative = self.tokens.get(self.pos + 1) == Some(&Token::Minus);
        let at = self.pos + if negative { 2 } else { 1 };
        if let Some(&Token::Number(n)) = self.tokens.get(at)
            && n.fract() == 0.0
            && (1.0..=6.0).contains(&n)
        {
            self.pos = at + 1;
            return if negative { -(n as i32) } else { n as i32 };
        }
        1
    }

    /// Finishes a unit that has just been read: applies any power written
    /// on it and records it as typed.
    fn finish_unit(&mut self, scale: f64, dim: Dim, index: Option<usize>) -> (f64, Dim) {
        if dim == NO_DIM {
            return (scale, dim);
        }
        let power = self.unit_power();
        if let Some(index) = index {
            self.typed.push((index, power));
        }
        (scale.powi(power), dim.map(|d| d * power as i8))
    }

    fn primary(&mut self) -> Option<Quantity> {
        self.nested(Self::operand)
    }

    fn operand(&mut self) -> Option<Quantity> {
        // Something was expected here and the input has run out.
        let Some(token) = self.peek().cloned() else { return self.refuse(Why::Unfinished) };
        self.pos += 1;
        match token {
            Token::Number(value) => {
                self.has_value = true;
                if let Some(Token::Word(word)) = self.peek().cloned()
                    && let Some(scale) = self.temperature_word(&word)
                {
                    self.pos += 1;
                    self.interesting = true;
                    return self.temperature(value, scale);
                }
                if let Some(Token::Word(word)) = self.peek().cloned()
                    && let Some((scale, dim, index)) = self.resolve(&word)
                {
                    self.pos += 1;
                    self.k_rate |= word == "K" && self.at_rate();
                    self.interesting = true;
                    if dim == NO_DIM && self.bare_text.is_none() {
                        self.bare_text = Some(format!("{value}{word}"));
                    }
                    let (scale, dim) = self.finish_unit(scale, dim, index);
                    return Some(Quantity::of(value * scale, dim));
                }
                Some(Quantity::plain(value))
            }
            Token::Name(name) => {
                self.has_value = true;
                self.interesting = true;
                self.names.iter().rev().find(|(known, _)| *known == name).map(|(_, value)| *value)
            }
            Token::Open => {
                let inner = self.sum()?;
                if self.take(Token::Close) { Some(inner) } else { self.unclosed() }
            }
            Token::Root => {
                self.interesting = true;
                let operand = self.primary()?;
                self.root(operand)
            }
            Token::Word(word) => {
                self.interesting = true;
                match word.as_str() {
                    "pi" | "π" => {
                        self.has_value = true;
                        self.saw_pi = true;
                        return Some(Quantity::plain(PI));
                    }
                    "e" => {
                        self.has_value = true;
                        return Some(Quantity::plain(std::f64::consts::E));
                    }
                    // The imaginary unit, as electrical work writes it. In
                    // front of a number it takes that number with it, so
                    // `3 + j4` is the same as `3 + 4j`.
                    "j" if self.typed_input() => {
                        self.has_value = true;
                        let unit = Quantity::complex(0.0, 1.0, NO_DIM);
                        return match self.peek() {
                            Some(Token::Number(_)) => multiply(unit, self.power()?),
                            _ => Some(unit),
                        };
                    }
                    _ => {}
                }
                if is_function(&word) || (self.typed_input() && is_complex_function(&word)) {
                    if !self.take(Token::Open) {
                        return None;
                    }
                    let argument = self.sum()?;
                    if !self.take(Token::Close) {
                        return self.unclosed();
                    }
                    // An angle found this way is shown in degrees.
                    if matches!(word.as_str(), "arg" | "phase") {
                        self.typed.push((unit_index("deg")?, 1));
                    }
                    return self.call(&word, argument);
                }
                // A temperature word on its own is one degree of difference,
                // as in `8 mW/C`.
                if let Some(scale) = self.temperature_word(&word) {
                    let (degree, dim) = self.finish_unit(scale.degree(), TEMPERATURE, None);
                    return Some(Quantity::of(degree, dim));
                }
                // A unit on its own, as in `m/s` or `N*m`.
                let (scale, dim, index) = self.resolve(&word)?;
                if dim == NO_DIM {
                    return None;
                }
                let (scale, dim) = self.finish_unit(scale, dim, index);
                Some(Quantity::of(scale, dim))
            }
            _ => None,
        }
    }
}

/// Adds, or with `sign` of -1 subtracts.
///
/// A reading on a temperature scale (`25 C`) is a point, not an amount, so:
/// a reading with a difference added or taken away is a reading; one
/// reading less another is a difference; and two readings cannot be added.
fn add(left: Quantity, right: Quantity, sign: f64) -> Option<Quantity> {
    if left.dim != right.dim || left.percent != right.percent {
        return None;
    }
    let value = left.value + sign * right.value;
    let im = left.im + sign * right.im;
    // A temperature is never complex.
    if im != 0.0 && (left.reading.is_some() || right.reading.is_some()) {
        return None;
    }
    match (left.reading, right.reading) {
        (None, None) | (Some(_), None) => Some(Quantity { value, im, ..left }),
        // A difference with a reading added to it. Taking a reading away
        // from a difference means nothing.
        (None, Some(_)) => (sign > 0.0).then_some(Quantity { value, ..right }),
        (Some(a), Some(b)) => (sign < 0.0).then(|| {
            let apart = if a == b {
                value
            } else {
                // Through kelvin, as the two are on different scales, with
                // the noise from adding each scale's zero rounded off.
                let apart = left.kelvin().unwrap_or(0.0) - right.kelvin().unwrap_or(0.0);
                (apart * 1e9).round() / 1e9
            };
            Quantity::of(apart, TEMPERATURE)
        }),
    }
}

/// A reading on a temperature scale cannot be multiplied, divided, raised
/// to a power or put through a function: twice 25 °C is not 50 °C.
fn multiply(a: Quantity, b: Quantity) -> Option<Quantity> {
    if a.reading.is_some() || b.reading.is_some() {
        return None;
    }
    let mut dim = a.dim;
    for (d, other) in dim.iter_mut().zip(b.dim) {
        *d += other;
    }
    Some(Quantity::complex(a.value * b.value - a.im * b.im, a.value * b.im + a.im * b.value, dim))
}

fn divide(a: Quantity, b: Quantity) -> Option<Quantity> {
    if (b.value == 0.0 && b.im == 0.0) || a.reading.is_some() || b.reading.is_some() {
        return None;
    }
    let mut dim = a.dim;
    for (d, other) in dim.iter_mut().zip(b.dim) {
        *d -= other;
    }
    if a.im == 0.0 && b.im == 0.0 {
        return Some(Quantity::of(a.value / b.value, dim));
    }
    // Scaled by the larger part of the divisor, so that squaring it cannot
    // overflow or vanish.
    let (re, im) = if b.value.abs() >= b.im.abs() {
        let ratio = b.im / b.value;
        let under = b.value + b.im * ratio;
        ((a.value + a.im * ratio) / under, (a.im - a.value * ratio) / under)
    } else {
        let ratio = b.value / b.im;
        let under = b.value * ratio + b.im;
        ((a.value * ratio + a.im) / under, (a.im * ratio - a.value) / under)
    };
    Some(Quantity::complex(re, im, dim))
}

/// e to the power of a complex number: e^a (cos b + j sin b).
fn exponential(re: f64, im: f64) -> Quantity {
    let size = re.exp();
    Quantity::complex(size * im.cos(), size * im.sin(), NO_DIM).settled()
}

/// The exponent must be a plain number. A quantity with units can only be
/// raised to a power that leaves whole-number dimensions (`V^2`, `√(H·F)`).
fn raise(base: Quantity, exponent: Quantity) -> Option<Quantity> {
    if exponent.dim != NO_DIM || base.reading.is_some() {
        return None;
    }
    if exponent.im != 0.0 {
        // A complex power of a plain positive number, as in `e^(j*pi/3)`.
        if base.dim != NO_DIM || base.im != 0.0 || base.value <= 0.0 {
            return None;
        }
        let log = base.value.ln();
        return Some(exponential(exponent.value * log, exponent.im * log));
    }
    let mut dim = NO_DIM;
    for (out, d) in dim.iter_mut().zip(base.dim) {
        let scaled = f64::from(d) * exponent.value;
        if (scaled - scaled.round()).abs() > 1e-9 || scaled.abs() > 100.0 {
            return None;
        }
        *out = scaled.round() as i8;
    }
    if base.im != 0.0 {
        // Through magnitude and angle, which gives the principal root.
        let size = base.magnitude().powf(exponent.value);
        let turn = base.im.atan2(base.value) * exponent.value;
        return Some(Quantity::complex(size * turn.cos(), size * turn.sin(), dim).settled());
    }
    Some(Quantity::of(base.value.powf(exponent.value), dim))
}

fn is_function(name: &str) -> bool {
    matches!(
        name,
        "sqrt" | "abs" | "ln" | "log" | "exp" | "sin" | "cos" | "tan" | "asin" | "acos" | "atan"
    )
}

/// Functions for complex numbers. Kept apart from `is_function`, which
/// also says what a step of a program may not be called: a program may
/// still have a step named `phase` or `re`.
fn is_complex_function(name: &str) -> bool {
    matches!(name, "arg" | "phase" | "re" | "im" | "conj")
}

fn apply(name: &str, argument: Quantity) -> Option<Quantity> {
    if argument.reading.is_some() {
        return None;
    }
    let Quantity { value: re, im, dim, .. } = argument;
    match name {
        "sqrt" => return raise(argument, Quantity::plain(0.5)),
        "abs" => return Some(Quantity::of(argument.magnitude(), dim)),
        "re" => return Some(Quantity::of(re, dim)),
        "im" => return Some(Quantity::of(im, dim)),
        "conj" => return Some(Quantity::complex(re, -im, dim)),
        // Nothing has an angle if it has no size.
        "arg" | "phase" => {
            return (argument.magnitude() > 0.0).then(|| Quantity::of(im.atan2(re), ANGLE_DIM));
        }
        "exp" if im != 0.0 => return (dim == NO_DIM).then(|| exponential(re, im)),
        // The other functions are for real numbers.
        _ if im != 0.0 => return None,
        _ => {}
    }
    // Trigonometry takes a plain number (radians) or an angle (`30 deg`).
    let is_trig = matches!(name, "sin" | "cos" | "tan");
    if argument.dim != NO_DIM && !(is_trig && argument.dim == ANGLE_DIM) {
        return None;
    }
    let x = argument.value;
    let value = match name {
        "ln" => x.ln(),
        "log" => x.log10(),
        "exp" => x.exp(),
        "sin" => x.sin(),
        "cos" => x.cos(),
        "tan" => x.tan(),
        "asin" => x.asin(),
        "acos" => x.acos(),
        "atan" => x.atan(),
        _ => return None,
    };
    Some(Quantity::plain(value))
}

// ---- conversion targets -----------------------------------------------------

/// The unit named after `to`, as in `3.5 bar to psi` or `12 L/min to gpm`.
struct Target {
    factor: f64,
    dim: Dim,
    label: String,
    mechanical: bool,
    electrical: bool,
    /// The target is hertz and nothing else (`Hz`, `kHz`): cycles a
    /// second, which is not the same as `s^-1`.
    hertz: bool,
}

fn superscript(power: i32) -> String {
    match power {
        1 => String::new(),
        2 => "²".into(),
        3 => "³".into(),
        -1 => "⁻¹".into(),
        -2 => "⁻²".into(),
        -3 => "⁻³".into(),
        n => format!("^{n}"),
    }
}

fn parse_target(tokens: &[Token]) -> Option<Target> {
    let mut parser = Parser::new(tokens, BareM::Metre, &[]);
    let mut target = Target {
        factor: 1.0,
        dim: NO_DIM,
        label: String::new(),
        mechanical: false,
        electrical: false,
        hertz: false,
    };
    let mut divide = false;
    let mut units = 0;

    loop {
        let Some(Token::Word(word)) = parser.peek().cloned() else { return None };
        parser.pos += 1;
        let (name, scale, index) = match lookup(&word)? {
            Suffix::BareM => ("m".to_string(), 1.0, METRE),
            Suffix::BarePrefix(_) => return None,
            Suffix::Unit { scale, index } => {
                (format!("{}{}", prefix_symbol(scale), UNITS[index].names[0]), scale, index)
            }
        };
        let unit = &UNITS[index];
        target.mechanical |= unit.family == Mechanical;
        target.electrical |= unit.family == Electrical;

        let power = parser.unit_power();
        let signed = if divide { -power } else { power };
        units += 1;
        target.hertz = units == 1 && signed == 1 && unit.names[0] == "Hz";
        target.factor *= (scale * unit.factor).powi(signed);
        for (d, u) in target.dim.iter_mut().zip(unit.dim) {
            *d += u * signed as i8;
        }
        if !target.label.is_empty() {
            target.label.push(if divide { '/' } else { '·' });
        }
        target.label.push_str(&name);
        target.label.push_str(&superscript(power));

        match parser.peek() {
            None => return Some(target),
            Some(Token::Slash) => divide = true,
            Some(Token::Star) => divide = false,
            _ => return None,
        }
        parser.pos += 1;
    }
}

/// Where the input splits into "value" and "unit to convert to", if it does.
/// `in` counts as the keyword only where it cannot be inches.
fn conversion_split(tokens: &[Token]) -> Option<usize> {
    let is_word = |i: usize, word: &str| matches!(&tokens[i], Token::Word(w) if w == word);
    let last = tokens.len().checked_sub(1)?;
    (1..last)
        .rev()
        .find(|&i| is_word(i, "to"))
        .or_else(|| {
            (1..last).rev().find(|&i| is_word(i, "in") && !matches!(tokens[i - 1], Token::Number(_)))
        })
}

// ---- temperature ------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq)]
enum Scale {
    Celsius,
    Fahrenheit,
    Kelvin,
}

impl Scale {
    /// Kelvin in one degree of this scale.
    fn degree(self) -> f64 {
        match self {
            Scale::Fahrenheit => 5.0 / 9.0,
            Scale::Celsius | Scale::Kelvin => 1.0,
        }
    }

    /// The scale's zero, in kelvin.
    fn zero(self) -> f64 {
        match self {
            Scale::Celsius => 273.15,
            Scale::Fahrenheit => 459.67 * 5.0 / 9.0,
            Scale::Kelvin => 0.0,
        }
    }

    fn symbol(self) -> &'static str {
        match self {
            Scale::Celsius => "°C",
            Scale::Fahrenheit => "°F",
            Scale::Kelvin => "K",
        }
    }

    /// How a difference is labelled. Kelvin is the unit of difference
    /// already. The other two are marked, so that a difference of 60 °C is
    /// not taken for a temperature of 60 °C.
    fn difference(self) -> &'static str {
        match self {
            Scale::Celsius => "Δ°C",
            Scale::Fahrenheit => "Δ°F",
            Scale::Kelvin => "K",
        }
    }
}

fn temperature_scale(word: &str) -> Option<Scale> {
    Some(match word {
        "C" | "°C" | "degC" | "celsius" | "Celsius" => Scale::Celsius,
        "F" | "°F" | "degF" | "fahrenheit" | "Fahrenheit" => Scale::Fahrenheit,
        "K" | "kelvin" | "Kelvin" => Scale::Kelvin,
        _ => return None,
    })
}

/// Temperatures have offsets, so they are only handled as a direct
/// conversion: `100 C to F`, `-40 °F to °C`, `300 K in C`.
fn temperature(tokens: &[Token]) -> Option<(Answer, f64)> {
    let (negative, rest) = match tokens {
        [Token::Minus, rest @ ..] => (true, rest),
        rest => (false, rest),
    };
    let [Token::Number(value), Token::Word(from), Token::Word(keyword), Token::Word(to)] = rest else {
        return None;
    };
    if keyword != "to" && keyword != "in" {
        return None;
    }
    let (from, to) = (temperature_scale(from)?, temperature_scale(to)?);
    let value = if negative { -value } else { *value };

    let kelvin = match from {
        Scale::Celsius => value + 273.15,
        Scale::Fahrenheit => (value - 32.0) * 5.0 / 9.0 + 273.15,
        Scale::Kelvin => value,
    };
    if kelvin < 0.0 {
        return None;
    }
    let (converted, label) = match to {
        Scale::Celsius => (kelvin - 273.15, "°C"),
        Scale::Fahrenheit => ((kelvin - 273.15) * 9.0 / 5.0 + 32.0, "°F"),
        Scale::Kelvin => (kelvin, "K"),
    };
    // Round off the noise from adding and removing 273.15.
    let converted = (converted * 1e9).round() / 1e9;
    Some((shown_as_is(converted, label), kelvin))
}

// ---- decibels ---------------------------------------------------------------
//
// Decibels are logarithmic, so they are read as whole patterns and never
// mixed into ordinary sums:
// - a level, with gains added or taken away: `10 dBm + 3 dB`, `+4 dBu in V`;
// - a value as a level: `250 mW in dBm`;
// - one quantity over another of the same kind: `2 V / 1 V to dB`.
//
// A plain number has no decibel value of its own, because it could be a
// ratio of powers (10·log) or of voltages (20·log). `47 in dB` is refused.

/// A level: decibels above a fixed reference, such as dBm (1 mW).
struct Level {
    /// The spelling shown in answers.
    name: &'static str,
    reference: f64,
    dim: Dim,
    /// The unit the reference is in.
    unit: &'static str,
}

static LEVELS: &[Level] = &[
    Level { name: "dBm", reference: 1e-3, dim: POWER, unit: "W" },
    Level { name: "dBW", reference: 1.0, dim: POWER, unit: "W" },
    Level { name: "dBV", reference: 1.0, dim: VOLTAGE, unit: "V" },
    // 0 dBu is the voltage that puts 1 mW into 600 Ω: √0.6 V.
    Level { name: "dBu", reference: 0.774_596_669_241_483_4, dim: VOLTAGE, unit: "V" },
];

/// Accepts the usual spelling and all lower case (`dbm`). `dBv` with a
/// small v alone is an old name for dBu, so it is not accepted as either.
fn level(word: &str) -> Option<&'static Level> {
    LEVELS.iter().find(|level| level.name == word || level.name.to_lowercase() == word)
}

fn is_gain(word: &str) -> bool {
    word == "dB" || word == "db"
}

/// Decibels for each factor of ten: 10 for power, 20 for voltage and
/// current. Nothing else has an agreed meaning in decibels here.
fn decibels_per_decade(dim: Dim) -> Option<f64> {
    if dim == POWER {
        Some(10.0)
    } else if dim == VOLTAGE || dim == CURRENT {
        Some(20.0)
    } else {
        None
    }
}

/// Rounds off the noise a logarithm leaves, so that 30 dBm is 0 dBW.
fn tidy(decibels: f64) -> f64 {
    (decibels * 1e9).round() / 1e9
}

/// The second value is the number of decibels when the answer is in
/// decibels, and SI base units otherwise.
fn decibels(tokens: &[Token], labelled: bool) -> Option<(Answer, f64)> {
    level_sum(tokens).or_else(|| as_decibels(tokens, labelled))
}

/// `[±] number level { ± number dB } [to unit]`
fn level_sum(tokens: &[Token]) -> Option<(Answer, f64)> {
    let word = |pos: usize| match tokens.get(pos) {
        Some(Token::Word(word)) => Some(word.as_str()),
        _ => None,
    };
    let number = |pos: &mut usize, sign_needed: bool| -> Option<f64> {
        let sign = match tokens.get(*pos) {
            Some(Token::Plus) => 1.0,
            Some(Token::Minus) => -1.0,
            _ if sign_needed => return None,
            _ => 0.0,
        };
        if sign != 0.0 {
            *pos += 1;
        }
        let Some(&Token::Number(value)) = tokens.get(*pos) else { return None };
        *pos += 1;
        Some(if sign < 0.0 { -value } else { value })
    };

    let mut pos = 0;
    let mut decibels = number(&mut pos, false)?;
    let from = level(word(pos)?)?;
    pos += 1;
    let mut gains = false;
    while matches!(tokens.get(pos), Some(Token::Plus | Token::Minus)) {
        let gain = number(&mut pos, true)?;
        if !is_gain(word(pos)?) {
            return None;
        }
        pos += 1;
        decibels += gain;
        gains = true;
    }
    let decibels = tidy(decibels);
    let per_decade = decibels_per_decade(from.dim)?;
    let linear = from.reference * 10f64.powf(decibels / per_decade);
    if !linear.is_normal() {
        return None;
    }

    if pos == tokens.len() {
        // A level alone is answered with what it stands for: 10 dBm is
        // 10 mW. Once gains are applied the answer stays a level.
        return Some(if gains {
            (shown_as_is(decibels, from.name), decibels)
        } else {
            (shown_with_prefix(linear, from.unit), linear)
        });
    }
    if !matches!(word(pos), Some("to" | "in")) {
        return None;
    }
    let target = &tokens[pos + 1..];
    if let [Token::Word(unit)] = target
        && let Some(to) = level(unit)
    {
        // Power and voltage levels are only related through an impedance,
        // which was not given.
        if to.dim != from.dim {
            return None;
        }
        let converted = tidy(per_decade * (linear / to.reference).log10());
        return Some((shown_as_is(converted, to.name), converted));
    }
    let target = parse_target(target)?;
    (target.dim == from.dim).then(|| (shown_as_is(linear / target.factor, &target.label), linear))
}

/// `value to level` and `quantity / quantity to dB`.
fn as_decibels(tokens: &[Token], labelled: bool) -> Option<(Answer, f64)> {
    let [expression @ .., Token::Word(keyword), Token::Word(unit)] = tokens else { return None };
    if keyword != "to" && keyword != "in" {
        return None;
    }
    let positive = |tokens: &[Token]| {
        reading(tokens, None, labelled).map(|outcome| outcome.quantity).filter(|q| q.value > 0.0 && q.im == 0.0)
    };

    if let Some(to) = level(unit) {
        let value = positive(expression)?;
        if value.dim != to.dim {
            return None;
        }
        let decibels = tidy(decibels_per_decade(to.dim)? * (value.value / to.reference).log10());
        return Some((shown_as_is(decibels, to.name), decibels));
    }
    if !is_gain(unit) {
        return None;
    }
    let slash = ratio_split(expression)?;
    let above = positive(&expression[..slash])?;
    let below = positive(&expression[slash + 1..])?;
    if above.dim != below.dim {
        return None;
    }
    let decibels = tidy(decibels_per_decade(above.dim)? * (above.value / below.value).log10());
    Some((shown_as_is(decibels, "dB"), decibels))
}

/// Where `tokens` divide into one quantity over another, if that is all
/// they are: a single division outside any brackets, with nothing after it
/// that the usual order of working would attach to the result instead.
fn ratio_split(tokens: &[Token]) -> Option<usize> {
    let mut depth = 0usize;
    let mut slash = None;
    for (i, token) in tokens.iter().enumerate() {
        match token {
            Token::Open => depth += 1,
            Token::Close => depth = depth.checked_sub(1)?,
            _ if depth > 0 => {}
            Token::Slash if slash.is_none() => slash = Some(i),
            Token::Slash | Token::Plus | Token::Minus | Token::Parallel => return None,
            Token::Star if slash.is_some() => return None,
            _ => {}
        }
    }
    let slash = slash?;
    // What is under the line must be one thing: a value with its unit, or
    // one pair of brackets.
    let simple = match &tokens[slash + 1..] {
        [Token::Number(_), Token::Word(_)] | [Token::Number(_), Token::Word(_), Token::Super(_)] => true,
        [Token::Open, inside @ .., Token::Close] => {
            let mut depth = 0i32;
            inside.iter().all(|token| {
                match token {
                    Token::Open => depth += 1,
                    Token::Close => depth -= 1,
                    _ => {}
                }
                depth >= 0
            })
        }
        _ => false,
    };
    simple.then_some(slash)
}

// ---- formatting -------------------------------------------------------------

/// Rounds to the significant figures in force.
fn round_sig(value: f64) -> f64 {
    format!("{:.*e}", (sig_figs() - 1) as usize, value)
        .parse()
        .unwrap_or(value)
}

/// A fixed number of significant figures, keeping trailing zeros.
/// The value must already be rounded and positive.
fn fixed_sig(magnitude: f64) -> String {
    let whole_digits = magnitude.log10().floor() as i32 + 1;
    let decimals = if magnitude < 1.0 {
        // Below 1, the zeros after the point do not count as significant.
        (sig_figs() - whole_digits) as usize
    } else {
        (sig_figs() - whole_digits).max(0) as usize
    };
    format!("{magnitude:.decimals$}")
}

/// A plain number for the clipboard.
fn plain_copy(value: f64) -> String {
    let magnitude = value.abs();
    if value == 0.0 {
        "0".into()
    } else if (1e-4..1e9).contains(&magnitude) {
        format!("{value}")
    } else {
        format!("{value:e}")
    }
}

fn sign_of(value: f64) -> &'static str {
    if value < 0.0 { "−" } else { "" }
}

/// A number shown in exactly the unit given, with no prefix added. Used for
/// conversions and for units that do not take prefixes (`psi`, `in`, `rpm`).
/// Copying gives the number as shown.
fn shown_as_is(value: f64, unit: &str) -> Answer {
    let value = round_sig(value);
    let magnitude = value.abs();
    let number = if value == 0.0 {
        "0".to_string()
    } else if (1e-3..1e6).contains(&magnitude) {
        format!("{}{}", sign_of(value), fixed_sig(magnitude))
    } else {
        format!("{}{:.*e}", sign_of(value), (sig_figs() - 1) as usize, magnitude)
    };
    Answer::real(number, unit.into(), plain_copy(value))
}

/// Engineering notation with an automatic SI prefix. Copying gives the full
/// value in the unprefixed unit.
fn shown_with_prefix(value: f64, unit: &str) -> Answer {
    let value = round_sig(value);
    if value == 0.0 {
        return Answer::real("0".into(), unit.into(), "0".into());
    }
    let magnitude = value.abs();
    let sign = sign_of(value);
    let copy = plain_copy(value);

    // A plain number of ordinary size reads best with no prefix at all.
    if unit.is_empty() && (0.1..1000.0).contains(&magnitude) {
        return Answer::real(format!("{sign}{}", fixed_sig(magnitude)), String::new(), copy);
    }

    // The small tolerance keeps exact powers of ten (1000, 1e-6) from
    // landing just below their own boundary.
    let exponent = ((magnitude.log10() + 1e-12) / 3.0).floor() as i32 * 3;
    // A plain number in the thousandths would get a lone "m", which reads as
    // metres. Write it out instead: 0.05000, not 50.00 m.
    if unit.is_empty() && exponent == -3 {
        return Answer::real(format!("{sign}{}", fixed_sig(magnitude)), String::new(), copy);
    }
    if !(-15..=12).contains(&exponent) {
        // Beyond the prefixes: fall back to scientific notation.
        return Answer::real(
            format!("{sign}{:.*e}", (sig_figs() - 1) as usize, magnitude),
            unit.into(),
            copy,
        );
    }
    let scale = 10f64.powi(exponent);
    Answer::real(
        format!("{sign}{}", fixed_sig(round_sig(magnitude / scale))),
        format!("{}{unit}", prefix_symbol(scale)),
        copy,
    )
}

/// Picks the largest unit in which the value is at least 1.
fn shown_in_best(value: f64, choices: &[(f64, &str)]) -> Answer {
    let (scale, unit) = choices
        .iter()
        .rev()
        .find(|(scale, _)| value.abs() / scale >= 1.0)
        .unwrap_or(&choices[0]);
    shown_as_is(value / scale, unit)
}

/// The unit a result is shown in when the user did not ask for one.
fn shown_by_default(value: f64, dim: Dim, joules: bool) -> Option<Answer> {
    const PREFIXED: &[(Dim, &str)] = &[
        (NO_DIM, ""),
        (VOLTAGE, "V"),
        (CURRENT, "A"),
        (RESISTANCE, "Ω"),
        (CAPACITANCE, "F"),
        (INDUCTANCE, "H"),
        (CONDUCTANCE, "S"),
        (FREQUENCY, "Hz"),
        (POWER, "W"),
        (CHARGE, "C"),
        (MAGNETIC_FLUX, "Wb"),
        (FLUX_DENSITY, "T"),
        (LENGTH, "m"),
        (FORCE, "N"),
        (PRESSURE, "Pa"),
    ];
    const BEST: &[(Dim, &[(f64, &str)])] = &[
        (TIME, &[(1e-9, "ns"), (1e-6, "µs"), (1e-3, "ms"), (1.0, "s")]),
        (MASS, &[(1e-6, "mg"), (1e-3, "g"), (1.0, "kg")]),
        (AREA, &[(1e-6, "mm²"), (1e-4, "cm²"), (1.0, "m²")]),
        (VOLUME, &[(1e-9, "mm³"), (1e-6, "mL"), (1e-3, "L"), (1.0, "m³")]),
        (SPEED, &[(1e-3, "mm/s"), (1.0, "m/s")]),
        (ACCELERATION, &[(1.0, "m/s²")]),
        (FLOW, &[(1e-3 / 60.0, "L/min")]),
        (ANGLE_DIM, &[(1.0, "rad")]),
        (ANGULAR_SPEED, &[(1.0, "rad/s")]),
    ];

    let named = |dim: Dim| -> Option<Answer> {
        if dim == ENERGY {
            return Some(shown_with_prefix(value, if joules { "J" } else { "N·m" }));
        }
        if let Some((_, unit)) = PREFIXED.iter().find(|(d, _)| *d == dim) {
            return Some(shown_with_prefix(value, unit));
        }
        BEST.iter().find(|(d, _)| *d == dim).map(|(_, choices)| shown_in_best(value, choices))
    };

    named(dim).or_else(|| {
        // The radian is dimensionless in SI, so torque × angular speed is
        // simply power. Angle only matters when comparing like with like.
        let mut without_angle = dim;
        without_angle[ANGLE] = 0;
        if without_angle == dim {
            return None;
        }
        // Energy for each radian is a torque: power over angular speed.
        if without_angle == ENERGY && dim[ANGLE] < 0 {
            return Some(shown_with_prefix(value, "N·m"));
        }
        named(without_angle)
    })
}

// ---- entry point ------------------------------------------------------------

struct Outcome {
    answer: Answer,
    quantity: Quantity,
    saw_bare_m: bool,
    electrical: bool,
    mechanical: bool,
    bare_prefix: bool,
    /// A Celsius or Fahrenheit word was typed.
    scale_typed: bool,
    /// A lone `K` came straight before a `/` and a unit.
    k_rate: bool,
    /// The first value typed with a prefix and no unit, as typed.
    bare_text: Option<String>,
    /// The result as the engine holds it. `quantity` gives a temperature
    /// in kelvin; this keeps it as a reading on its scale.
    raw: Quantity,
    hints: Hints,
}

/// What the units typed say about an input. It is carried from the values
/// given after a formula to the formula itself, where it decides how the
/// answer is shown: millijoules or newton metres, hertz or per second,
/// °C or °F.
#[derive(Clone, Default)]
struct Hints {
    electrical: bool,
    mechanical: bool,
    joules: bool,
    saw_pi: bool,
    celsius: bool,
    fahrenheit: bool,
    typed: Vec<(usize, i32)>,
}

impl Hints {
    fn of(parser: &Parser) -> Self {
        Self {
            electrical: parser.electrical,
            mechanical: parser.mechanical,
            joules: parser.joules,
            saw_pi: parser.saw_pi,
            celsius: parser.celsius,
            fahrenheit: parser.fahrenheit,
            typed: parser.typed.clone(),
        }
    }

    fn add(&mut self, other: &Hints) {
        self.electrical |= other.electrical;
        self.mechanical |= other.mechanical;
        self.joules |= other.joules;
        self.saw_pi |= other.saw_pi;
        self.celsius |= other.celsius;
        self.fahrenheit |= other.fahrenheit;
        self.typed.extend(&other.typed);
    }
}

/// How one part of typed input is read.
struct Part<'a> {
    /// The named values it may use.
    names: &'a [(String, Quantity)],
    hints: &'a Hints,
    /// The part is a value given a name. It is used, not shown, so it need
    /// not be something the window could show: it may be a bare number, or
    /// have a unit with no name of its own (`2 mA/V^2`).
    value_only: bool,
}

// ---- why an input was refused -----------------------------------------------
//
// The evaluator returns nothing when it is unsure, and that does not change.
// Where it knows exactly why it could not answer, it can also say so
// (`why_not`). It never offers a number with the reason, and it says nothing
// unless every reading it tried was refused for the same reason.

/// How two quantities were being combined.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Joined {
    Added,
    Subtracted,
    Parallel,
}

/// A reason for refusing typed input that is known for certain.
#[derive(Clone, Debug, PartialEq)]
enum Why {
    /// Two different kinds of quantity were combined: `5V + 3mA`.
    Unlike { left: Dim, right: Dim, how: Joined },
    /// `25 C + 30 C`.
    TwoTemperatures,
    /// `10 K - 25 C`.
    TemperatureFromDifference,
    /// A temperature on a scale was multiplied, divided, raised to a power
    /// or put through a function: `2 * 25 C`.
    TemperatureScaled,
    DividedByZero,
    /// A function that takes a plain number was given one with a unit.
    NeedsPlain(String),
    /// `sin(3 V)`.
    NeedsAngle(String),
    /// `asin(2)`, `ln(0)`, `sqrt(-4)`.
    OutOfRange(String),
    /// `2 ^ 3V`.
    PowerWithUnit,
    /// `1/2pi`: above the line or below it?
    AfterDivision,
    /// `230 V @ 120`.
    AngleNeedsUnit,
    /// The input ends where a value was expected: `4.7k *`.
    Unfinished,
    Unclosed,
    Unopened,
    /// `3.5 bar to V`.
    CannotConvert { from: Dim, to: Dim },
    /// `1/(20 ms) to rpm`: cycles or radians?
    PerSecond,
    BelowZero,
    /// A named answer with a value that has a prefix and no unit:
    /// `V = 2.2mA * 4.7k`.
    NoUnit(String),
    /// A lone `m` that could be milli or metres: `2m + 3m`.
    LoneM,
    /// `47 in dB`: power or voltage?
    BareDecibel,
    /// In a formula with its values after it: a name the formula uses
    /// that was given no value.
    NotGiven(String),
    /// A value that was given and is used nowhere.
    NotUsed(String),
    GivenTwice(String),
    /// A word that could be a unit or a value the user named: the `m` of
    /// `100 N/m` where a value is called `m`.
    UnitOrName(String),
    /// Values written with nothing between them: `RC`. With it, how to
    /// write them.
    RunTogether(String, String),
    /// A name that already means something: a function, a constant, or
    /// `x`, which is the times sign.
    NameTaken(String),
    /// An equation in which every value is given.
    NothingToSolve,
    /// An equation with two values left to find.
    TwoNotGiven(String, String),
    /// No kind of quantity for the value being found lets the two sides
    /// of the equation be compared.
    NoKind(String),
    /// No value was found that makes the two sides equal.
    NoRoot(String),
    /// Several values do, and they are not one value with either sign.
    /// With the name, the values as they would be shown.
    ManyRoots(String, String),
}

/// The kinds of quantity the engine has a name for, each with its article.
const KINDS: &[(Dim, &str)] = &[
        (NO_DIM, "a plain number"),
        (VOLTAGE, "a voltage"),
        (CURRENT, "a current"),
        (RESISTANCE, "a resistance"),
        (CONDUCTANCE, "a conductance"),
        (CAPACITANCE, "a capacitance"),
        (INDUCTANCE, "an inductance"),
        (POWER, "a power"),
        // Newton metres and joules have the same dimensions.
        (ENERGY, "an energy or torque"),
        (CHARGE, "a charge"),
        (FREQUENCY, "a frequency"),
        (TIME, "a time"),
        (LENGTH, "a length"),
        (AREA, "an area"),
        (VOLUME, "a volume"),
        (MASS, "a mass"),
        (SPEED, "a speed"),
        (ACCELERATION, "an acceleration"),
        (FLOW, "a flow"),
        (FORCE, "a force"),
        (PRESSURE, "a pressure"),
        (MAGNETIC_FLUX, "a magnetic flux"),
        (FLUX_DENSITY, "a flux density"),
        (ANGLE_DIM, "an angle"),
        (ANGULAR_SPEED, "a rotation speed"),
        (TEMPERATURE, "a temperature"),
        (THERMAL_RESISTANCE, "a thermal resistance"),
];

/// What a quantity with these dimensions is called.
fn kind(dim: Dim) -> Option<&'static str> {
    KINDS.iter().find(|(known, _)| *known == dim).map(|(_, name)| *name)
}

fn capital(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

impl Why {
    /// The reason as one plain line for the window.
    fn say(&self) -> String {
        match self {
            Why::Unlike { left, right, how } => match (kind(*left), kind(*right), how) {
                (Some(a), Some(b), Joined::Added) => format!("{} and {b} cannot be added.", capital(a)),
                (Some(a), Some(b), Joined::Subtracted) => format!("{} cannot be subtracted from {a}.", capital(b)),
                (Some(a), Some(b), Joined::Parallel) => format!("{} and {b} cannot be put in parallel.", capital(a)),
                (_, _, Joined::Added) => "Those are different kinds of quantity, so they cannot be added.".into(),
                (_, _, Joined::Subtracted) => {
                    "Those are different kinds of quantity, so one cannot be subtracted from the other.".into()
                }
                (_, _, Joined::Parallel) => {
                    "Those are different kinds of quantity, so they cannot be put in parallel.".into()
                }
            },
            Why::TwoTemperatures => "Two temperatures cannot be added. One less the other is a difference.".into(),
            Why::TemperatureFromDifference => "A temperature cannot be subtracted from a difference.".into(),
            Why::TemperatureScaled => {
                "A temperature cannot be multiplied or divided. A difference in K can.".into()
            }
            Why::DividedByZero => "That divides by zero.".into(),
            Why::NeedsPlain(name) => format!("{name} needs a plain number. Divide by a unit first."),
            Why::NeedsAngle(name) => format!("{name} needs an angle or a plain number."),
            Why::OutOfRange(name) => match name.as_str() {
                "asin" | "acos" => format!("{name} needs a number from −1 to 1."),
                "ln" | "log" => format!("{name} needs a number above zero."),
                _ => "A negative number has no real square root.".into(),
            },
            Why::PowerWithUnit => "A power must be a plain number.".into(),
            Why::AfterDivision => {
                "What follows the division could be above or below the line. Add brackets, as in 1/(2*pi).".into()
            }
            Why::AngleNeedsUnit => "The angle needs a unit: deg or rad.".into(),
            Why::Unfinished => "The expression is not finished.".into(),
            Why::Unclosed => "A bracket is not closed.".into(),
            Why::Unopened => "A closing bracket has no opening one.".into(),
            Why::CannotConvert { from, to } => match (kind(*from), kind(*to)) {
                (Some(a), Some(b)) => format!("{} cannot be converted to {b}.", capital(a)),
                _ => "That cannot be converted to the unit asked for.".into(),
            },
            Why::PerSecond => {
                "A per-second value could be cycles or radians. Type it in Hz to convert it to a rotation speed."
                    .into()
            }
            Why::BelowZero => "That is below absolute zero.".into(),
            Why::NoUnit(text) => format!("{text} has a prefix but no unit."),
            Why::LoneM => "A lone m could be milli or metres here. Give it a unit, as in mA.".into(),
            Why::BareDecibel => {
                "A plain number in dB could be a power or a voltage ratio. Give both sides units, as in 2V / 1V to dB."
                    .into()
            }
            Why::NotGiven(name) => format!("{name} is not given a value."),
            Why::NotUsed(name) => format!("{name} is given a value but not used."),
            Why::GivenTwice(name) => format!("{name} is given a value twice."),
            Why::RunTogether(word, apart) => format!("{word} is read as one name. Write it as {apart}."),
            Why::NameTaken(name) => match name.as_str() {
                "x" | "X" => format!("{name} is the times sign here, so it cannot name a value. Use another letter."),
                _ => format!("{name} already means something here, so it cannot name a value."),
            },
            Why::NothingToSolve => "Every value is given, so there is nothing to solve for.".into(),
            Why::TwoNotGiven(first, second) => format!("Two values are not given: {first} and {second}."),
            Why::NoKind(name) => format!("No kind of quantity for {name} makes the two sides agree."),
            Why::NoRoot(name) => format!("No value of {name} was found that makes the two sides equal."),
            Why::ManyRoots(name, values) => format!("More than one value of {name} fits: {values}."),
            Why::UnitOrName(name) => format!(
                "{name} could be a unit or the value you named {name}. Give the value another name, such as {name}_1."
            ),
        }
    }
}

/// Why `add` refused, where that is known.
fn why_not_added(left: Quantity, right: Quantity, sign: f64) -> Option<Why> {
    if left.percent || right.percent {
        return None;
    }
    if left.dim != right.dim {
        let how = if sign > 0.0 { Joined::Added } else { Joined::Subtracted };
        return Some(Why::Unlike { left: left.dim, right: right.dim, how });
    }
    match (left.reading, right.reading) {
        (Some(_), Some(_)) if sign > 0.0 => Some(Why::TwoTemperatures),
        (None, Some(_)) if sign < 0.0 => Some(Why::TemperatureFromDifference),
        _ => None,
    }
}

/// Why `apply` refused, where that is known.
fn why_not_applied(name: &str, argument: Quantity) -> Option<Why> {
    if argument.reading.is_some() {
        return Some(Why::TemperatureScaled);
    }
    if argument.im != 0.0 || argument.dim == NO_DIM {
        return None;
    }
    match name {
        "sin" | "cos" | "tan" => Some(Why::NeedsAngle(name.into())),
        "ln" | "log" | "exp" | "asin" | "acos" | "atan" => Some(Why::NeedsPlain(name.into())),
        _ => None,
    }
}

/// Whether every word in the input is one the engine knows: a unit, a
/// prefix, a function, a constant, `to` or `in`. If not, the input is in
/// words, which is a problem for a model and not a slip in the maths, and
/// no reason is given for refusing it.
fn all_maths(tokens: &[Token]) -> bool {
    tokens.iter().all(|token| match token {
        Token::Word(word) => {
            matches!(word.as_str(), "pi" | "π" | "e" | "j" | "to" | "in")
                || is_function(word)
                || is_complex_function(word)
                || lookup(word).is_some()
                || temperature_scale(word).is_some()
                || level(word).is_some()
                || is_gain(word)
        }
        Token::Name(_) => false,
        _ => true,
    })
}

/// `47 in dB`: a plain number asked for in decibels.
fn bare_decibel(tokens: &[Token], labelled: bool) -> bool {
    let [expression @ .., Token::Word(keyword), Token::Word(unit)] = tokens else { return false };
    if (keyword != "to" && keyword != "in") || !is_gain(unit) || ratio_split(expression).is_some() {
        return false;
    }
    let plain = |quantity: Quantity| quantity.dim == NO_DIM && quantity.im == 0.0 && quantity.value > 0.0;
    matches!(expression, [Token::Number(value)] if *value > 0.0)
        || reading(expression, None, labelled).is_some_and(|outcome| plain(outcome.quantity))
}

/// Why one reading of a lone `K` was refused. Follows `reading_with`.
fn why_reading(
    tokens: &[Token],
    split: Option<usize>,
    labelled: bool,
    kelvin: bool,
    part: &Part,
) -> Option<Why> {
    let no_unit = |outcome: Outcome| {
        (labelled && outcome.bare_prefix).then(|| Why::NoUnit(outcome.bare_text.unwrap_or_default()))
    };

    let mut why_milli = None;
    let milli = attempt_noting(tokens, split, BareM::Milli, kelvin, part, &mut why_milli);
    let saw_bare_m = milli.as_ref().is_none_or(|outcome| outcome.saw_bare_m);
    if !saw_bare_m {
        return milli.and_then(no_unit);
    }
    let mut why_metre = None;
    let metre = attempt_noting(tokens, split, BareM::Metre, kelvin, part, &mut why_metre);

    match (milli, metre) {
        (Some(only), None) | (None, Some(only)) => no_unit(only),
        // Both ways of reading a lone `m` must fail for the same reason.
        (None, None) => why_milli.filter(|why| Some(why) == why_metre.as_ref()),
        (Some(milli), Some(metre)) => match (milli.electrical, milli.mechanical) {
            (true, false) => no_unit(milli),
            (false, true) => no_unit(metre),
            _ => Some(Why::LoneM),
        },
    }
}

/// Says why typed input got no answer, in one plain line, where the reason
/// is certain. Returns nothing for input that is answered, for input in
/// words, and whenever the reason is not known or the readings disagree.
pub fn why_not(input: &str) -> Option<String> {
    let text = input.trim().trim_end_matches('=').trim_end();
    if text.is_empty() || text.chars().count() > 400 || evaluate(input).is_some() {
        return None;
    }
    if let Some(result) = given(text) {
        return result.err().flatten().map(|why| why.say());
    }
    let (labelled, text) = without_label(text);
    let tokens = lex(text, &[])?;
    if tokens.is_empty() || !all_maths(&tokens) {
        return None;
    }
    if bare_decibel(&tokens, labelled) {
        return Some(Why::BareDecibel.say());
    }
    let part = Part { names: &[], hints: &Hints::default(), value_only: false };
    why_refused(&tokens, conversion_split(&tokens), labelled, &part).map(|why| why.say())
}

/// Why every reading of these tokens was refused, if they all agree.
/// Follows `reading_named`.
fn why_refused(tokens: &[Token], split: Option<usize>, labelled: bool, part: &Part) -> Option<Why> {
    let kilo = why_reading(tokens, split, labelled, false, part)?;
    // A lone `K` is read as kilo and as kelvin. Both must agree.
    if tokens.iter().any(|token| matches!(token, Token::Word(word) if word == "K"))
        && why_reading(tokens, split, labelled, true, part).as_ref() != Some(&kilo)
    {
        return None;
    }
    Some(kilo)
}

// ---- values after a formula -------------------------------------------------
//
// `gain = Rd * gm, Rd = 10kohm, gm = 2.1mS`: a formula as it would be
// written on paper, with the values it uses listed after it. This is the
// form Claude writes its working in, typed by the user.
//
// A word standing alone that the user has given a value to is that value,
// even where it is also a unit: `V` in `P = V / R, V = 12V, R = 8ohm`.
// Straight after a number a word is always a unit, as everywhere else, so
// `12V` is twelve volts. Where a word could be read both ways, the input is
// refused (`unit_or_name`).

/// Splits typed input at its commas into a formula and the values given
/// after it. Returns nothing unless every part after the first is a name,
/// an `=` and something, in which case the input is not of this form.
///
/// With no values after it, only an equation counts: something other than
/// a name to the left of its `=`, as in `12V = I * 8ohm`.
fn split_given(text: &str) -> Option<(&str, Vec<(&str, &str)>)> {
    let mut parts = Vec::new();
    let (mut depth, mut start) = (0i32, 0);
    for (at, c) in text.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => depth -= 1,
            ',' if depth == 0 => {
                parts.push(&text[start..at]);
                start = at + 1;
            }
            _ => {}
        }
    }
    parts.push(&text[start..]);

    let (formula, rest) = parts.split_first()?;
    let formula = formula.trim();
    if formula.is_empty() {
        return None;
    }
    if rest.is_empty() {
        let (left, right) = formula.split_once('=')?;
        let (left, right) = (left.trim(), right.trim());
        if left.is_empty() || right.is_empty() || is_label(left) {
            return None;
        }
    }
    let mut values = Vec::new();
    for part in rest {
        let (name, value) = part.split_once('=')?;
        let (name, value) = (name.trim(), value.trim());
        let chars: Vec<char> = name.chars().collect();
        if identifier_end(&chars, 0) != Some(chars.len()) || value.is_empty() || value.contains('=') {
            return None;
        }
        values.push((name, value));
    }
    Some((formula, values))
}

/// Whether a word is a unit, a prefix or a temperature scale.
fn is_unit_word(word: &str) -> bool {
    lookup(word).is_some() || temperature_scale(word).is_some()
}

/// A word that could be read as a unit or as a value the user named.
///
/// Straight after a number a word is always a unit, so `9.81 m` is metres
/// even where a value is called `m`. The doubt is in the run of units that
/// follows: a `/` or `*` and then a name that is also a unit. In
/// `100 N/m` with a value named `m`, the `m` could be metres or that
/// value.
///
/// `finding` is the value an equation is being solved for, which is not
/// in doubt: the equation is only tried with it as a value.
fn unit_or_name(tokens: &[Token], finding: Option<&str>) -> Option<String> {
    let mut in_units = false;
    let mut at = 0;
    while at < tokens.len() {
        match (&tokens[at], tokens.get(at + 1)) {
            (Token::Number(_), Some(Token::Word(_))) => {
                in_units = true;
                at += 2;
            }
            // A power on a unit: `m²`, `s^2`, `s^-2`.
            (Token::Super(_), _) if in_units => at += 1,
            (Token::Caret, Some(Token::Number(_))) if in_units => at += 2,
            (Token::Caret, Some(Token::Minus)) if in_units && matches!(tokens.get(at + 2), Some(Token::Number(_))) => {
                at += 3;
            }
            (Token::Slash | Token::Star, Some(Token::Word(_))) if in_units => at += 2,
            (Token::Slash | Token::Star, Some(Token::Name(name)))
                if in_units && is_unit_word(name) && finding != Some(name.as_str()) =>
            {
                return Some(name.clone());
            }
            _ => {
                in_units = false;
                at += 1;
            }
        }
    }
    None
}

/// The first value written with a prefix and no unit, as typed: `4.7k`.
fn prefix_without_unit(tokens: &[Token]) -> Option<String> {
    tokens.windows(2).find_map(|pair| match pair {
        [Token::Number(value), Token::Word(word)] if matches!(lookup(word), Some(Suffix::BarePrefix(_))) => {
            Some(format!("{value}{word}"))
        }
        _ => None,
    })
}

/// Names with a digit in them, such as `R2`, which the lexer would
/// otherwise read as a word and a number. A unit with a power on it
/// (`mm2`) is not one, and nor is anything straight after a number
/// (`4k7`).
fn numbered_names(text: &str) -> Vec<&str> {
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let mut found = Vec::new();
    let mut at = 0;
    while at < chars.len() {
        if !chars[at].1.is_alphabetic() {
            at += 1;
            continue;
        }
        let start = at;
        while at < chars.len() && (chars[at].1.is_alphanumeric() || chars[at].1 == '_') {
            at += 1;
        }
        let word = &text[chars[start].0..chars.get(at).map_or(text.len(), |(byte, _)| *byte)];
        let before = text[..chars[start].0].trim_end().chars().next_back();
        let after_number = before.is_some_and(|c| c.is_ascii_digit() || c == '.');
        let Some(digit) = word.find(|c: char| c.is_ascii_digit()) else { continue };
        let unit_with_power = is_dimensioned_unit(&word[..digit]) && matches!(&word[digit..], "2" | "3");
        if !after_number && !unit_with_power && !word.contains('_') {
            found.push(word);
        }
    }
    found
}

/// Whether the text has an `x` standing by itself, which the engine reads
/// as a times sign.
fn has_lone_x(text: &str) -> bool {
    let chars: Vec<char> = text.chars().collect();
    let part_of_word = |c: Option<&char>| c.is_some_and(|c| c.is_alphanumeric() || *c == '_');
    (0..chars.len()).any(|at| {
        matches!(chars[at], 'x' | 'X')
            && !part_of_word(at.checked_sub(1).and_then(|before| chars.get(before)))
            && !part_of_word(chars.get(at + 1))
    })
}

/// A formula, or an equation, with the values given after it.
struct Form<'a> {
    /// Each part as text, with the name it gives a value to. The sides
    /// come first, with no name: one for a formula, two for an equation.
    texts: Vec<(&'a str, &'a str)>,
    /// The values given, as name and text.
    values: &'a [(&'a str, &'a str)],
    equation: bool,
    /// Names with a digit in them, given or not.
    numbered: Vec<&'a str>,
}

impl Form<'_> {
    fn sides(&self) -> usize {
        if self.equation { 2 } else { 1 }
    }

    /// Every part as tokens. `finding` is the value an equation is being
    /// solved for; it is a name wherever it stands alone, even if it is
    /// also a unit's letter. A value cannot use its own name, which lets
    /// `V = 12V` be twelve volts.
    fn lex(&self, finding: Option<&str>) -> Result<Vec<Vec<Token>>, Option<Why>> {
        self.texts
            .iter()
            .map(|(own, text)| {
                let names: Vec<(String, Quantity)> = self
                    .values
                    .iter()
                    .map(|(name, _)| *name)
                    .chain(self.numbered.iter().copied())
                    .chain(finding)
                    .filter(|name| name != own)
                    .map(|name| (name.to_string(), Quantity::plain(1.0)))
                    .collect();
                let tokens = lex(text, &names).ok_or(None)?;
                match unit_or_name(&tokens, finding) {
                    Some(word) => Err(Some(Why::UnitOrName(word))),
                    None => Ok(tokens),
                }
            })
            .collect()
    }

    /// Works the form out. For an equation, `finding` is the value to
    /// find; with none, the equation is only checked for having nothing
    /// left to find.
    fn work_out(&self, finding: Option<&str>) -> Result<Outcome, Option<Why>> {
        let mut parts = self.lex(finding)?;
        let sides = self.sides();

        // Every value given must be used, or the formula is not the one
        // meant.
        let uses =
            |name: &str| parts.iter().flatten().any(|token| matches!(token, Token::Name(used) if used == name));
        if let Some((name, _)) = self.values.iter().find(|(name, _)| !uses(name)) {
            return Err(Some(Why::NotUsed(name.to_string())));
        }
        if self.equation {
            // A conversion has no place in an equation, and a prefix with
            // no unit is refused as it is for any named answer.
            if parts[..sides].iter().any(|tokens| conversion_split(tokens).is_some()) {
                return Err(None);
            }
            if let Some(text) = parts[..sides].iter().find_map(|tokens| prefix_without_unit(tokens)) {
                return Err(Some(Why::NoUnit(text)));
            }
        }

        // Whether the work is electrical or mechanical, from the units of
        // every part. It settles a lone `m` anywhere in the input.
        let mut flags = Hints::default();
        for token in parts.iter().flatten() {
            if let Token::Word(word) = token
                && let Some(Suffix::Unit { index, .. }) = lookup(word)
            {
                match UNITS[index].family {
                    Electrical => flags.electrical = true,
                    Mechanical => flags.mechanical = true,
                    Neutral => {}
                }
            }
        }

        // The values, each as soon as the values it uses are known. Those
        // that need the value being found wait, and are worked out with
        // each trial.
        let mut known: Vec<(String, Quantity)> = Vec::new();
        let mut carried = flags.clone();
        let mut waiting: Vec<(&str, Vec<Token>)> =
            self.values.iter().map(|(name, _)| *name).zip(parts.split_off(sides)).collect();
        let ready = |tokens: &[Token], known: &[(String, Quantity)]| {
            tokens.iter().all(|token| match token {
                Token::Name(name) => known.iter().any(|(have, _)| have == name),
                _ => true,
            })
        };
        while let Some(at) = waiting.iter().position(|(_, tokens)| ready(tokens, &known)) {
            let (name, tokens) = waiting.remove(at);
            // A value is named, so a prefix with no unit is refused, as it
            // is for any named answer.
            let part = Part { names: &known, hints: &flags, value_only: true };
            let outcome = reading_named(&tokens, None, true, &part)
                .ok_or_else(|| why_refused(&tokens, None, true, &part))?;
            // A percentage does not keep its meaning as a bare value.
            if outcome.raw.percent {
                return Err(None);
            }
            carried.add(&outcome.hints);
            known.push((name.to_string(), outcome.raw));
        }

        if !self.equation {
            // Values still waiting depend on one another in a ring.
            if !waiting.is_empty() {
                return Err(None);
            }
            let split = conversion_split(&parts[0]);
            let part = Part { names: &known, hints: &carried, value_only: false };
            return reading_named(&parts[0], split, true, &part)
                .ok_or_else(|| why_refused(&parts[0], split, true, &part));
        }

        // An equation. `apart` says how far its two sides are from equal
        // with these names, and how large they are.
        let apart = |mut names: Vec<(String, Quantity)>| -> Option<(f64, f64, Hints)> {
            let mut hints = carried.clone();
            let mut to_do: Vec<&(&str, Vec<Token>)> = waiting.iter().collect();
            while let Some(at) = to_do.iter().position(|(_, tokens)| ready(tokens, &names)) {
                let (name, tokens) = to_do.remove(at);
                let part = Part { names: &names, hints: &flags, value_only: true };
                let outcome = reading_named(tokens, None, true, &part)?;
                if outcome.raw.percent {
                    return None;
                }
                hints.add(&outcome.hints);
                names.push((name.to_string(), outcome.raw));
            }
            if !to_do.is_empty() {
                return None;
            }
            let mut side = |tokens: &[Token]| {
                let part = Part { names: &names, hints: &flags, value_only: true };
                let outcome = reading_named(tokens, None, true, &part)?;
                hints.add(&outcome.hints);
                Some(outcome.raw)
            };
            let (left, right) = (side(&parts[0])?, side(&parts[1])?);
            let comparable = left.dim == right.dim
                && left.reading.is_some() == right.reading.is_some()
                && left.im == 0.0
                && right.im == 0.0
                && !left.percent
                && !right.percent;
            if !comparable {
                return None;
            }
            let (left, right) = (left.kelvin().unwrap_or(left.value), right.kelvin().unwrap_or(right.value));
            let gap = left - right;
            gap.is_finite().then(|| (gap, left.abs().max(right.abs()), hints))
        };

        let Some(finding) = finding else {
            // Nothing is being found. If the two sides can be compared as
            // they stand, every value was given.
            return Err(apart(known.clone()).map(|_| Why::NothingToSolve));
        };
        let with = |x: Quantity| {
            let mut names = known.clone();
            names.push((finding.to_string(), x));
            apart(names)
        };

        // What kind of quantity is being found: the one kind for which the
        // two sides can be compared at all.
        const TRIALS: [f64; 7] = [1.0, 2.0, 0.5, 1e-3, 1e3, 1e-6, 1e6];
        let mut kinds = KINDS
            .iter()
            .map(|(dim, _)| *dim)
            .filter(|dim| TRIALS.iter().any(|&x| with(Quantity::of(x, *dim)).is_some()));
        let Some(dim) = kinds.next() else {
            return Err(Some(Why::NoKind(finding.to_string())));
        };
        if kinds.next().is_some() {
            return Err(None);
        }

        let roots = roots_of(|x| with(Quantity::of(x, dim)).map(|(gap, size, _)| (gap, size)));
        let shown = |value: f64| -> Option<Outcome> {
            let root = Quantity::of(value, dim);
            let hints = with(root)?.2;
            let names = [(finding.to_string(), root)];
            let part = Part { names: &names, hints: &hints, value_only: false };
            reading_named(&[Token::Name(finding.to_string())], None, false, &part)
        };
        let spoken = |value: f64| {
            shown(value)
                .map(|outcome| format!("{} {}", outcome.answer.number, outcome.answer.unit).trim_end().to_string())
        };
        match roots[..] {
            [] => Err(Some(Why::NoRoot(finding.to_string()))),
            [only] => shown(only).ok_or(None),
            // The same size with opposite signs: the positive one, with
            // the other on the line beneath.
            [a, b] if (a + b).abs() <= 1e-9 * a.abs().max(b.abs()) => {
                let (positive, negative) = if a > 0.0 { (a, b) } else { (b, a) };
                let mut outcome = shown(positive).ok_or(None)?;
                outcome.answer.parts = Some(format!("or {}", spoken(negative).ok_or(None)?));
                Ok(outcome)
            }
            _ => {
                let list: Option<Vec<String>> = roots.iter().take(4).map(|&root| spoken(root)).collect();
                let list = list.ok_or(None)?;
                let (last, rest) = list.split_last().ok_or(None)?;
                Err(Some(Why::ManyRoots(finding.to_string(), format!("{} and {last}", rest.join(", ")))))
            }
        }
    }
}

/// Works out a formula with the values given after it, or an equation with
/// one value left to find. Returns nothing if the input is not of that
/// form. Otherwise the answer, or a refusal with the reason where that is
/// certain.
fn given(text: &str) -> Option<Result<Outcome, Option<Why>>> {
    let (formula, values) = split_given(text)?;
    Some(given_parts(formula, &values))
}

fn given_parts(formula: &str, values: &[(&str, &str)]) -> Result<Outcome, Option<Why>> {
    for (index, (name, _)) in values.iter().enumerate() {
        // A function, a constant, a keyword or the times sign cannot also
        // name a value.
        if !is_valid_name(name) || is_complex_function(name) || matches!(*name, "j" | "π") {
            return Err(Some(Why::NameTaken(name.to_string())));
        }
        if values[..index].iter().any(|(earlier, _)| earlier == name) {
            return Err(Some(Why::GivenTwice(name.to_string())));
        }
    }
    let is_given = |word: &str| values.iter().any(|(name, _)| *name == word);

    // What is left of the `=`. A name there that was given no value is only
    // the user's name for the answer. Anything else makes it an equation.
    let (left, right) = match formula.split_once('=') {
        Some((left, right)) => (Some(left.trim()), right.trim()),
        None => (None, formula),
    };
    if right.contains('=') {
        return Err(None);
    }
    let left = left.filter(|left| !is_label(left) || is_given(left));
    let texts: Vec<(&str, &str)> =
        left.map(|left| ("", left)).into_iter().chain([("", right)]).chain(values.iter().copied()).collect();
    let numbered: Vec<&str> = texts.iter().flat_map(|(_, text)| numbered_names(text)).collect();
    let form = Form { texts, values, equation: left.is_some(), numbered };
    let parts = form.lex(None)?;

    // Words nothing knows. In an equation one of them is the value to
    // find; in a formula it is a value that was not given.
    let mut strange: Vec<&str> = Vec::new();
    // Unit letters not straight after a number. In an equation, one of
    // them may be the value to find and not the unit. Those standing alone
    // (`V` in `V^2 / R`) are likelier to be than those in a run of units
    // behind a number (`C` in `1Mohm*C`), so the two are kept apart.
    let mut alone: Vec<&str> = Vec::new();
    let mut in_a_run: Vec<&str> = Vec::new();
    for tokens in &parts {
        let mut in_units = false;
        for (at, token) in tokens.iter().enumerate() {
            let before = at.checked_sub(1).map(|before| &tokens[before]);
            let after_number = matches!(before, Some(Token::Number(_)));
            // The same run of units that `unit_or_name` follows.
            in_units = match token {
                Token::Word(_) if after_number => true,
                Token::Word(_) | Token::Super(_) | Token::Slash | Token::Star | Token::Caret => in_units,
                Token::Number(_) | Token::Minus if matches!(before, Some(Token::Caret | Token::Minus)) => in_units,
                _ => false,
            };
            let (list, word) = match token {
                Token::Name(name) if !is_given(name) => (&mut strange, name.as_str()),
                Token::Word(word) if !all_maths(std::slice::from_ref(token)) => (&mut strange, word.as_str()),
                Token::Word(word)
                    if !after_number && is_unit_word(word) && !matches!(word.as_str(), "to" | "in") =>
                {
                    (if in_units { &mut in_a_run } else { &mut alone }, word.as_str())
                }
                _ => continue,
            };
            if !list.contains(&word) {
                list.push(word);
            }
        }
    }

    let names: Vec<&str> = values.iter().map(|(name, _)| *name).collect();
    if let [word] = strange[..]
        && let Some(pieces) = run_together(word, &names)
    {
        // Several values run together, as in `2πRC`.
        return Err(Some(Why::RunTogether(word.to_string(), pieces.join("*"))));
    }
    if !form.equation {
        return match strange[..] {
            [] => form.work_out(None),
            // One strange word is a value that was not given. More than
            // that means the input is in words, which is for a model.
            [word] => Err(Some(Why::NotGiven(word.to_string()))),
            _ => Err(None),
        };
    }
    match strange[..] {
        [word] => {
            let chars: Vec<char> = word.chars().collect();
            if identifier_end(&chars, 0) != Some(chars.len()) || !is_valid_name(word) {
                return Err(None);
            }
            form.work_out(Some(word))
        }
        [first, second] => Err(Some(Why::TwoNotGiven(first.to_string(), second.to_string()))),
        [] => {
            // A unit's letter may be the value to find. It is, if the
            // equation makes sense with it as a value, and with no other
            // of its kind. `Err(None)` here means more than one did.
            let one_of = |letters: &[&str]| -> Option<Result<Outcome, Option<Why>>> {
                let mut found = None;
                for letter in letters {
                    let result = form.work_out(Some(letter));
                    if matches!(result, Ok(_) | Err(Some(Why::NoRoot(_) | Why::ManyRoots(..)))) {
                        if found.is_some() {
                            return Some(Err(None));
                        }
                        found = Some(result);
                    }
                }
                found
            };
            // Letters standing alone first. Then, if the equation holds
            // together as it is written, there is nothing to find. Only
            // if it does not is a letter in a run of units tried.
            if let Some(result) = one_of(&alone) {
                return result;
            }
            if let Err(Some(Why::NothingToSolve)) = form.work_out(None) {
                return Err(Some(Why::NothingToSolve));
            }
            one_of(&in_a_run).unwrap_or_else(|| {
                let lone_x = form.texts.iter().any(|(_, text)| has_lone_x(text));
                Err(lone_x.then(|| Why::NameTaken("x".into())))
            })
        }
        _ => Err(None),
    }
}

/// The values of `x` at which `apart` comes to nothing, for `x` from
/// 10⁻¹⁵ to 10¹⁵ either side of zero, and zero itself.
///
/// `apart` gives how far the two sides of an equation are from equal, and
/// how large they are, or nothing where they cannot be worked out. The
/// range is walked sixteen steps to the decade; wherever the sides
/// cross, the place is closed in on by halving. A crossing that is not a
/// meeting (either side of `1/x` at zero, say) is thrown out by checking
/// that the sides really are equal there.
///
/// Two answers within about fifteen per cent of one another can be missed,
/// as can one where the sides touch without crossing. A value that is
/// returned always fits: it has been put back into the equation.
fn roots_of(apart: impl Fn(f64) -> Option<(f64, f64)>) -> Vec<f64> {
    let meets = |x: f64| apart(x).is_some_and(|(gap, size)| gap.abs() <= 1e-9 * size || gap == 0.0);
    let mut roots: Vec<f64> = Vec::new();
    let mut found = |x: f64| {
        if meets(x) && !roots.iter().any(|&have| (have - x).abs() <= 1e-9 * have.abs().max(x.abs())) {
            roots.push(x);
        }
    };

    if apart(0.0).is_some_and(|(gap, _)| gap == 0.0) {
        found(0.0);
    }
    for sign in [1.0, -1.0] {
        let mut before: Option<(f64, f64)> = None;
        for step in -240..=240 {
            let x = sign * 10f64.powf(f64::from(step) / 16.0);
            let Some((gap, _)) = apart(x) else {
                before = None;
                continue;
            };
            if gap == 0.0 {
                found(x);
                // Carry on from just past it, so that another answer close
                // by is still crossed.
                let past = x * (1.0 + 1e-6);
                before = apart(past).map(|(gap, _)| (past, gap));
                continue;
            } else if let Some((mut low, low_gap)) = before
                && low_gap != 0.0
                && (low_gap < 0.0) != (gap < 0.0)
            {
                // Halve between the two, by size, until they meet.
                let mut high = x;
                for _ in 0..200 {
                    let middle = sign * (low.abs() * high.abs()).sqrt();
                    if middle == low || middle == high {
                        break;
                    }
                    match apart(middle) {
                        Some((middle_gap, _)) if middle_gap != 0.0 && (middle_gap < 0.0) == (low_gap < 0.0) => {
                            low = middle;
                        }
                        Some(_) => high = middle,
                        None => break,
                    }
                }
                found(high);
            }
            before = Some((x, gap));
        }
    }
    roots.sort_by(f64::total_cmp);
    roots
}

/// Splits a word into values the user named, and π, if it is nothing but
/// two or more of them run together: `RC`, `πRC`.
fn run_together<'a>(word: &'a str, names: &[&'a str]) -> Option<Vec<&'a str>> {
    fn split<'a>(rest: &'a str, names: &[&'a str], found: &mut Vec<&'a str>) -> bool {
        if rest.is_empty() {
            return found.len() > 1;
        }
        for piece in names.iter().copied().chain(["π", "pi"]) {
            if let Some(after) = rest.strip_prefix(piece) {
                found.push(&rest[..piece.len()]);
                if split(after, names, found) {
                    return true;
                }
                found.pop();
            }
        }
        false
    }
    let mut found = Vec::new();
    split(word, names, &mut found).then_some(found)
}

/// `kelvin` reads a lone `K` as kelvin, not as the prefix kilo.
fn attempt(
    tokens: &[Token],
    split: Option<usize>,
    bare_m: BareM,
    kelvin: bool,
    names: &[(String, Quantity)],
) -> Option<Outcome> {
    let part = Part { names, hints: &Hints::default(), value_only: false };
    attempt_noting(tokens, split, bare_m, kelvin, &part, &mut None)
}

/// As `attempt`. When it refuses and knows why, the reason is left in `why`.
fn attempt_noting(
    tokens: &[Token],
    split: Option<usize>,
    bare_m: BareM,
    kelvin: bool,
    part: &Part,
    why: &mut Option<Why>,
) -> Option<Outcome> {
    // Typed input must be worth answering: more than a bare number. A step
    // of a program may be as simple as `200` or another step's name.
    let in_program = bare_m == BareM::Program;
    let (expression, wanted) = match split {
        Some(at) => (&tokens[..at], Some(&tokens[at + 1..])),
        None => (tokens, None),
    };

    let mut parser = Parser::new(expression, bare_m, part.names);
    parser.kelvin = kelvin;
    parser.electrical = part.hints.electrical;
    parser.mechanical = part.hints.mechanical;
    parser.joules = part.hints.joules;
    parser.saw_pi = part.hints.saw_pi;
    parser.celsius = part.hints.celsius;
    parser.fahrenheit = part.hints.fahrenheit;
    parser.typed = part.hints.typed.clone();
    let Some(result) = parser.sum() else {
        *why = parser.why.take();
        return None;
    };
    let result = result.settled();
    if parser.pos != expression.len() {
        if parser.peek() == Some(&Token::Close) {
            *why = Some(Why::Unopened);
        }
        return None;
    }
    if !parser.has_value || !result.value.is_finite() || !result.im.is_finite() {
        return None;
    }
    if part.value_only {
        // Nothing can be colder than absolute zero, named or not.
        if result.kelvin().is_some_and(|kelvin| kelvin < -1e-9) {
            *why = Some(Why::BelowZero);
            return None;
        }
        return Some(Outcome {
            answer: shown_as_is(result.value, ""),
            quantity: result,
            saw_bare_m: parser.saw_bare_m,
            electrical: parser.electrical,
            mechanical: parser.mechanical,
            bare_prefix: parser.bare_prefix,
            scale_typed: parser.celsius || parser.fahrenheit,
            k_rate: parser.k_rate,
            raw: result,
            hints: Hints::of(&parser),
            bare_text: parser.bare_text.take(),
        });
    }
    if !in_program && !parser.interesting && wanted.is_none() {
        return None;
    }

    // A temperature is converted to a scale, not to a unit: `to F` after
    // a temperature is Fahrenheit, though `F` is otherwise the farad.
    let to_scale = match wanted {
        Some([Token::Word(word)]) if result.dim == TEMPERATURE => temperature_scale(word),
        _ => None,
    };
    let target = match wanted {
        Some(unit) if to_scale.is_none() => Some(parse_target(unit)?),
        _ => None,
    };
    let scale_typed = parser.celsius || parser.fahrenheit;

    // Typed input asked for in a unit of another kind. The reason is noted
    // here; the refusal itself is left to the code below, as before.
    if let Some(target) = &target
        && !in_program
        && result.dim != target.dim
    {
        let ordinary = result.im == 0.0 && result.reading.is_none();
        let in_hertz = typed_in_hertz(&parser, result.dim);
        if !ordinary || across_angle(result, target, in_hertz).is_none() {
            *why = Some(if ordinary && result.dim == FREQUENCY && target.dim == ANGULAR_SPEED {
                Why::PerSecond
            } else {
                Why::CannotConvert { from: result.dim, to: target.dim }
            });
        }
    }

    let answer = if result.im != 0.0 {
        shown_complex(result, target.as_ref(), parser.joules)?
    } else if result.dim == TEMPERATURE && target.is_none() {
        if result.kelvin().is_some_and(|kelvin| kelvin < -1e-9) {
            *why = Some(Why::BelowZero);
        }
        shown_temperature(result, to_scale, parser.typed_scale(), scale_typed)?
    } else if result.reading.is_some() {
        return None;
    } else if let Some(target) = &target {
        let value = if result.dim == target.dim {
            result.value
        } else if in_program {
            // Claude is told not to convert between rpm and Hz with `to`.
            return None;
        } else {
            across_angle(result, target, typed_in_hertz(&parser, result.dim))?
        };
        shown_as_is(value / target.factor, &target.label)
    } else if result.dim == THERMAL_RESISTANCE {
        // In the scale typed, as a datasheet gives it: °C/W.
        let scale = parser.typed_scale().unwrap_or(Scale::Kelvin);
        shown_as_is(result.value / scale.degree(), &format!("{}/W", scale.symbol()))
    } else {
        // If the result is in the same kind of unit the user typed, and that
        // unit does not take prefixes, answer in it: `3 bar * 2` is 6 bar.
        let same_kind: Vec<(usize, i32)> = parser
            .typed
            .iter()
            .copied()
            .filter(|&(index, power)| UNITS[index].dim.map(|d| d * power as i8) == result.dim)
            .collect();
        match same_kind.first() {
            Some(&(index, power))
                if !UNITS[index].prefixable && same_kind.iter().all(|&t| t == (index, power)) =>
            {
                let unit = &UNITS[index];
                let label = format!("{}{}", unit.names[0], superscript(power));
                shown_as_is(result.value / unit.factor.powi(power), &label)
            }
            // Typed input only: what a program shows is as it was.
            _ if !in_program && result.dim == FREQUENCY && !parser.counts_cycles() => {
                shown_as_is(result.value, "s⁻¹")
            }
            // In a program, an in-between value may have a unit with no
            // name (mA/V² for a MOSFET's k). It is shown in SI base units
            // and labelled as such, so the steps after it can still run.
            _ => match shown_by_default(result.value, result.dim, parser.joules) {
                Some(answer) => answer,
                None if in_program => shown_as_is(result.value, "(SI)"),
                None => return None,
            },
        }
    };

    Some(Outcome {
        answer,
        // A reading is handed on as an absolute temperature, in kelvin.
        quantity: Quantity { value: result.kelvin().unwrap_or(result.value), ..result },
        saw_bare_m: parser.saw_bare_m,
        bare_prefix: parser.bare_prefix,
        electrical: parser.electrical || target.as_ref().is_some_and(|t| t.electrical),
        mechanical: parser.mechanical || target.as_ref().is_some_and(|t| t.mechanical),
        scale_typed,
        k_rate: parser.k_rate,
        raw: result,
        hints: Hints::of(&parser),
        bare_text: parser.bare_text.take(),
    })
}

/// A complex result. The big line is its magnitude and angle, which are
/// what is measured; the real and imaginary parts go on a line beneath, in
/// the same unit and prefix. Copying gives the magnitude.
fn shown_complex(z: Quantity, target: Option<&Target>, joules: bool) -> Option<Answer> {
    let size = z.magnitude();
    let (shown, scale) = match target {
        Some(target) => {
            if z.dim != target.dim {
                return None;
            }
            (shown_as_is(size / target.factor, &target.label), target.factor)
        }
        None => {
            let shown = shown_by_default(size, z.dim, joules)?;
            // What the magnitude was divided by to be shown: the prefix.
            let number: f64 = shown.number.replace('−', "-").parse().ok()?;
            let scale = 10f64.powf((size / number).log10().round());
            // Only a prefix will do: the parts must be in the same unit.
            if !scale.is_normal() || (size / number / scale - 1.0).abs() > 0.01 {
                return None;
            }
            (shown, scale)
        }
    };
    let part = |value: f64| shown_as_is(value / scale, "").number;
    let unit = if shown.unit.is_empty() { String::new() } else { format!(" {}", shown.unit) };
    let sign = if z.im < 0.0 { '−' } else { '+' };
    let degrees = z.im.atan2(z.value).to_degrees();
    Some(Answer {
        angle: Some(format!("{}°", shown_as_is(degrees, "").number)),
        parts: Some(format!("{} {sign} j{}{unit}", part(z.value), part(z.im.abs()))),
        ..shown
    })
}

/// Whether the result is a frequency the user gave in hertz, so that it is
/// known to count cycles. A bare `1/s`, as from `1/(R*C)`, may as well be
/// radians a second.
fn typed_in_hertz(parser: &Parser, dim: Dim) -> bool {
    let mut same_kind = parser
        .typed
        .iter()
        .filter(|&&(index, power)| UNITS[index].dim.map(|d| d * power as i8) == dim)
        .peekable();
    dim == FREQUENCY && same_kind.peek().is_some() && same_kind.all(|&(index, _)| UNITS[index].names[0] == "Hz")
}

/// Converts a result to a target whose dimensions differ from it only by
/// angle, where that has one meaning. Gives the value in the target's SI
/// units.
///
/// - Rotation asked for in hertz, or hertz asked for as rotation: one
///   revolution is one cycle, so `1500 rpm to Hz` is 25 Hz and
///   `50 Hz to rad/s` is 314.2 rad/s.
/// - Anything else that only loses an angle: the radian is dimensionless in
///   SI, so torque times angular speed is power and `to hp` is allowed.
fn across_angle(result: Quantity, target: &Target, in_hertz: bool) -> Option<f64> {
    const CYCLE: f64 = 2.0 * PI;
    if result.dim == ANGULAR_SPEED && target.hertz {
        return Some(result.value / CYCLE);
    }
    if result.dim == FREQUENCY && target.dim == ANGULAR_SPEED {
        return in_hertz.then_some(result.value * CYCLE);
    }
    let mut without_angle = result.dim;
    without_angle[ANGLE] = 0;
    (target.dim[ANGLE] == 0 && without_angle == target.dim).then_some(result.value)
}

/// A result that is a temperature: a reading on a scale, or a difference.
///
/// `to` is the scale asked for, `typed` the one scale the user typed if
/// there was only one, and `scale_typed` whether they typed any.
fn shown_temperature(
    result: Quantity,
    to: Option<Scale>,
    typed: Option<Scale>,
    scale_typed: bool,
) -> Option<Answer> {
    if let Some(kelvin) = result.kelvin() {
        // Nothing is colder than absolute zero.
        if kelvin < -1e-9 {
            return None;
        }
        let scale = to.or(result.reading)?;
        let reading = (kelvin - scale.zero()) / scale.degree();
        // Round off the noise from adding and removing the scale's zero.
        return Some(shown_as_is((reading * 1e9).round() / 1e9, scale.symbol()));
    }
    let scale = match to {
        Some(Scale::Kelvin) => Scale::Kelvin,
        // A number of kelvin alone may be a temperature or a difference,
        // and the two convert differently. Only where a Celsius or
        // Fahrenheit temperature was typed is it known to be a difference.
        Some(scale) => scale_typed.then_some(scale)?,
        None => typed.unwrap_or(Scale::Kelvin),
    };
    Some(shown_as_is(result.value / scale.degree(), scale.difference()))
}

/// Whether `text` is a name the user has put in front of a sum, as in
/// `VGS = 0.3V * 2`: a word such as `VGS`, `V_ov` or `R1`, with or
/// without a tag in brackets (`VGS(off)`).
fn is_label(text: &str) -> bool {
    let chars: Vec<char> = text.chars().collect();
    let Some(end) = identifier_end(&chars, 0) else { return false };
    match &chars[end..] {
        [] => true,
        ['(', tag @ .., ')'] => !tag.is_empty() && tag.iter().all(|c| c.is_alphanumeric() || *c == '_'),
        _ => false,
    }
}

/// Splits off a name written in front of plain maths. The name is the user's
/// own label for the answer and takes no part in the sum, so the rest must
/// still be answerable by itself. Anything else to the left of an `=` makes
/// the input an equation, which is left as it is and refused.
fn without_label(text: &str) -> (bool, &str) {
    match text.split_once('=') {
        Some((left, right)) if is_label(left.trim()) => (true, right.trim_start()),
        _ => (false, text),
    }
}

/// Evaluates plain maths. Returns `None` for anything else, including input
/// that is incomplete, ambiguous, dimensionally inconsistent, or just a bare
/// number.
pub fn evaluate(input: &str) -> Option<Answer> {
    evaluate_with_value(input).map(|(answer, _)| answer)
}

/// As `evaluate`, and also gives the value in SI base units (kelvin for a
/// temperature). Used to check answers against the reference set.
pub fn evaluate_with_value(input: &str) -> Option<(Answer, f64)> {
    let text = input.trim().trim_end_matches('=').trim_end();
    if text.is_empty() || text.chars().count() > 400 {
        return None;
    }
    // A formula with its values after it. A comma means nothing else.
    if let Some(result) = given(text) {
        return result.ok().map(|outcome| {
            (Answer { given: true, ..outcome.answer }, outcome.quantity.magnitude_or_value())
        });
    }
    let (labelled, text) = without_label(text);
    let tokens = lex(text, &[])?;
    if let Some(found) = temperature(&tokens) {
        return Some(found);
    }
    if let Some(found) = decibels(&tokens, labelled) {
        return Some(found);
    }
    let split = conversion_split(&tokens);
    // For a complex answer the value is its magnitude.
    reading(&tokens, split, labelled).map(|outcome| (outcome.answer, outcome.quantity.magnitude_or_value()))
}

/// Works out typed input, or a part of it, as ordinary maths. `split` is
/// where a conversion's `to` is, if there is one.
fn reading(tokens: &[Token], split: Option<usize>, labelled: bool) -> Option<Outcome> {
    reading_named(tokens, split, labelled, &Part { names: &[], hints: &Hints::default(), value_only: false })
}

/// As `reading`, for one part of a formula with its values after it.
fn reading_named(tokens: &[Token], split: Option<usize>, labelled: bool, part: &Part) -> Option<Outcome> {
    // A lone `K` is the prefix kilo (`4.7K`) or kelvin (`25 C + 10 K`).
    // Kilo is tried first, so everything that worked before still does.
    let kilo = reading_with(tokens, split, labelled, false, part);
    if !tokens.iter().any(|token| matches!(token, Token::Word(word) if word == "K")) {
        return kilo;
    }
    let kelvin = reading_with(tokens, split, labelled, true, part);
    match (kilo, kelvin) {
        // Both readings work. It is kelvin beside a Celsius or Fahrenheit
        // temperature, and where it is written as a rate (`40 K/W`).
        (Some(kilo), Some(kelvin)) => Some(if kelvin.scale_typed || kilo.k_rate { kelvin } else { kilo }),
        (kilo, kelvin) => kilo.or(kelvin),
    }
}

fn reading_with(
    tokens: &[Token],
    split: Option<usize>,
    labelled: bool,
    kelvin: bool,
    part: &Part,
) -> Option<Outcome> {
    // Someone who names the answer is working with real quantities. A value
    // with a prefix and no unit (`V = 2.2mA * 4.7k`) then has a unit they
    // left out, and reading it as a plain number would give the answer the
    // wrong unit (amps, there). That needs understanding, so it is refused.
    let done = |outcome: Outcome| (!(labelled && outcome.bare_prefix)).then_some(outcome);

    // A lone `m` after a number is milli in electrical shorthand (`2.2m`)
    // and metres in mechanical work (`5 m`). Try both readings.
    let milli = attempt_noting(tokens, split, BareM::Milli, kelvin, part, &mut None);
    let saw_bare_m = milli.as_ref().is_none_or(|outcome| outcome.saw_bare_m);
    if !saw_bare_m {
        return milli.and_then(done);
    }
    let metre = attempt_noting(tokens, split, BareM::Metre, kelvin, part, &mut None);

    match (milli, metre) {
        (Some(only), None) | (None, Some(only)) => done(only),
        (None, None) => None,
        // Both readings work, so the rest of the input has to settle it.
        // If it cannot, there is no answer.
        (Some(milli), Some(metre)) => match (milli.electrical, milli.mechanical) {
            (true, false) => done(milli),
            (false, true) => done(metre),
            _ => None,
        },
    }
}

// ---- programs ---------------------------------------------------------------
//
// For problems that need reasoning, Claude writes the formulas and this
// engine works out every number. A program is a list of named steps. Each is
// either an expression, which may use the names of earlier steps, or an
// equation to solve for the step's own name.

/// One step of a program.
pub struct Step<'a> {
    pub name: &'a str,
    pub kind: StepKind<'a>,
    /// A unit to show the result in, as after `to`.
    pub to: Option<&'a str>,
}

pub enum StepKind<'a> {
    Expression(&'a str),
    /// Find the value of this step's name that makes the equation true,
    /// starting the search from `guess`. The equation is `left = right`,
    /// or an expression that should come to zero.
    Solve { equation: &'a str, guess: &'a str },
}

/// The value of one step: how to show it, and the value in SI base units.
pub struct Computed {
    pub answer: Answer,
    pub si_value: f64,
}

/// Why a program could not be run, and at which step (counted from zero).
#[derive(Debug, PartialEq)]
pub struct ProgramError {
    pub step: usize,
    pub reason: &'static str,
}

type Names = Vec<(String, Quantity)>;

/// The longest formula a step of a program may be, in characters. Claude's
/// formulas are a line at most; anything far longer is not working.
pub const MAX_STEP_CHARS: usize = 1000;

/// How many times one run of a program may try a value in an equation,
/// over all its `solve` steps together. An equation that converges needs a
/// few dozen tries; this stops a reply built to keep the engine busy.
const SOLVE_TRIES: u32 = 20_000;

fn evaluate_step(text: &str, to: Option<&str>, names: &Names) -> Option<Outcome> {
    if text.chars().count() > MAX_STEP_CHARS || to.is_some_and(|unit| unit.chars().count() > MAX_STEP_CHARS) {
        return None;
    }
    let mut tokens = lex(text, names)?;
    if let Some(unit) = to {
        tokens.push(Token::Word("to".into()));
        tokens.extend(lex(unit, &[])?);
    }
    let split = conversion_split(&tokens);
    attempt(&tokens, split, BareM::Program, false, names)
}

/// How far the equation is from true when the unknown has the value `x`.
fn residual(equation: &str, name: &str, x: Quantity, names: &Names) -> Option<f64> {
    let mut with_unknown = names.clone();
    with_unknown.push((name.to_string(), x));
    let side = |text: &str| evaluate_step(text, None, &with_unknown).map(|o| o.quantity);

    let value = match equation.split_once('=') {
        Some((left, right)) => {
            let (left, right) = (side(left)?, side(right)?);
            if left.dim != right.dim {
                return None;
            }
            left.value - right.value
        }
        None => side(equation)?.value,
    };
    value.is_finite().then_some(value)
}

/// Secant method, shortening any step that leaves the region where the
/// equation can be evaluated (for example a logarithm of a negative number).
///
/// `tries` is how many more values may be tried; it runs down as they are.
fn solve(
    equation: &str,
    name: &str,
    guess: Quantity,
    names: &Names,
    tries: &std::cell::Cell<u32>,
) -> Option<Quantity> {
    let at = |x: f64| {
        tries.set(tries.get().checked_sub(1)?);
        residual(equation, name, Quantity::of(x, guess.dim), names)
    };

    let mut x0 = guess.value;
    let mut f0 = at(x0)?;
    let mut x1 = if x0 == 0.0 { 1e-6 } else { x0 * 1.001 };
    let mut f1 = at(x1)?;

    for _ in 0..200 {
        if f1 == 0.0 {
            return Some(Quantity::of(x1, guess.dim));
        }
        if f1 == f0 {
            return None;
        }
        let mut step = -f1 * (x1 - x0) / (f1 - f0);
        let mut next = None;
        for _ in 0..40 {
            if let Some(f) = at(x1 + step) {
                next = Some((x1 + step, f));
                break;
            }
            step /= 2.0;
        }
        let (x2, f2) = next?;
        if (x2 - x1).abs() <= 1e-13 * x2.abs().max(f64::MIN_POSITIVE) {
            return Some(Quantity::of(x2, guess.dim));
        }
        (x0, f0, x1, f1) = (x1, f1, x2, f2);
    }
    None
}

/// Runs a program and returns the value of every step, in order.
pub fn run_program(steps: &[Step]) -> Result<Vec<Computed>, ProgramError> {
    let mut names: Names = Vec::new();
    let mut computed = Vec::new();
    let tries = std::cell::Cell::new(SOLVE_TRIES);

    for (index, step) in steps.iter().enumerate() {
        let fail = |reason| ProgramError { step: index, reason };
        if !is_valid_name(step.name) {
            return Err(fail("the name cannot be used"));
        }
        if names.iter().any(|(known, _)| known == step.name) {
            return Err(fail("the name is already used"));
        }

        let (quantity, answer) = match step.kind {
            StepKind::Expression(text) => {
                let outcome = evaluate_step(text, step.to, &names)
                    .ok_or_else(|| fail("the expression could not be worked out"))?;
                (outcome.quantity, outcome.answer)
            }
            StepKind::Solve { equation, guess } => {
                let guess = evaluate_step(guess, None, &names)
                    .ok_or_else(|| fail("the starting guess could not be worked out"))?
                    .quantity;
                let root = solve(equation, step.name, guess, &names, &tries)
                    .ok_or_else(|| fail("the equation could not be solved"))?;
                // Show the root the same way any other value would be shown.
                names.push((step.name.to_string(), root));
                let shown = evaluate_step(step.name, step.to, &names);
                names.pop();
                let outcome = shown.ok_or_else(|| fail("the solution could not be shown"))?;
                (root, outcome.answer)
            }
        };

        names.push((step.name.to_string(), quantity));
        computed.push(Computed { answer, si_value: quantity.value });
    }
    Ok(computed)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The answer as the window shows it: number, space, unit.
    fn show(input: &str) -> Option<String> {
        evaluate(input).map(|a| format!("{} {}", a.number, a.unit).trim_end().to_string())
    }

    fn copied(input: &str) -> Option<String> {
        evaluate(input).map(|a| a.copy)
    }

    #[track_caller]
    fn check(input: &str, expected: &str) {
        assert_eq!(show(input).as_deref(), Some(expected), "for {input:?}");
    }

    /// A complex answer in full: the big line, then the line beneath it.
    #[track_caller]
    fn check_complex(input: &str, polar: &str, parts: &str) {
        let answer = evaluate(input).unwrap_or_else(|| panic!("no answer for {input:?}"));
        let big = format!("{} {} ∠ {}", answer.number, answer.unit, answer.angle.as_deref().unwrap_or("?"));
        assert_eq!(big.replace("  ", " "), polar, "for {input:?}");
        assert_eq!(answer.parts.as_deref(), Some(parts), "for {input:?}");
    }

    #[track_caller]
    fn refuse(input: &str) {
        assert_eq!(show(input), None, "should not answer {input:?}");
    }

    #[test]
    fn prefixes_without_units() {
        check("2.2m*4.7k", "10.34");
        check("1/(2*pi*4.7k*100n)", "338.6");
        check("4.7k*100n", "470.0 µ");
        check("2*2500", "5.000 k");
        check("1meg/1k", "1.000 k");
        check("4.7K + 300", "5.000 k");
    }

    #[test]
    fn significant_figures_can_be_chosen() {
        let at = |figures: u8, input: &str| with_sig_figs(figures, || show(input));
        let cutoff = "1/(2*pi*4.7kohm*100nF)";
        assert_eq!(at(3, cutoff).as_deref(), Some("339 Hz"));
        assert_eq!(at(4, cutoff).as_deref(), Some("338.6 Hz"));
        assert_eq!(at(5, cutoff).as_deref(), Some("338.63 Hz"));
        assert_eq!(at(6, cutoff).as_deref(), Some("338.628 Hz"));
        assert_eq!(at(3, "2*2500").as_deref(), Some("5.00 k"));
        assert_eq!(at(6, "10 Nm in lbft").as_deref(), Some("7.37562 lb·ft"));
        assert_eq!(at(3, "1/3").as_deref(), Some("0.333"));
        assert_eq!(at(3, "100 psi to bar").as_deref(), Some("6.89 bar"));
        // What is copied follows what is shown.
        assert_eq!(with_sig_figs(6, || copied("1/3")).as_deref(), Some("0.333333"));
        // Out of range is pulled back in, and the choice does not outlive
        // the work it was made for.
        assert_eq!(at(9, "1/3").as_deref(), Some("0.333333"));
        assert_eq!(at(1, "1/3").as_deref(), Some("0.333"));
        check("1/3", "0.3333");
    }

    #[test]
    fn electrical_units_are_tracked() {
        check("2.2mA*4.7kohm", "10.34 V");
        check("1/(2*pi*4.7kohm*100nF)", "338.6 Hz");
        check("9V*4.7k/(10k+4.7k)", "2.878 V");
        check("12V/8ohm", "1.500 A");
        check("25mV/2mA", "12.50 Ω");
        check("1Mohm*22uF", "22.00 s");
        check("(20V/sqrt(2))^2/8ohm", "25.00 W");
        check("1/(2*pi*sqrt(2.5H*470pF))", "4.643 kHz");
        check("0.047uF", "47.00 nF");
        check("5 V * 2 mA", "10.00 mW");
        check("2mS * 10kΩ", "20.00");
        check("24V * 2A * 10s", "480.0 J");
        check("1mA * 1mohm", "1.000 µV");
        check("0.5 tesla * 20 cm²", "1.000 mWb");
        check("2 mWb / 4 cm²", "5.000 T");
    }

    #[test]
    fn the_users_own_notation() {
        // V_ov = |VGS(off)| × √(ID ÷ IDSS), with the numbers put in.
        check("2.5V × √(1.2mA ÷ 6mA)", "1.118 V");
        check("2π × 1kHz", "6.283 kHz");
    }

    #[test]
    fn parallel_operator() {
        check("10k||22k||47k", "5.998 k");
        check("100kohm || 22kohm", "18.03 kΩ");
        check("1k + 10k||10k", "6.000 k");
        refuse("10kohm || 10nF");
    }

    #[test]
    fn component_notation() {
        check("4k7", "4.700 k");
        check("4R7", "4.700 Ω");
        check("2n2", "2.200 n");
        check("1M5 + 500k", "2.000 M");
    }

    #[test]
    fn precedence_and_functions() {
        check("-2^2", "−4.000");
        check("2^-3", "0.1250");
        check("2^3^2", "512.0");
        check("2**10", "1.024 k");
        check("sqrt(2)", "1.414");
        check("20*log(47)", "33.44");
        check("2pi", "6.283");
        check("2(3+4)", "14.00");
        check("1e-14*(exp(0.65/0.025852)-1)", "830.8 µ");
        check("2+2=", "4.000");
    }

    #[test]
    fn forgiving_input() {
        check("5 x 3", "15.00");
        check("5x3", "15.00");
        check("4.7k X 2", "9.400 k");
        check("25²", "625.0");
        check("2³", "8.000");
        check("200 * 15%", "30.00");
        check("15%", "0.1500");
        check("5% + 10%", "0.1500");
        // Is this 200.15 or 230? It is not for us to choose.
        refuse("200 + 15%");
        refuse("200 - 15%");
    }

    #[test]
    fn conversions() {
        check("3.5 bar to psi", "50.76 psi");
        check("3.5 bar in psi", "50.76 psi");
        check("1 in to mm", "25.40 mm");
        check("25.4 mm to in", "1.000 in");
        check("10 N·m to lb·ft", "7.376 lb·ft");
        check("10 Nm in lbft", "7.376 lb·ft");
        check("5 hp to kW", "3.728 kW");
        check("2 gal to L", "7.571 L");
        check("12 L/min to gpm", "3.170 gal/min");
        check("60 mph to km/h", "96.56 km/h");
        check("1 kWh to MJ", "3.600 MJ");
        check("5 m to ft", "16.40 ft");
        check("2 in² to cm²", "12.90 cm²");
        check("2 in^2 to cm2", "12.90 cm²");
        check("1 atm to kPa", "101.3 kPa");
        check("0.5 thou to µm", "12.70 µm");
        check("14500 psi to MPa", "99.97 MPa");
        check("100 MPa to psi", "14500 psi");
        refuse("3.5 bar to mm");
        refuse("5 V to psi");
        refuse("3.5 bar to");
        refuse("3.5 bar to k");
    }

    #[test]
    fn temperature() {
        check("100 C to F", "212.0 °F");
        check("32 °F to °C", "0 °C");
        check("-40 F to C", "−40.00 °C");
        check("300 K to C", "26.85 °C");
        check("25 degC in K", "298.1 K");
        refuse("-300 C to K");
        // Outside a direct conversion, C and F are not temperatures.
        check("5 F to mF", "5000 mF");
        refuse("20 C + 5 C");
    }

    #[test]
    fn temperatures_and_differences() {
        // A temperature with a rise added to it, or taken away.
        check("25 C + 2 W * 50 C/W", "125.0 °C");
        check("25C + 2W*50C/W", "125.0 °C");
        check("Tj = 25 °C + 2 W * 50 °C/W", "125.0 °C");
        check("2 W * 50 C/W + 25 C", "125.0 °C");
        check("125 C - 2 W * 50 C/W", "25.00 °C");
        check("25 C + 2 W * (0.5 C/W + 1.2 C/W)", "28.40 °C");
        check("-40 C + 10 K", "−30.00 °C");
        check("68 °F + 2 W * 9 °F/W", "86.00 °F");
        // One temperature less another is a difference, marked as one.
        check("85 C - 25 C", "60.00 Δ°C");
        check("77 °F - 32 °F", "45.00 Δ°F");
        check("25 C - 85 C", "−60.00 Δ°C");
        check("212 °F - 100 C", "0 K");
        check("(85 C - 25 C) * 2", "120.0 Δ°C");
        // A rise for each watt is one quantity, whatever comes before it.
        check("(150 C - 25 C) / 62 C/W", "2.016 W");
        check("(150 C - 25 C) / 2 W", "62.50 °C/W");
        check("0.5 C/W + 1.2 C/W", "1.700 °C/W");
        check("8 mW/C * (85 C - 25 C)", "480.0 mW");
        check("20 C/min * 3 min", "60.00 Δ°C");
        // Asked for on another scale.
        check("25 C + 2 W * 50 C/W to F", "257.0 °F");
        check("25 C + 2 W * 50 C/W to K", "398.1 K");
        check("85 C - 25 C to F", "108.0 Δ°F");
        check("85 C - 25 C to K", "60.00 K");
        assert_eq!(copied("25 C + 2 W * 50 C/W").as_deref(), Some("125"));

        // Two temperatures cannot be added, and a temperature cannot be
        // scaled: twice 25 °C is not 50 °C.
        refuse("20 C + 5 C");
        refuse("25 C * 2");
        refuse("2 * 25 C");
        refuse("25 C / 2");
        refuse("sqrt(25 C)");
        refuse("abs(-25 C)");
        refuse("25 C || 50 C");
        refuse("1.38e-23 J/kelvin * 27 C");
        // A difference less a temperature means nothing.
        refuse("5 kelvin - 25 C");
        refuse("(85 C - 25 C) - 25 C");
        // Colder than absolute zero.
        refuse("-300 C + 1 kelvin");
        // Not a temperature and something else.
        refuse("25 C + 5");
        refuse("25 C + 5 V");
        refuse("25 C to K/W");
        // `F` is the farad. Fahrenheit is written °F or degF in a sum.
        check("100 F - 50 F", "50.00 F");
    }

    #[test]
    fn a_lone_k_is_kilo_or_kelvin() {
        // Kilo wherever that works, as before.
        check("4.7K", "4.700 k");
        check("300 K * 2", "600.0 k");
        check("12 V / 4.7K", "2.553 mV");
        check("300 K + 10 K", "310.0 k");
        // Kelvin where kilo makes no sense.
        check("25 C + 10 K", "35.00 °C");
        check("10 K + 25 C", "35.00 °C");
        check("25 C - 10 K", "15.00 °C");
        check("2 W/K * 30 kelvin", "60.00 W");
        check("60 kelvin / 2 W", "30.00 K/W");
        // Kelvin where it is written as a rate.
        check("2 W * 40 K/W", "80.00 K");
        check("40 K/W * 2 W", "80.00 K");
        check("25 C + 2 W * 40 K/W", "105.0 °C");
        // A number of kelvin alone may be a temperature or a difference, so
        // it is converted only as a temperature, in the direct form.
        check("300 K to C", "26.85 °C");
        refuse("2 W * 40 K/W to C");
    }

    #[test]
    fn decibel_levels() {
        // A value as a level.
        check("250mW in dBm", "23.98 dBm");
        check("1 W to dBm", "30.00 dBm");
        check("2 V to dBV", "6.021 dBV");
        check("1.228 V to dBu", "4.002 dBu");
        // A level as a value. Alone, it is answered with what it stands for.
        check("10 dBm", "10.00 mW");
        check("0 dBu", "774.6 mV");
        check("+4 dBu in V", "1.228 V");
        check("10 dBm to mW", "10.00 mW");
        check("P = 10 dBm to mW", "10.00 mW");
        // One level to another of the same kind.
        check("-10 dBV to dBu", "−7.782 dBu");
        check("30 dBm to dBW", "0 dBW");
        // Gains added to a level, and taken away.
        check("10 dBm + 3 dB", "13.00 dBm");
        check("4 dBu - 20 dB + 6 dB", "−10.00 dBu");
        check("10 dBm - 30 dB to uW", "10.00 µW");
        check("10dbm + 3db", "13.00 dBm");
        assert_eq!(copied("10 dBm + 3 dB").as_deref(), Some("13"));

        // Power and voltage levels are related only through an impedance.
        refuse("10 dBm to dBV");
        refuse("10 dBm to V");
        refuse("5 V to dBm");
        // Levels are not added to each other, or scaled.
        refuse("10 dBm + 20 dBm");
        refuse("2 * 10 dBm");
        refuse("10 dBm + 3");
        refuse("10 dBm * 2");
        // No level for nothing, or for less than nothing.
        refuse("0 W to dBm");
        refuse("-5 W to dBm");
        // An old name for dBu, easily taken for dBV.
        refuse("10 dBv");
    }

    #[test]
    fn ratios_in_decibels() {
        check("2V / 1V to dB", "6.021 dB");
        check("10W / 1W to dB", "10.00 dB");
        check("100mV / 2V in dB", "−26.02 dB");
        check("20mA / 4mA to dB", "13.98 dB");
        check("2 * 3V / (1V + 2V) to dB", "6.021 dB");
        check("A = 2V / 1V to dB", "6.021 dB");

        // A plain ratio could be of powers or of voltages.
        refuse("47 in dB");
        refuse("2 to dB");
        refuse("2/1 to dB");
        // Decibels alone are not a quantity, and gains are not summed here.
        refuse("6 dB");
        refuse("20 dB + 6 dB");
        // Unlike things, and things decibels have no agreed meaning for.
        refuse("2V / 1A to dB");
        refuse("2 ohm / 1 ohm to dB");
        // Anything the usual order of working would attach to the ratio.
        refuse("2V / 1V * 3 to dB");
        refuse("2V / 1V + 1 to dB");
        refuse("2V / 1V / 1V to dB");
        refuse("2V / 1V (3) to dB");
    }

    #[test]
    fn complex_numbers() {
        // j after the number, before it, and on its own.
        check_complex("3 + 4j", "5.000 ∠ 53.13°", "3.000 + j4.000");
        check_complex("3 + j4", "5.000 ∠ 53.13°", "3.000 + j4.000");
        check_complex("3 + 4*j", "5.000 ∠ 53.13°", "3.000 + j4.000");
        check_complex("3 - j4", "5.000 ∠ −53.13°", "3.000 − j4.000");
        check_complex("j", "1.000 ∠ 90.00°", "0 + j1.000");
        check_complex("-j2", "2.000 ∠ −90.00°", "0 − j2.000");
        // Arithmetic.
        check_complex("(3 + 4j) * (1 - 2j)", "11.18 ∠ −10.30°", "11.00 − j2.000");
        check_complex("(3 + 4j) / (1 - 2j)", "2.236 ∠ 116.6°", "−1.000 + j2.000");
        check_complex("(1 + j)^2", "2.000 ∠ 90.00°", "0 + j2.000");
        check_complex("sqrt(-3 + 4j)", "2.236 ∠ 63.43°", "1.000 + j2.000");
        check_complex("1/(1 + j)", "0.7071 ∠ −45.00°", "0.5000 − j0.5000");
        // A result with no imaginary part left is an ordinary number.
        check("j * j", "−1.000");
        check("(1 + j) * (1 - j)", "2.000");
        check("(3 + 4j) - 4j", "3.000");
        // Impedances.
        check_complex("10kohm + 1/(j*2*pi*1kHz*100nF)", "10.13 kΩ ∠ −9.043°", "10.00 − j1.592 kΩ");
        check_complex("10kohm + j*2*pi*1kHz*100mH", "10.02 kΩ ∠ 3.595°", "10.00 + j0.6283 kΩ");
        check_complex("1kohm || j*2*pi*2kHz*100mH", "782.5 Ω ∠ 38.51°", "612.3 + j487.2 Ω");
        check_complex("j*2*pi*1kHz*100mH", "628.3 Ω ∠ 90.00°", "0 + j628.3 Ω");
        check_complex("Z = 10kohm - j1.5916kohm", "10.13 kΩ ∠ −9.043°", "10.00 − j1.592 kΩ");
        check_complex("5 V / (10kohm + j*2*pi*1kHz*100mH)", "499.0 µA ∠ −3.595°", "498.0 − j31.29 µA");
        check_complex("(3 + 4j) kohm to ohm", "5000 Ω ∠ 53.13°", "3000 + j4000 Ω");
        // What copy gives: the magnitude, as shown.
        assert_eq!(copied("3 + 4j").as_deref(), Some("5"));
        assert_eq!(copied("10kohm + 1/(j*2*pi*1kHz*100nF)").as_deref(), Some("10130"));
        // The value handed on for checking is the magnitude.
        let (_, value) = evaluate_with_value("3 + 4j").unwrap();
        assert!((value - 5.0).abs() < 1e-12);

        // Ambiguous after a division, as `1/2pi` is.
        refuse("1/4j");
        // Unlike things, and things that are never complex.
        refuse("3 V + 4j");
        refuse("25 C + j*5 kelvin");
        refuse("(3 + 4j)%");
        refuse("ln(3 + 4j)");
        refuse("sin(j)");
        refuse("(3 + 4j) V to dBV");
        refuse("(3 + 4j) / 0");
        // `i` is not the imaginary unit here, and `J` is the joule.
        refuse("3 + 4i");
        check("3 J + 4J", "7.000 J");
    }

    #[test]
    fn magnitude_and_angle() {
        // Entered with the angle sign, or with @.
        check_complex("10 ∠ 30°", "10.00 ∠ 30.00°", "8.660 + j5.000");
        check_complex("10 @ 30 deg", "10.00 ∠ 30.00°", "8.660 + j5.000");
        check_complex("10@30deg", "10.00 ∠ 30.00°", "8.660 + j5.000");
        check_complex("230 V @ 120°", "230.0 V ∠ 120.0°", "−115.0 + j199.2 V");
        check_complex("5 @ 0.5 rad", "5.000 ∠ 28.65°", "4.388 + j2.397");
        check_complex("10 @ -30 deg", "10.00 ∠ −30.00°", "8.660 − j5.000");
        check_complex("10 @ 90°", "10.00 ∠ 90.00°", "0 + j10.00");
        check_complex("2 * 10 @ 30 deg", "20.00 ∠ 30.00°", "17.32 + j10.00");
        check_complex("230 V @ 0° - 230 V @ 120°", "398.4 V ∠ −30.00°", "345.0 − j199.2 V");
        check_complex("(10 @ 30°) / (2 @ 10°)", "5.000 ∠ 20.00°", "4.698 + j1.710");
        check_complex("e^(j*pi/3)", "1.000 ∠ 60.00°", "0.5000 + j0.8660");
        check_complex("10 * exp(j * 30 deg / 1 rad)", "10.00 ∠ 30.00°", "8.660 + j5.000");
        // No imaginary part: an ordinary number.
        check("10 @ 180°", "−10.00");
        check("10 @ 0 deg", "10.00");
        // Taken apart.
        check("abs(3 + 4j)", "5.000");
        check("re(3 + 4j)", "3.000");
        check("im(3 + 4j)", "4.000");
        check("arg(3 + 4j)", "53.13 deg");
        check("phase(10kohm + 1/(j*2*pi*1kHz*100nF))", "−9.043 deg");
        check("arg(3 + 4j) to rad", "0.9273 rad");
        check_complex("conj(3 + 4j)", "5.000 ∠ −53.13°", "3.000 − j4.000");
        check("abs(1/(1 + j*2*pi*5kHz*10kohm*10nF))", "0.3033");
        check("20*log(abs(1/(1 + j*2*pi*5kHz*10kohm*10nF)))", "−10.36");

        // The angle must say what it is in.
        refuse("10 @ 30");
        refuse("10 ∠ 30");
        refuse("10 @ 30 V");
        // The magnitude is an ordinary value, and nothing has no angle.
        refuse("(3 + 4j) @ 30 deg");
        refuse("arg(0)");
    }

    #[test]
    fn programs_have_no_complex_numbers() {
        // Claude works out the parts as separate steps, as it is told to.
        assert!(run(&[expression("Z_t", "3 + 4j")]).is_err());
        assert!(run(&[expression("Z_t", "j*2*pi*1kHz*100mH")]).is_err());
        assert!(run(&[expression("Z_t", "10 @ 30 deg")]).is_err());
        assert!(run(&[expression("Z_t", "re(3)")]).is_err());
        // A step may still have a name that typed input uses as a function.
        assert_eq!(run(&[expression("phase", "atan(1) * 1rad")]).unwrap(), ["0.7854 rad"]);
        assert_eq!(run(&[expression("re", "3 ohm"), expression("im", "4 ohm")]).unwrap(), ["3.000 Ω", "4.000 Ω"]);
    }

    /// Refused, with this reason given.
    #[track_caller]
    fn why(input: &str, expected: &str) {
        assert_eq!(show(input), None, "should not answer {input:?}");
        assert_eq!(why_not(input).as_deref(), Some(expected), "for {input:?}");
    }

    /// Refused, with no reason given: it is not known for certain.
    #[track_caller]
    fn silent(input: &str) {
        assert_eq!(show(input), None, "should not answer {input:?}");
        assert_eq!(why_not(input), None, "should give no reason for {input:?}");
    }

    #[test]
    fn says_why_unlike_quantities_cannot_be_combined() {
        why("5V + 3mA", "A voltage and a current cannot be added.");
        why("5V + 3", "A voltage and a plain number cannot be added.");
        why("12V - 2A", "A current cannot be subtracted from a voltage.");
        why("10kohm || 100nF", "A resistance and a capacitance cannot be put in parallel.");
        why("1500 rpm + 50 Hz", "A rotation speed and a frequency cannot be added.");
        why("10 Nm + 5 V", "An energy or torque and a voltage cannot be added.");
        // The reason is the first thing that went wrong, wherever it is.
        why("2 * (5V + 3mA) / 4", "A voltage and a current cannot be added.");
        why("VGS = 5V + 3mA", "A voltage and a current cannot be added.");
        // Units with no everyday name are not named.
        why("5 V/s + 3 A", "Those are different kinds of quantity, so they cannot be added.");
    }

    #[test]
    fn says_why_a_temperature_was_refused() {
        why("25 C + 30 C", "Two temperatures cannot be added. One less the other is a difference.");
        why("25 C + 30", "A temperature and a plain number cannot be added.");
        why("2 * 25 C", "A temperature cannot be multiplied or divided. A difference in K can.");
        why("25 C / 2", "A temperature cannot be multiplied or divided. A difference in K can.");
        why("(25 C)^2", "A temperature cannot be multiplied or divided. A difference in K can.");
        why("-300 C", "That is below absolute zero.");
        why("25 C to V", "A temperature cannot be converted to a voltage.");
        // What is allowed still is.
        check("25 C + 2 W * 50 C/W", "125.0 °C");
        check("150 C - 25 C", "125.0 Δ°C");
    }

    #[test]
    fn says_why_a_function_or_power_was_refused() {
        why("ln(5V)", "ln needs a plain number. Divide by a unit first.");
        why("exp(2 s)", "exp needs a plain number. Divide by a unit first.");
        why("sin(3 V)", "sin needs an angle or a plain number.");
        why("asin(2)", "asin needs a number from −1 to 1.");
        why("ln(0)", "ln needs a number above zero.");
        why("log(-1)", "log needs a number above zero.");
        why("sqrt(-4)", "A negative number has no real square root.");
        why("√(-4)", "A negative number has no real square root.");
        why("2 ^ 3V", "A power must be a plain number.");
        why("1/0", "That divides by zero.");
        why("5V / (2 - 2)", "That divides by zero.");
        check("sin(30 deg)", "0.5000");
        check("sqrt(4V * 1V)", "2.000 V");
    }

    #[test]
    fn says_why_a_conversion_was_refused() {
        why("3.5 bar to V", "A pressure cannot be converted to a voltage.");
        why("25 mm in kg", "A length cannot be converted to a mass.");
        why("(3+4j) ohm to V", "A resistance cannot be converted to a voltage.");
        why(
            "1/(20 ms) to rpm",
            "A per-second value could be cycles or radians. Type it in Hz to convert it to a rotation speed.",
        );
        why("10 Nm * 1500 rpm to V", "That cannot be converted to the unit asked for.");
        check("50 Hz to rpm", "3000 rpm");
        check("10 Nm * 1500 rpm to hp", "2.106 hp");
    }

    #[test]
    fn says_what_is_missing_or_unclear() {
        why("4.7k *", "The expression is not finished.");
        why("(2 + 3", "A bracket is not closed.");
        why("sin(30 deg", "A bracket is not closed.");
        why("2 + 3)", "A closing bracket has no opening one.");
        why("1/2pi", "What follows the division could be above or below the line. Add brackets, as in 1/(2*pi).");
        why("230 V @ 120", "The angle needs a unit: deg or rad.");
        why("2m + 3m", "A lone m could be milli or metres here. Give it a unit, as in mA.");
        why("V = 2.2mA * 4.7k", "4.7k has a prefix but no unit.");
        why(
            "47 in dB",
            "A plain number in dB could be a power or a voltage ratio. Give both sides units, as in 2V / 1V to dB.",
        );
    }

    #[test]
    fn gives_no_reason_unless_it_is_certain() {
        // Words are a problem for a model, not a slip in the maths, even
        // where part of the input would have a reason.
        silent("what is 5V + 3mA");
        silent("cutoff of an RC lowpass, R = 4.7k, C = 100nF");
        silent("tau for 1M and 22uF");
        silent("re = 25mV / Ic");
        silent("5 furlongs");
        // Refused on purpose, but not for a reason this covers.
        silent("3 + 4i");
        silent("5 dBv in V");
        silent("10 dBm + 3 dBm");
        silent("(-8)^(1/3)");
        silent("5V + 10%");
        // The two ways of reading it fail differently, so neither is given.
        silent("5 m + 3 V");
        silent("10 K - 25 C");
        silent("100 C - 400 K");
        silent("");
    }

    #[test]
    fn gives_no_reason_for_input_that_is_answered() {
        for input in [
            "2.2mA * 4.7kohm",
            "1/(2*pi*4.7kohm*100nF)",
            "10k || 22k || 47k",
            "4k7 + 330",
            "10 Nm * 1500 rpm to hp",
            "100 C to F",
            "(150 C - 25 C) / 62 C/W",
            "250mW in dBm",
            "2V / 1V to dB",
            "10kohm + 1/(j*2*pi*1kHz*100nF)",
            "230 V @ 0° - 230 V @ 120°",
            "VGS = -0.3V * (1 - sqrt(0.1mA / 0.2mA))",
            "2 W * 40 K/W",
            "25 mm to in",
        ] {
            assert!(evaluate(input).is_some(), "should answer {input:?}");
            assert_eq!(why_not(input), None, "should give no reason for {input:?}");
        }
    }

    #[test]
    fn a_formula_with_its_values_after_it() {
        check("re = 25mV / Ic, Ic = 2mA", "12.50 Ω");
        check("gain = Rd * gm, Rd = 10kohm, gm = 2.1mS", "21.00");
        check("fc = 1/(2*pi*R*C), R = 4.7kohm, C = 100nF", "338.6 Hz");
        check("Vth = Vcc * R2 / (R1 + R2), Vcc = 9V, R1 = 100kohm, R2 = 22kohm", "1.623 V");
        // No name in front, and a unit asked for.
        check("Rd * gm, Rd = 10kohm, gm = 2.1mS", "21.00");
        check("P = V^2 / R to mW, V = 5V, R = 1kohm", "25.00 mW");
        // A value may use other values, given in any order.
        check("Ic = beta * Ib, Ib = Vth / Rth, Vth = 1V, Rth = 20kohm, beta = 200", "10.00 mA");
        // A value need not be something that could be shown by itself: a
        // bare number, or a unit with no name of its own.
        check("Id = 0.5 * k * (Vgs - Vth)^2, k = 2 mA/V^2, Vgs = 2.5V, Vth = 1V", "2.250 mA");
        // Underscores, functions, complex values and temperatures all work.
        check("V_ov = abs(V_off) * sqrt(I_D / I_DSS), V_off = -2.5V, I_D = 1.2mA, I_DSS = 6mA", "1.118 V");
        check_complex(
            "Z = R + 1/(j*w*C), R = 10kohm, C = 100nF, w = 2*pi*1kHz",
            "10.13 kΩ ∠ −9.043°",
            "10.00 − j1.592 kΩ",
        );
        check("T = Ta + P * Rth, Ta = 25 C, P = 2 W, Rth = 50 C/W", "125.0 °C");
        // It works out the formula as typed, right or wrong.
        check("P = V / R, V = 12V, R = 8ohm", "1.500 A");
        check("Vc = Vcc - Ic * Rc, Vcc = 9V, Ic = 5mA, Rc = 4.7kohm", "−14.50 V");
    }

    #[test]
    fn a_value_may_have_the_name_of_a_unit() {
        // Alone, a word the user gave a value to is that value. Straight
        // after a number it is a unit, as it always is.
        check("P = V^2 / R, V = 12V, R = 8ohm", "18.00 W");
        check("F = m * a, m = 5 kg, a = 9.81 m/s^2", "49.05 N");
        check("X = 2*pi*f*L, f = 1kHz, L = 100mH", "628.3 Ω");
        check("tau = R * C, R = 1Mohm, C = 22uF", "22.00 s");
        check("KE = 0.5 * m * v^2, m = 2 kg, v = 3 m/s", "9.000 N·m");
        // The units of the values decide how the answer is shown, as they
        // would if the values were typed into the formula.
        check("E = 0.5 * C * V^2, C = 100uF, V = 12V", "7.200 mJ");
        check("0.5 * 100uF * (12V)^2", "7.200 mJ");
        check("f = 1 / T, T = 20 ms", "50.00 Hz");
        check("p2 = p1 * 2, p1 = 3 bar", "6.000 bar");
        // In the run of units behind a number, a name that is also a unit
        // could be either, so it is refused.
        why(
            "f = sqrt(k/m)/(2*pi), k = 100 N/m, m = 2 kg",
            "m could be a unit or the value you named m. Give the value another name, such as m_1.",
        );
        check("f = sqrt(k/m_1)/(2*pi), k = 100 N/m, m_1 = 2 kg", "1.125 Hz");
    }

    #[test]
    fn values_after_a_formula_are_held_to_the_same_rules() {
        // A prefix with no unit is refused, as for any named answer.
        why("gain = Rd * gm, Rd = 10k, gm = 2.1mS", "10k has a prefix but no unit.");
        why("V = I * R, I = 2.2mA, R = 4.7k", "4.7k has a prefix but no unit.");
        // Every name used must be given, once, and every value given used.
        why("re = 25mV / Ic, Ib = 2mA", "Ic is not given a value.");
        why("re = 25mV / Ic, Ic = 2mA, Ic = 3mA", "Ic is given a value twice.");
        why("re = 25mV / Ic, Ic = 2mA, Rc = 4.7kohm", "Rc is given a value but not used.");
        // `2V` is two volts, so the V that was given goes unused.
        why("Vpp = 2V, V = 5V", "V is given a value but not used.");
        check("Vpp = 2*V, V = 5V", "10.00 V");
        why("fc = 2πRC, R = 4.7kohm, C = 100nF", "πRC is read as one name. Write it as π*R*C.");
        why("tau = RC, R = 1Mohm, C = 22uF", "RC is read as one name. Write it as R*C.");
        // The reasons for plain maths apply inside it.
        why("P = V + I, V = 12V, I = 2A", "A voltage and a current cannot be added.");
        why("a = b / c, b = 5V, c = 0 ohm", "That divides by zero.");
        // Words with values after them are a problem for a model.
        silent("cutoff of an RC lowpass, R = 4.7kohm, C = 100nF");
        silent("voltage divider, 9V in, R1 = 10k on top, R2 = 4.7k to ground. vout?");
        // Names that mean something else, values in a ring, and commas that
        // are not between values.
        why("y = e * 2, e = 3", "e already means something here, so it cannot name a value.");
        why("y = sin * 2, sin = 3", "sin already means something here, so it cannot name a value.");
        silent("a = b * 2, b = c, c = b");
        silent("2, 3");
        silent("1,000 + 2");
        silent("V = 4,7 * 2");
    }

    #[test]
    fn solving_for_the_value_that_was_not_given() {
        check("12V = I * 8ohm", "1.500 A");
        check("V = I * R, V = 12V, R = 8ohm", "1.500 A");
        check("V = I * R, V = 12V, I = 1.5A", "8.000 Ω");
        check("fc = 1/(2*pi*R*C), fc = 10 Hz, R = 1Mohm", "15.92 nF");
        check("Vth = Vcc * R2 / (R1 + R2), Vth = 1.623V, Vcc = 9V, R1 = 100kohm", "22.00 kΩ");
        check("Rt = R1 || R2, Rt = 5kohm, R1 = 10kohm", "10.00 kΩ");
        check("5V - I * 1kohm = 0.65V", "4.350 mA");
        check("tau = R * C, tau = 22 s, R = 1Mohm", "22.00 µF");
        check("f = 1/T, f = 50 Hz", "20.00 ms");
        check("T = Ta + P * Rth, T = 125 C, Ta = 25 C, Rth = 50 C/W", "2.000 W");
        // The value to find may be inside a value that was given.
        check("Vout = Vin * k, k = R2/(R1+R2), Vout = 3V, Vin = 9V, R1 = 10kohm", "5.000 kΩ");
        // Equations that cannot be rearranged.
        check("2 = exp(y)", "0.6931");
        check("5V - I * 1kohm = 25.85mV * ln(I / 1e-14 A + 1)", "4.308 mA");
        check("0 = y * 5", "0");
        // The answer is marked as coming from a formula with its values.
        assert!(evaluate("12V = I * 8ohm").unwrap().given);
        assert!(!evaluate("12V / 8ohm").unwrap().given);
    }

    #[test]
    fn a_unit_letter_may_be_the_value_to_find() {
        // Standing alone, where the equation makes sense with it as a
        // value and with no other.
        check("5 N = m * 9.81 m/s^2", "509.7 g");
        check("20 = 20*log(A)", "10.00");
        check("12 V = (2 + 1) A * R", "4.000 Ω");
        // In a run of units behind a number, but only if the equation
        // does not hold together as it is written.
        check("10 Hz = 1/(2*pi*1Mohm*C)", "15.92 nF");
        // This one does, so there is nothing to find, though a value for
        // `s` would fit it.
        why("10 N = 2 kg * 5 m/s^2", "Every value is given, so there is nothing to solve for.");
        why("2 + 2 = 4", "Every value is given, so there is nothing to solve for.");
    }

    #[test]
    fn an_equation_with_more_than_one_answer() {
        // The same size either side of zero: the positive one, with the
        // other on the line beneath.
        let answer = evaluate("P = V^2 / R, P = 18 W, R = 8 ohm").unwrap();
        assert_eq!((answer.number.as_str(), answer.unit.as_str()), ("12.00", "V"));
        assert_eq!(answer.parts.as_deref(), Some("or −12.00 V"));
        assert_eq!(answer.copy, "12");
        let answer = evaluate("P = I^2 * R, P = 2 W, R = 8 ohm").unwrap();
        assert_eq!((answer.number.as_str(), answer.parts.as_deref()), ("500.0", Some("or −500.0 mA")));
        // Anything else is refused, with the values named.
        why(
            "Id = Idss * (1 - Vgs/Vp)^2, Id = 2 mA, Idss = 8 mA, Vp = -3V",
            "More than one value of Vgs fits: −4.500 V and −1.500 V.",
        );
        why("y^2 - 5*y + 6 = 0", "More than one value of y fits: 2.000 and 3.000.");
        // Two answers close together, one of them met exactly.
        why("y^2 - 2.1*y + 1.1 = 0", "More than one value of y fits: 1.000 and 1.100.");
        why("y^2 = -4", "No value of y was found that makes the two sides equal.");
        // Where the sides touch without crossing, the answer is not found.
        why("(y - 2)^2 = 0", "No value of y was found that makes the two sides equal.");
    }

    #[test]
    fn equations_that_are_refused() {
        why("V = I * R, V = 12V, I = 1.5A, R = 8ohm", "Every value is given, so there is nothing to solve for.");
        why("V = I * R, V = 12V", "Two values are not given: I and R.");
        why("12V = I * 4.7k", "4.7k has a prefix but no unit.");
        why("5V = I + 3ohm", "No kind of quantity for I makes the two sides agree.");
        why("2 = exp(x)", "x is the times sign here, so it cannot name a value. Use another letter.");
        why(
            "X = 2*pi*f*L, X = 628.3 ohm, L = 100mH",
            "X is the times sign here, so it cannot name a value. Use another letter.",
        );
        // The value could be a plain number or an angle.
        silent("0.5 = sin(y)");
        // Words.
        silent("voltage divider 9V in R1 = 10k");
        // A name in front that was given no value is still only a name,
        // so a formula is read as it was.
        check("VGS = -0.3V * 2", "−600.0 mV");
        check("V = 12V", "12.00 V");
        why("re = 25mV / Ic, Ib = 2mA", "Ic is not given a value.");
        why("Vth = Vcc * R2 / (R1 + R2), Vcc = 9V, R1 = 100kohm", "R2 is not given a value.");
    }

    #[test]
    fn programs_are_unchanged_by_values_after_a_formula() {
        // A step of a program is one formula. A comma in it is an error.
        assert!(run(&[expression("r_e", "25mV / I_c, I_c = 2mA")]).is_err());
        assert_eq!(run(&[expression("I_c", "2mA"), expression("r_e", "25mV / I_c")]).unwrap()[1], "12.50 Ω");
    }

    #[test]
    fn mechanical_units() {
        check("6 bar * pi * (25mm)^2", "1.178 kN");
        check("5 kg * 9.81 m/s^2", "49.05 N");
        check("50 mm * 20 mm", "10.00 cm²");
        check("5 mm²", "5.000 mm²");
        check("5 mm2 * 2", "10.00 mm²");
        check("0.5 in + 3 mm", "15.70 mm");
        check("10 N * 5 m", "50.00 N·m");
        check("12 L / 3 min", "4.000 L/min");
        check("100 km / 2 h", "13.89 m/s");
        check("2 kN / 4 cm²", "5.000 MPa");
        check("5 kW * 2 h", "36.00 MJ");
        check("250 g * 4", "1.000 kg");
    }

    #[test]
    fn answers_in_the_unit_typed() {
        check("3 bar * 2", "6.000 bar");
        check("2 in * 3", "6.000 in");
        check("1500 rpm / 3", "500.0 rpm");
        check("5 hp * 2", "10.00 hp");
        check("30 min * 2", "60.00 min");
        check("5 cm + 5 cm", "10.00 cm");
        // Mixed units of the same kind fall back to SI.
        check("100 psi + 1 bar", "789.5 kPa");
        check("90 min + 1 h", "9000 s");
    }

    #[test]
    fn rotation_and_angles() {
        check("10 N·m * 1500 rpm", "1.571 kW");
        check("1500 rpm to rad/s", "157.1 rad/s");
        check("90 deg to rad", "1.571 rad");
        check("90° to rad", "1.571 rad");
        check("1 rev to deg", "360.0 deg");
        check("sin(30 deg)", "0.5000");
        check("sin(pi/6)", "0.5000");
        // rpm is an angular speed and Hz is not; they differ by 2π. The two
        // are never mixed in a sum.
        refuse("1500 rpm + 25 Hz");
    }

    #[test]
    fn rotation_and_frequency() {
        // Asked for with `to`, one revolution is one cycle.
        check("1500 rpm to Hz", "25.00 Hz");
        check("90000 rpm to kHz", "1.500 kHz");
        check("25 rev/s to Hz", "25.00 Hz");
        check("314.159 rad/s to Hz", "50.00 Hz");
        check("50 Hz to rpm", "3000 rpm");
        check("2 * 50 Hz to rpm", "6000 rpm");
        check("1.5 kHz to rpm", "90000 rpm");
        check("50 Hz to rad/s", "314.2 rad/s");
        check("n = 1500 rpm to Hz", "25.00 Hz");
        // Hertz count cycles. A bare 1/s may as well be radians a second,
        // as 1/(R·C) is, so it is not turned into a rotation.
        refuse("1/(20 ms) to rpm");
        refuse("1/(10kohm*100nF) to rad/s");
        refuse("1/(10kohm*100nF) to rpm");
        // The radian is dimensionless, so only the angle drops out.
        check("1500 rpm to s^-1", "157.1 s⁻¹");
        check("10 Nm * 1500 rpm to W", "1571 W");
        check("10 Nm * 1500 rpm to hp", "2.106 hp");
        check("1 kW / 1500 rpm to Nm", "6.366 N·m");
        // Still unlike things.
        refuse("1500 rpm to m");
        refuse("50 Hz to deg");
        refuse("90 deg to Hz");
    }

    #[test]
    fn a_bare_per_second_is_not_called_hertz() {
        // Known to count cycles: a π in the formula, a time, or Hz typed.
        check("1/(2*pi*10kohm*100nF)", "159.2 Hz");
        check("1/(2π√(2.5H*470pF))", "4.643 kHz");
        check("1/(20 ms)", "50.00 Hz");
        check("2 * 50 Hz", "100.0 Hz");
        // Not known. 1/(R·C) is radians a second: 2π times the cutoff.
        check("1/(10kohm*100nF)", "1000 s⁻¹");
        check("10 ohm / 2 mH", "5000 s⁻¹");
        check("1/sqrt(2.5H*470pF)", "29170 s⁻¹");
        check("fc = 1/(10kohm*100nF)", "1000 s⁻¹");
        // Asked for in hertz, it is taken at the user's word.
        check("1.44/((1kohm + 2*10kohm)*100nF) to Hz", "685.7 Hz");
        // A program shows what it did before.
        assert_eq!(run(&[expression("w_c", "1/(10kohm*100nF)")]).unwrap(), ["1.000 kHz"]);
    }

    #[test]
    fn power_over_rotation_is_a_torque() {
        check("1 kW / 1500 rpm", "6.366 N·m");
        check("1 hp / 3000 rpm", "2.374 N·m");
        check("230 V * 10 A / 1500 rpm", "14.64 N·m");
        // Torque times an angle is still work, labelled as before.
        check("10 Nm * 2 rad", "20.00 N·m");
        check("10 Nm * 1500 rpm", "1.571 kW");
    }

    #[test]
    fn programs_do_not_convert_between_rotation_and_frequency() {
        // Claude is told to divide by 60 instead, and its instructions
        // have not changed.
        let to = |text, unit| run(&[Step { name: "n_s", kind: StepKind::Expression(text), to: Some(unit) }]);
        assert!(to("1500 rpm", "Hz").is_err());
        assert!(to("50 Hz", "rpm").is_err());
        assert!(to("10 Nm * 1500 rpm", "W").is_err());
        assert_eq!(to("1500 rpm", "rad/s").unwrap(), ["157.1 rad/s"]);
    }

    #[test]
    fn a_lone_m_is_milli_or_metres() {
        // Electrical shorthand around it: milli.
        check("2.2m*4.7k", "10.34");
        check("5V*100m", "500.0 mV");
        // Mechanical units around it, or a length asked for: metres.
        check("10 N * 5 m", "50.00 N·m");
        check("5 m to ft", "16.40 ft");
        check("5 m/s to km/h", "18.00 km/h");
        // Nothing to go on: no answer.
        refuse("100m*5");
        refuse("5m*3m");
        refuse("1m*1m");
    }

    #[test]
    fn rounding_and_edges() {
        check("999.96*1", "1.000 k");
        check("1000*1", "1.000 k");
        check("5-5", "0");
        check("3/4", "0.7500");
        check("1/20", "0.05000");
        check("1/400", "0.002500");
        check("1/2000", "500.0 µ");
        check("1f*1f", "1.000e-30");
        check("999.96 psi * 1", "1000 psi");
        check("0.0005 in * 1", "5.000e-4 in");
    }

    #[test]
    fn what_copy_gives() {
        // With an automatic prefix: the full value in the unprefixed unit.
        assert_eq!(copied("1/(2*pi*4.7k*100n)").as_deref(), Some("338.6"));
        assert_eq!(copied("4.7k*100n").as_deref(), Some("0.00047"));
        assert_eq!(copied("0.047uF").as_deref(), Some("4.7e-8"));
        assert_eq!(copied("10k||22k||47k").as_deref(), Some("5998"));
        assert_eq!(copied("-2^2").as_deref(), Some("-4"));
        // In a unit that was asked for or typed: the number as shown.
        assert_eq!(copied("1 in to mm").as_deref(), Some("25.4"));
        assert_eq!(copied("3.5 bar to psi").as_deref(), Some("50.76"));
        assert_eq!(copied("3 bar * 2").as_deref(), Some("6"));
        assert_eq!(copied("100 C to F").as_deref(), Some("212"));
    }

    fn expression<'a>(name: &'a str, text: &'a str) -> Step<'a> {
        Step { name, kind: StepKind::Expression(text), to: None }
    }

    /// Runs a program and returns each step as the window would show it.
    fn run(steps: &[Step]) -> Result<Vec<String>, ProgramError> {
        run_program(steps).map(|all| {
            all.iter()
                .map(|c| format!("{} {}", c.answer.number, c.answer.unit).trim_end().to_string())
                .collect()
        })
    }

    #[test]
    fn programs_use_earlier_steps() {
        // The BJT voltage-divider bias problem from the reference set.
        let shown = run(&[
            expression("V_th", "9V * 22kohm / (100kohm + 22kohm)"),
            expression("R_th", "100kohm || 22kohm"),
            expression("beta_f", "200"),
            expression("I_b", "(V_th - 0.65V) / (R_th + (beta_f + 1) * 1kohm)"),
            expression("I_c", "beta_f * I_b"),
            expression("V_c", "9V - I_c * 4.7kohm"),
        ])
        .unwrap();
        assert_eq!(shown, ["1.623 V", "18.03 kΩ", "200.0", "4.442 µA", "888.4 µA", "4.824 V"]);
    }

    #[test]
    fn programs_can_solve_an_equation() {
        // 5 V source, 1 kΩ, and a diode with Is = 1e-14 A at 300 K.
        let steps = [
            expression("V_t", "0.025852V"),
            expression("I_s", "1e-14A"),
            Step {
                name: "I_d",
                kind: StepKind::Solve {
                    equation: "5V - I_d * 1kohm = V_t * ln(I_d / I_s + 1)",
                    guess: "4mA",
                },
                to: None,
            },
            expression("V_d", "V_t * ln(I_d / I_s + 1)"),
        ];
        assert_eq!(run(&steps).unwrap()[2..], ["4.307 mA", "692.5 mV"]);

        // The same equation written as something that should come to zero.
        let zero_form = [
            expression("V_t", "0.025852V"),
            Step {
                name: "I_d",
                kind: StepKind::Solve {
                    equation: "5V - I_d * 1kohm - V_t * ln(I_d / 1e-14A + 1)",
                    guess: "1mA",
                },
                to: None,
            },
        ];
        assert_eq!(run(&zero_form).unwrap()[1], "4.307 mA");
    }

    #[test]
    fn programs_can_ask_for_a_unit() {
        let steps = [
            expression("A_p", "pi * (25mm)^2"),
            Step { name: "F_p", kind: StepKind::Expression("6 bar * A_p"), to: Some("lbf") },
        ];
        assert_eq!(run(&steps).unwrap(), ["19.63 cm²", "264.8 lbf"]);
    }

    #[test]
    fn programs_report_where_they_fail() {
        let fail = |steps: &[Step]| run(steps).unwrap_err();
        assert_eq!(fail(&[expression("sin", "9V")]).reason, "the name cannot be used");
        assert_eq!(fail(&[expression("2nd", "9V")]).reason, "the name cannot be used");
        assert_eq!(
            fail(&[expression("V_a", "9V"), expression("V_a", "1V")]),
            ProgramError { step: 1, reason: "the name is already used" }
        );
        // Volts plus amps and an unknown name are refused.
        assert_eq!(fail(&[expression("V_a", "9V"), expression("V_b", "V_a + 3mA")]).step, 1);
        assert_eq!(fail(&[expression("V_b", "V_missing * 2")]).step, 0);
    }

    #[test]
    fn programs_have_no_temperatures_or_decibels() {
        // These are for typed input. The language Claude writes its
        // formulas in is as it was, and so are its instructions.
        assert!(run(&[expression("T_j", "25 C + 10 kelvin")]).is_err());
        assert!(run(&[expression("T_j", "25 degC")]).is_err());
        assert!(run(&[expression("R_th", "50 kelvin / 2 W")]).is_err());
        assert!(run(&[expression("P_o", "10 dBm")]).is_err());
        assert!(run(&[expression("P_o", "10 dBm + 3 dB")]).is_err());
    }

    #[test]
    fn in_a_program_a_lone_m_is_metres() {
        let steps = [expression("F_g", "5 kg * 9.81 m/s^2"), expression("L_a", "2.5 m")];
        assert_eq!(run(&steps).unwrap(), ["49.05 N", "2.500 m"]);
    }

    #[test]
    fn names_need_no_underscore() {
        let steps = [
            expression("tau", "1Mohm * 22uF"),
            expression("R1", "10kohm"),
            expression("k", "2mA / 1V^2"),
            // `5V` is still five volts in a program with a step called V.
            expression("V", "5V"),
            expression("Id", "0.5 * k * (V - 3.5V)^2"),
            expression("twice", "2 * tau + R1 * 1uF"),
        ];
        assert_eq!(
            run(&steps).unwrap(),
            ["22.00 s", "10.00 kΩ", "0.002000 (SI)", "5.000 V", "2.250 mA", "44.01 s"]
        );
    }

    #[test]
    fn names_do_not_leak_into_typed_input() {
        refuse("V_th * 2");
        refuse("V_ov = V_th * 2");
    }

    #[test]
    fn a_name_in_front_of_plain_maths_is_a_label() {
        // The problem that failed in the installed 0.6.0.
        check("VGS = −0.3V × (1 − √(0.1mA / 0.2mA))", "−87.87 mV");
        check("V_ov = 2.5V * 2", "5.000 V");
        check("VGS(off) = 2 * 1.5V", "3.000 V");
        check("R1 = 10kΩ || 10kΩ", "5.000 kΩ");
        check("x = 3 * 4", "12.00");
        check("  fc=1/(2*pi*4.7kohm*100nF) =", "338.6 Hz");
        check("d = 5 m * 2 * 3 mm / 1 mm", "30.00 m");

        // A value with a prefix and no unit, under a name, has a unit the
        // user left out. Read as typed, the first of these would be amps.
        refuse("V = 2.2mA * 4.7k");
        refuse("R1 = 10k || 10k");
        refuse("V = 9V * 4.7k / (10k + 4.7k)");
        refuse("t = 2.2m * 3");
        // A lone m that cannot be settled stays unsettled.
        refuse("d = 5 m * 2");

        // The rest must be plain maths by itself.
        refuse("V = I * R");
        refuse("V = 5");
        refuse("VGS =");
        refuse("VGS = ");
        // Anything but a single name on the left is an equation.
        for input in [
            "= 2*3",
            "2x = 10",
            "5V = 2*3",
            "2*3 = 6",
            "a = b = 2*3",
            "x == 2*3",
            "the answer = 2*3",
            "f(x, y) = 2*3",
            "VGS() = 2*3",
            "R = 4.7k, C = 100nF",
        ] {
            refuse(input);
        }
    }

    #[test]
    fn refuses_what_it_does_not_understand() {
        for input in [
            "",
            "4",
            "4.7",
            "2+",
            "2*",
            "1/(2*pi*4.7kohm*100nF)*",
            "4.7k *",
            "(2+3",
            "2 3",
            "5V+3mA",
            "1/2pi",
            "1/0",
            "V = 2.2mA * 4.7k",
            "fc = 2πRC, R = 4.7k, C = 100nF",
            "gain of the stage",
            "cutoff of an RC lowpass, R = 4.7k, C = 100nF",
            "10 dB",
            "3 apples",
            "1,000 * 2",
            "5V * 5V",
            "sin(2V)",
            "2^(1V)",
            "s",
            "mm",
            "N*m",
            "5 kg + 2 m",
            "to psi",
            "x",
        ] {
            refuse(input);
        }
    }

    /// `depth` brackets, signs or roots around a 2, then `* 3V`.
    fn nested_in(open: &str, close: &str, depth: usize) -> String {
        format!("{}2{} * 3V", open.repeat(depth), close.repeat(depth))
    }

    #[test]
    fn deep_nesting_is_refused_not_followed() {
        // Ordinary nesting is unaffected.
        check(&nested_in("(", ")", 20), "6.000 V");
        check("--2 * 3V", "6.000 V");
        // Deeper than anyone writes, and still within the typed-input limit.
        refuse(&nested_in("(", ")", 150));
        refuse(&nested_in("-", "", 300));
        refuse(&nested_in("√", "", 300));
        refuse(&format!("2{} * 3V", "^2".repeat(150)));
    }

    #[test]
    fn a_program_step_far_too_deep_or_long_fails_without_crashing() {
        let failed = |text: &str| {
            let steps = [Step { name: "V_a", kind: StepKind::Expression(text), to: None }];
            run_program(&steps).map(|_| ()).unwrap_err().reason
        };
        // Within the length limit, but nested far too deep.
        assert_eq!(failed(&nested_in("(", ")", 450)), "the expression could not be worked out");
        assert_eq!(failed(&nested_in("-", "", 900)), "the expression could not be worked out");
        // Too long to be a formula at all. This depth would use up the stack.
        assert_eq!(failed(&nested_in("(", ")", 50_000)), "the expression could not be worked out");
        let long = format!("1V{}", " + 1V".repeat(MAX_STEP_CHARS / 5));
        assert_eq!(failed(&long), "the expression could not be worked out");
    }

    #[test]
    fn solving_stops_when_its_tries_run_out() {
        let names = Names::new();
        let guess = Quantity::plain(1.0);
        let root = solve("R^2 = 4", "R", guess, &names, &std::cell::Cell::new(SOLVE_TRIES)).unwrap();
        assert!((root.value - 2.0).abs() < 1e-9);
        assert!(solve("R^2 = 4", "R", guess, &names, &std::cell::Cell::new(3)).is_none());
        assert!(solve("R^2 = 4", "R", guess, &names, &std::cell::Cell::new(0)).is_none());
    }
}
