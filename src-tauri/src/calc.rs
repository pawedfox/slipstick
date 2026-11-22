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

/// Exponents of: kilogram, metre, second, ampere, angle.
///
/// Angle is tracked as its own dimension so that `rpm` (an angular speed)
/// cannot be silently mixed up with `Hz`; the two differ by 2π.
type Dim = [i8; 5];
const NO_DIM: Dim = [0; 5];
const ANGLE: usize = 4;

const LENGTH: Dim = [0, 1, 0, 0, 0];
const AREA: Dim = [0, 2, 0, 0, 0];
const VOLUME: Dim = [0, 3, 0, 0, 0];
const MASS: Dim = [1, 0, 0, 0, 0];
const TIME: Dim = [0, 0, 1, 0, 0];
const FREQUENCY: Dim = [0, 0, -1, 0, 0];
const SPEED: Dim = [0, 1, -1, 0, 0];
const ACCELERATION: Dim = [0, 1, -2, 0, 0];
const FLOW: Dim = [0, 3, -1, 0, 0];
const FORCE: Dim = [1, 1, -2, 0, 0];
const PRESSURE: Dim = [1, -1, -2, 0, 0];
const ENERGY: Dim = [1, 2, -2, 0, 0];
const POWER: Dim = [1, 2, -3, 0, 0];
const CURRENT: Dim = [0, 0, 0, 1, 0];
const CHARGE: Dim = [0, 0, 1, 1, 0];
const VOLTAGE: Dim = [1, 2, -3, -1, 0];
const RESISTANCE: Dim = [1, 2, -3, -2, 0];
const CONDUCTANCE: Dim = [-1, -2, 3, 2, 0];
const CAPACITANCE: Dim = [-1, -2, 4, 2, 0];
const INDUCTANCE: Dim = [1, 2, -2, -2, 0];
const MAGNETIC_FLUX: Dim = [1, 2, -2, -1, 0];
const FLUX_DENSITY: Dim = [1, 0, -2, -1, 0];
const ANGLE_DIM: Dim = [0, 0, 0, 0, 1];
const ANGULAR_SPEED: Dim = [0, 0, -1, 0, 1];

#[derive(Clone, Copy, Debug)]
struct Quantity {
    /// In SI base units.
    value: f64,
    dim: Dim,
    /// Written with a `%`. Adding a percentage to a plain number is
    /// ambiguous (`200 + 15%`), so that combination is refused.
    percent: bool,
}

impl Quantity {
    fn plain(value: f64) -> Self {
        Self { value, dim: NO_DIM, percent: false }
    }

    fn of(value: f64, dim: Dim) -> Self {
        Self { value, dim, percent: false }
    }
}

/// What the window shows and what a copy puts on the clipboard.
#[derive(Debug, PartialEq, Serialize)]
pub struct Answer {
    /// The number, for example `338.6`.
    pub number: String,
    /// SI prefix plus unit symbol, for example `kΩ`. May be empty.
    pub unit: String,
    /// A plain number for pasting. See `SPEC.md` for which value this is.
    pub copy: String,
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
}

impl<'a> Parser<'a> {
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
        }
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
            if left.dim != right.dim || left.percent != right.percent {
                return None;
            }
            left.value += sign * right.value;
        }
    }

    /// `a || b`, the parallel combination. Binds tighter than `+`.
    fn parallel(&mut self) -> Option<Quantity> {
        let mut left = self.product()?;
        while self.take(Token::Parallel) {
            let right = self.product()?;
            if left.dim != right.dim || left.value + right.value == 0.0 {
                return None;
            }
            left = Quantity::of(left.value * right.value / (left.value + right.value), left.dim);
        }
        Some(left)
    }

    fn product(&mut self) -> Option<Quantity> {
        let mut left = self.unary()?;
        let mut after_division = false;
        loop {
            if self.take(Token::Star) {
                left = multiply(left, self.unary()?);
                after_division = false;
            } else if self.take(Token::Slash) {
                left = divide(left, self.unary()?)?;
                after_division = true;
            } else if matches!(
                self.peek(),
                Some(Token::Open | Token::Word(_) | Token::Name(_) | Token::Root)
            ) {
                // Implied multiplication, as in `2pi` or `2(3)`. After a
                // division it is ambiguous (`1/2pi`), so it is refused.
                if after_division {
                    return None;
                }
                self.interesting = true;
                left = multiply(left, self.power()?);
            } else {
                return Some(left);
            }
        }
    }

    fn unary(&mut self) -> Option<Quantity> {
        if self.take(Token::Minus) {
            let mut operand = self.unary()?;
            operand.value = -operand.value;
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
        while self.take(Token::Percent) {
            if base.dim != NO_DIM {
                return None;
            }
            base = Quantity { value: base.value / 100.0, dim: NO_DIM, percent: true };
        }
        if let Some(&Token::Super(n)) = self.peek() {
            self.pos += 1;
            self.interesting = true;
            base = raise(base, Quantity::plain(f64::from(n)))?;
        }
        if self.take(Token::Caret) {
            let exponent = self.unary()?;
            raise(base, exponent)
        } else {
            Some(base)
        }
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
        let token = self.peek()?.clone();
        self.pos += 1;
        match token {
            Token::Number(value) => {
                self.has_value = true;
                if let Some(Token::Word(word)) = self.peek().cloned()
                    && let Some((scale, dim, index)) = self.resolve(&word)
                {
                    self.pos += 1;
                    self.interesting = true;
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
                self.take(Token::Close).then_some(inner)
            }
            Token::Root => {
                self.interesting = true;
                let operand = self.primary()?;
                raise(operand, Quantity::plain(0.5))
            }
            Token::Word(word) => {
                self.interesting = true;
                match word.as_str() {
                    "pi" | "π" => {
                        self.has_value = true;
                        return Some(Quantity::plain(PI));
                    }
                    "e" => {
                        self.has_value = true;
                        return Some(Quantity::plain(std::f64::consts::E));
                    }
                    _ => {}
                }
                if is_function(&word) {
                    if !self.take(Token::Open) {
                        return None;
                    }
                    let argument = self.sum()?;
                    if !self.take(Token::Close) {
                        return None;
                    }
                    return apply(&word, argument);
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

fn multiply(a: Quantity, b: Quantity) -> Quantity {
    let mut dim = a.dim;
    for (d, other) in dim.iter_mut().zip(b.dim) {
        *d += other;
    }
    Quantity::of(a.value * b.value, dim)
}

fn divide(a: Quantity, b: Quantity) -> Option<Quantity> {
    if b.value == 0.0 {
        return None;
    }
    let mut dim = a.dim;
    for (d, other) in dim.iter_mut().zip(b.dim) {
        *d -= other;
    }
    Some(Quantity::of(a.value / b.value, dim))
}

/// The exponent must be a plain number. A quantity with units can only be
/// raised to a power that leaves whole-number dimensions (`V^2`, `√(H·F)`).
fn raise(base: Quantity, exponent: Quantity) -> Option<Quantity> {
    if exponent.dim != NO_DIM {
        return None;
    }
    let mut dim = NO_DIM;
    for (out, d) in dim.iter_mut().zip(base.dim) {
        let scaled = f64::from(d) * exponent.value;
        if (scaled - scaled.round()).abs() > 1e-9 || scaled.abs() > 100.0 {
            return None;
        }
        *out = scaled.round() as i8;
    }
    Some(Quantity::of(base.value.powf(exponent.value), dim))
}

fn is_function(name: &str) -> bool {
    matches!(
        name,
        "sqrt" | "abs" | "ln" | "log" | "exp" | "sin" | "cos" | "tan" | "asin" | "acos" | "atan"
    )
}

fn apply(name: &str, argument: Quantity) -> Option<Quantity> {
    match name {
        "sqrt" => return raise(argument, Quantity::plain(0.5)),
        "abs" => return Some(Quantity::of(argument.value.abs(), argument.dim)),
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
}

fn superscript(power: i32) -> String {
    match power {
        1 => String::new(),
        2 => "²".into(),
        3 => "³".into(),
        n => format!("^{n}"),
    }
}

fn parse_target(tokens: &[Token]) -> Option<Target> {
    let mut parser = Parser::new(tokens, BareM::Metre, &[]);
    let mut target =
        Target { factor: 1.0, dim: NO_DIM, label: String::new(), mechanical: false, electrical: false };
    let mut divide = false;

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

#[derive(Clone, Copy, PartialEq)]
enum Scale {
    Celsius,
    Fahrenheit,
    Kelvin,
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
    Answer { number, unit: unit.into(), copy: plain_copy(value) }
}

/// Engineering notation with an automatic SI prefix. Copying gives the full
/// value in the unprefixed unit.
fn shown_with_prefix(value: f64, unit: &str) -> Answer {
    let value = round_sig(value);
    if value == 0.0 {
        return Answer { number: "0".into(), unit: unit.into(), copy: "0".into() };
    }
    let magnitude = value.abs();
    let sign = sign_of(value);
    let copy = plain_copy(value);

    // A plain number of ordinary size reads best with no prefix at all.
    if unit.is_empty() && (0.1..1000.0).contains(&magnitude) {
        return Answer { number: format!("{sign}{}", fixed_sig(magnitude)), unit: String::new(), copy };
    }

    // The small tolerance keeps exact powers of ten (1000, 1e-6) from
    // landing just below their own boundary.
    let exponent = ((magnitude.log10() + 1e-12) / 3.0).floor() as i32 * 3;
    // A plain number in the thousandths would get a lone "m", which reads as
    // metres. Write it out instead: 0.05000, not 50.00 m.
    if unit.is_empty() && exponent == -3 {
        return Answer { number: format!("{sign}{}", fixed_sig(magnitude)), unit: String::new(), copy };
    }
    if !(-15..=12).contains(&exponent) {
        // Beyond the prefixes: fall back to scientific notation.
        return Answer {
            number: format!("{sign}{:.*e}", (sig_figs() - 1) as usize, magnitude),
            unit: unit.into(),
            copy,
        };
    }
    let scale = 10f64.powi(exponent);
    Answer {
        number: format!("{sign}{}", fixed_sig(round_sig(magnitude / scale))),
        unit: format!("{}{unit}", prefix_symbol(scale)),
        copy,
    }
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
        (without_angle != dim).then(|| named(without_angle)).flatten()
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
}

fn attempt(
    tokens: &[Token],
    split: Option<usize>,
    bare_m: BareM,
    names: &[(String, Quantity)],
) -> Option<Outcome> {
    // Typed input must be worth answering: more than a bare number. A step
    // of a program may be as simple as `200` or another step's name.
    let in_program = bare_m == BareM::Program;
    let (expression, target) = match split {
        Some(at) => (&tokens[..at], Some(parse_target(&tokens[at + 1..])?)),
        None => (tokens, None),
    };

    let mut parser = Parser::new(expression, bare_m, names);
    let result = parser.sum()?;
    if parser.pos != expression.len() || !parser.has_value || !result.value.is_finite() {
        return None;
    }
    if !in_program && !parser.interesting && target.is_none() {
        return None;
    }

    let answer = if let Some(target) = &target {
        if result.dim != target.dim {
            return None;
        }
        shown_as_is(result.value / target.factor, &target.label)
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
        quantity: result,
        saw_bare_m: parser.saw_bare_m,
        bare_prefix: parser.bare_prefix,
        electrical: parser.electrical || target.as_ref().is_some_and(|t| t.electrical),
        mechanical: parser.mechanical || target.as_ref().is_some_and(|t| t.mechanical),
    })
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
    let (labelled, text) = without_label(text);
    let tokens = lex(text, &[])?;
    if let Some(found) = temperature(&tokens) {
        return Some(found);
    }
    let split = conversion_split(&tokens);
    // Someone who names the answer is working with real quantities. A value
    // with a prefix and no unit (`V = 2.2mA * 4.7k`) then has a unit they
    // left out, and reading it as a plain number would give the answer the
    // wrong unit (amps, there). That needs understanding, so it is refused.
    let done = |outcome: Outcome| {
        (!(labelled && outcome.bare_prefix)).then_some((outcome.answer, outcome.quantity.value))
    };

    // A lone `m` after a number is milli in electrical shorthand (`2.2m`)
    // and metres in mechanical work (`5 m`). Try both readings.
    let milli = attempt(&tokens, split, BareM::Milli, &[]);
    let saw_bare_m = milli.as_ref().is_none_or(|outcome| outcome.saw_bare_m);
    if !saw_bare_m {
        return milli.and_then(done);
    }
    let metre = attempt(&tokens, split, BareM::Metre, &[]);

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

fn evaluate_step(text: &str, to: Option<&str>, names: &Names) -> Option<Outcome> {
    let mut tokens = lex(text, names)?;
    if let Some(unit) = to {
        tokens.push(Token::Word("to".into()));
        tokens.extend(lex(unit, &[])?);
    }
    let split = conversion_split(&tokens);
    attempt(&tokens, split, BareM::Program, names)
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
fn solve(equation: &str, name: &str, guess: Quantity, names: &Names) -> Option<Quantity> {
    let at = |x: f64| residual(equation, name, Quantity::of(x, guess.dim), names);

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
                let root = solve(equation, step.name, guess, &names)
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
        // rpm is an angular speed and Hz is not; they differ by 2π.
        refuse("1500 rpm to Hz");
        refuse("1500 rpm + 25 Hz");
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
}
