//! Turns Claude's reply into something the window can show.
//!
//! Claude never works out a number. It replies with a small program: named
//! steps written as formulas (see `prompts/solver.md`). This module runs that
//! program through the local engine, so every number on screen was computed
//! here, and anything Claude got wrong in a formula shows up as a step that
//! will not evaluate instead of as a plausible wrong answer.

use serde::{Deserialize, Serialize};

use std::sync::Arc;

use crate::calc::{self, Step, StepKind};
use crate::claude_code::{ClaudeCode, Image, STOPPED};

/// The instructions given to Claude.
pub const INSTRUCTIONS: &str = include_str!("../prompts/solver.md");

// ---- what Claude sends ------------------------------------------------------

#[derive(Deserialize)]
struct Reply {
    state: String,
    #[serde(default)]
    needs: Option<String>,
    #[serde(default)]
    formula: Option<String>,
    #[serde(default)]
    assumptions: Vec<ReplyAssumption>,
    #[serde(default)]
    steps: Vec<ReplyStep>,
    #[serde(default)]
    result: Option<String>,
    #[serde(default)]
    also: Vec<String>,
    /// Several answers of equal standing, when the user asked for more than
    /// one thing.
    #[serde(default)]
    results: Vec<String>,
    /// Two values the user wants compared: the first against the second.
    #[serde(default)]
    compare: Vec<String>,
    #[serde(default)]
    flag: Option<ReplyFlag>,
}

#[derive(Deserialize)]
struct ReplyStep {
    name: String,
    #[serde(default)]
    expr: Option<String>,
    #[serde(default)]
    solve: Option<String>,
    #[serde(default)]
    guess: Option<String>,
    #[serde(default)]
    to: Option<String>,
    /// A unit to print after a plain number, such as `dB`.
    #[serde(default)]
    label: Option<String>,
    #[serde(default)]
    note: String,
}

/// Something Claude supplied that the user did not. Either plain words, or
/// words with the step that holds the assumed value, so the engine can
/// measure how much the answer depends on it.
#[derive(Deserialize)]
#[serde(untagged)]
enum ReplyAssumption {
    Text(String),
    Detailed {
        text: String,
        #[serde(default)]
        step: Option<String>,
        #[serde(default)]
        sensitive: bool,
    },
}

#[derive(Deserialize)]
struct ReplyFlag {
    reason: String,
    #[serde(default)]
    standard: Option<String>,
}

// ---- what the window gets ---------------------------------------------------

#[derive(Clone, Debug, Serialize)]
pub struct Value {
    pub name: String,
    pub number: String,
    pub unit: String,
    pub copy: String,
    /// In SI base units. Used to check answers against the reference set.
    pub si_value: f64,
}

#[derive(Debug, Serialize)]
pub struct Assumption {
    pub text: String,
    /// The answer depends heavily on this value.
    pub sensitive: bool,
    /// How far the answer moves, in percent, if this value is 10% off.
    /// Measured by the engine; absent when it could not be measured.
    pub moves: Option<u32>,
}

/// An assumption is marked sensitive when a 10% error in it moves the answer
/// by at least this many percent, or when Claude says its real-world spread
/// is wide (a transistor's gain, for example).
const SENSITIVE_AT: f64 = 15.0;

/// How far the answer moves, in percent, when the named step's value is made
/// 10% larger and the whole program is run again.
fn moved_by(steps: &[ReplyStep], name: &str, result_index: usize, base: f64) -> Option<f64> {
    let index = steps.iter().position(|step| step.name == name)?;
    let nudged = format!("({}) * 1.1", steps[index].expr.as_deref()?);

    let program = steps
        .iter()
        .enumerate()
        .map(|(i, step)| {
            let kind = match (&step.expr, &step.solve, &step.guess) {
                (Some(text), None, _) => StepKind::Expression(if i == index { &nudged } else { text }),
                (None, Some(equation), Some(guess)) => StepKind::Solve { equation, guess },
                _ => return None,
            };
            Some(Step { name: &step.name, kind, to: None })
        })
        .collect::<Option<Vec<Step>>>()?;

    let moved = calc::run_program(&program).ok()?.get(result_index)?.si_value;
    (base != 0.0 && moved.is_finite()).then(|| ((moved - base) / base * 100.0).abs())
}

#[derive(Debug, Serialize)]
pub struct ShownStep {
    pub name: String,
    /// The formula exactly as it was computed.
    pub text: String,
    pub number: String,
    pub unit: String,
    pub note: String,
    pub solved: bool,
}

#[derive(Debug, Serialize)]
pub struct Solved {
    /// `clean`, `assumed`, `flagged`, `blocked` or `failed`.
    pub state: &'static str,
    /// For `blocked`: what is needed. For `failed`: what went wrong.
    pub message: Option<String>,
    /// The main relationship used, in ordinary symbols.
    pub formula: Option<String>,
    pub assumptions: Vec<Assumption>,
    /// For `flagged`: what is wrong with the formula as typed.
    pub flag: Option<String>,
    /// How the answer is laid out: `single`, `list` (several answers of
    /// equal standing) or `compare` (two values and their difference).
    pub layout: &'static str,
    /// The main number. For `list` it is the first of `results`; for
    /// `compare` it is the difference.
    pub result: Option<Value>,
    /// The answers for `list`, or the two values for `compare`.
    pub results: Vec<Value>,
    /// For `compare`: how much larger the first is than the second, in
    /// percent of the second. Negative if it is smaller.
    pub percent: Option<f64>,
    pub also: Vec<Value>,
    /// For `flagged`: the result of the standard form.
    pub standard: Option<Value>,
    pub steps: Vec<ShownStep>,
    /// The first reply would not run, and this is the result of asking again.
    pub second_try: bool,
}

/// What to tell Claude when its reply could not be used, so that it can try
/// once more. `None` if the reply was fine.
pub fn correction_request(solved: &Solved) -> Option<String> {
    if solved.state != "failed" {
        return None;
    }
    let problem = solved.message.as_deref().unwrap_or("It could not be read.");
    Some(format!(
        "Your reply could not be used. {problem}\n\n\
         Reply again with the whole corrected JSON object and nothing else. Check that every \
         name used in a formula is defined by an earlier step, that the units balance, that \
         only the listed units and functions appear, that multiplication is written with *, \
         and that no step is named after a unit or a reserved word (x, e, pi, to, in, or a \
         function name)."
    ))
}

/// Asks Claude a problem and runs its reply through the engine. If the
/// working will not run, Claude is told what went wrong and asked once more;
/// the second reply stands, whether or not it runs.
pub fn ask_and_solve(claude: &Arc<ClaudeCode>, problem: &str, images: &[Image]) -> Result<Solved, String> {
    let (reply, mut conversation) = claude.begin(problem, images)?;
    let first = solve_reply(&reply);
    let Some(correction) = correction_request(&first) else { return Ok(first) };

    match conversation.follow_up(&correction) {
        Ok(reply) => {
            let mut second = solve_reply(&reply);
            second.second_try = true;
            Ok(second)
        }
        // The user stopped it: that is not an answer of any kind.
        Err(error) if error == STOPPED => Err(error),
        // The second try could not be made, so the first failure stands.
        Err(_) => Ok(first),
    }
}

impl Solved {
    fn without_answer(state: &'static str, message: String) -> Self {
        Self {
            state,
            message: Some(message),
            formula: None,
            assumptions: Vec::new(),
            flag: None,
            layout: "single",
            result: None,
            results: Vec::new(),
            percent: None,
            also: Vec::new(),
            standard: None,
            steps: Vec::new(),
            second_try: false,
        }
    }

    fn failed(message: impl Into<String>) -> Self {
        Self::without_answer("failed", message.into())
    }
}

/// The difference between two steps, and that difference as a percentage of
/// the second. Worked out by the engine from extra steps added to Claude's
/// working, so the units are checked like everything else. `None` if the
/// two cannot be subtracted, or the second is zero.
fn difference_between(steps: &[ReplyStep], first: &str, second: &str) -> Option<(Value, f64)> {
    let difference = format!("{first} - {second}");
    let percent = format!("({first} - {second}) / {second} * 100");

    let mut program = steps
        .iter()
        .map(|step| {
            let kind = match (&step.expr, &step.solve, &step.guess) {
                (Some(text), None, _) => StepKind::Expression(text),
                (None, Some(equation), Some(guess)) => StepKind::Solve { equation, guess },
                _ => return None,
            };
            Some(Step { name: &step.name, kind, to: None })
        })
        .collect::<Option<Vec<Step>>>()?;
    program.push(Step { name: "slipstick_difference", kind: StepKind::Expression(&difference), to: None });
    program.push(Step { name: "slipstick_percent", kind: StepKind::Expression(&percent), to: None });

    let computed = calc::run_program(&program).ok()?;
    let [.., difference, percent] = &computed[..] else { return None };
    let value = Value {
        name: "difference".into(),
        number: difference.answer.number.clone(),
        unit: difference.answer.unit.clone(),
        copy: difference.answer.copy.clone(),
        si_value: difference.si_value,
    };
    Some((value, percent.si_value))
}

/// Finds the JSON object in a reply, ignoring a code fence or stray words
/// around it.
fn json_in(text: &str) -> Option<&str> {
    let start = text.find('{')?;
    let end = text.rfind('}')?;
    (end > start).then(|| &text[start..=end])
}

/// Runs Claude's reply and returns what to show.
pub fn solve_reply(text: &str) -> Solved {
    let Some(reply) = json_in(text).and_then(|json| serde_json::from_str::<Reply>(json).ok()) else {
        return Solved::failed("Claude's reply could not be read.");
    };

    if reply.state == "blocked" {
        let needs = reply.needs.unwrap_or_else(|| "More information is needed.".into());
        return Solved::without_answer("blocked", needs);
    }
    if reply.steps.is_empty() {
        return Solved::failed("Claude's reply had no working in it.");
    }

    let mut program = Vec::new();
    for step in &reply.steps {
        let kind = match (&step.expr, &step.solve) {
            (Some(text), None) => StepKind::Expression(text),
            (None, Some(equation)) => {
                let Some(guess) = &step.guess else {
                    return Solved::failed(format!("{}: an equation needs a starting guess.", step.name));
                };
                StepKind::Solve { equation, guess }
            }
            _ => return Solved::failed(format!("{}: a step needs one formula.", step.name)),
        };
        program.push(Step { name: &step.name, kind, to: step.to.as_deref() });
    }

    let computed = match calc::run_program(&program) {
        Ok(computed) => computed,
        Err(error) => {
            let name = &reply.steps[error.step].name;
            return Solved::failed(format!("{name}: {}.", error.reason));
        }
    };

    let values: Vec<Value> = reply
        .steps
        .iter()
        .zip(&computed)
        .map(|(step, computed)| {
            let answer = &computed.answer;
            // A plain number can be given a label such as dB.
            let unit = match &step.label {
                Some(label) if answer.unit.is_empty() => label.clone(),
                _ => answer.unit.clone(),
            };
            Value {
                name: step.name.clone(),
                number: answer.number.clone(),
                unit,
                copy: answer.copy.clone(),
                si_value: computed.si_value,
            }
        })
        .collect();

    let find = |name: &str| values.iter().find(|value| value.name == name).cloned();

    // Several answers of equal standing, or two values to compare.
    let mut named = Vec::new();
    for name in if reply.compare.len() == 2 { &reply.compare } else { &reply.results } {
        match find(name) {
            Some(value) => named.push(value),
            None => return Solved::failed(format!("\"{name}\" is not one of the steps.")),
        }
    }
    let compared = (reply.compare.len() == 2)
        .then(|| difference_between(&reply.steps, &reply.compare[0], &reply.compare[1]))
        .flatten();

    let (layout, results, result, percent) = match compared {
        Some((difference, percent)) => ("compare", named, difference, Some(percent)),
        None if named.len() >= 2 => {
            let first = named[0].clone();
            ("list", named, first, None)
        }
        None => {
            let result = match reply.result.as_ref().or(reply.results.first()) {
                Some(name) => match find(name) {
                    Some(value) => value,
                    None => {
                        return Solved::failed(format!("The result \"{name}\" is not one of the steps."));
                    }
                },
                None => values.last().cloned().expect("steps is not empty"),
            };
            ("single", Vec::new(), result, None)
        }
    };
    let mut also = Vec::new();
    for name in &reply.also {
        match find(name) {
            Some(value) => also.push(value),
            None => return Solved::failed(format!("\"{name}\" is not one of the steps.")),
        }
    }
    let standard = match reply.flag.as_ref().and_then(|flag| flag.standard.as_deref()) {
        Some(name) => match find(name) {
            Some(value) => Some(value),
            None => return Solved::failed(format!("\"{name}\" is not one of the steps.")),
        },
        None => None,
    };

    let steps = reply
        .steps
        .iter()
        .zip(&values)
        .map(|(step, value)| ShownStep {
            name: step.name.clone(),
            text: step.expr.clone().or_else(|| step.solve.clone()).unwrap_or_default(),
            number: value.number.clone(),
            unit: value.unit.clone(),
            note: step.note.clone(),
            solved: step.solve.is_some(),
        })
        .collect();

    let result_index = reply.steps.iter().position(|step| step.name == result.name);
    let assumptions: Vec<Assumption> = reply
        .assumptions
        .iter()
        .map(|assumption| match assumption {
            ReplyAssumption::Text(text) => Assumption { text: text.clone(), sensitive: false, moves: None },
            ReplyAssumption::Detailed { text, step, sensitive } => {
                let moves = step.as_deref().zip(result_index).and_then(|(name, result_index)| {
                    moved_by(&reply.steps, name, result_index, result.si_value)
                });
                Assumption {
                    text: text.clone(),
                    sensitive: *sensitive || moves.is_some_and(|percent| percent >= SENSITIVE_AT),
                    moves: moves.map(|percent| percent.round() as u32),
                }
            }
        })
        .collect();

    let state = if reply.flag.is_some() {
        "flagged"
    } else if !assumptions.is_empty() {
        "assumed"
    } else {
        "clean"
    };

    Solved {
        state,
        message: None,
        formula: reply.formula,
        assumptions,
        flag: reply.flag.map(|flag| flag.reason),
        layout,
        result: Some(result),
        results,
        percent,
        also,
        standard,
        steps,
        second_try: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_clean_answer() {
        let solved = solve_reply(
            r#"```json
            {"state":"answer","formula":"f_c = 1/(2πRC)",
             "steps":[{"name":"f_c","expr":"1/(2*pi*4.7kohm*100nF)","note":"cutoff formula"}],
             "result":"f_c"}
            ```"#,
        );
        assert_eq!(solved.state, "clean");
        let result = solved.result.unwrap();
        assert_eq!((result.number.as_str(), result.unit.as_str()), ("338.6", "Hz"));
        assert_eq!(solved.steps[0].note, "cutoff formula");
    }

    #[test]
    fn an_assumption_and_a_label() {
        let solved = solve_reply(
            r#"{"state":"answer","assumptions":["300 K"],
                "steps":[{"name":"g_db","expr":"20*log(47)","label":"dB","note":"to decibels"}]}"#,
        );
        assert_eq!(solved.state, "assumed");
        let result = solved.result.unwrap();
        assert_eq!((result.number.as_str(), result.unit.as_str()), ("33.44", "dB"));
    }

    #[test]
    fn the_engine_measures_which_assumption_matters() {
        // The output stage from a user's schematic: the answer hangs on the
        // assumed gain, and hardly at all on the assumed base-emitter drop.
        let solved = solve_reply(
            r#"{"state":"answer","formula":"V_out = V_th - I_C·R_th",
                "assumptions":[{"text":"β = 100","step":"beta_f"},
                               {"text":"V_BE = 0.7 V","step":"V_be"},
                               "output unloaded"],
                "steps":[{"name":"beta_f","expr":"100","note":"assumed gain"},
                         {"name":"V_be","expr":"0.7V","note":"assumed drop"},
                         {"name":"I_b","expr":"(12V - V_be) / 1.5Mohm","note":"base current"},
                         {"name":"I_c","expr":"beta_f * I_b","note":"collector current"},
                         {"name":"V_th","expr":"12V * 4.7kohm / (10kohm + 4.7kohm)","note":"divider"},
                         {"name":"R_th","expr":"10kohm || 4.7kohm","note":"divider resistance"},
                         {"name":"V_out","expr":"V_th - I_c * R_th","note":"output"}],
                "result":"V_out"}"#,
        );
        assert_eq!(solved.state, "assumed");
        assert_eq!(solved.result.unwrap().number, "1.428");

        let [gain, drop, unloaded] = &solved.assumptions[..] else { panic!("three assumptions") };
        assert_eq!((gain.sensitive, gain.moves), (true, Some(17)));
        assert_eq!((drop.sensitive, drop.moves), (false, Some(1)));
        assert_eq!((unloaded.sensitive, unloaded.moves), (false, None));
    }

    #[test]
    fn claude_can_mark_an_assumption_sensitive_itself() {
        // Collector current is simply proportional to gain, which the 10%
        // test does not count as heavy, but a transistor's gain varies widely.
        let solved = solve_reply(
            r#"{"state":"answer",
                "assumptions":[{"text":"β = 100","step":"beta_f","sensitive":true}],
                "steps":[{"name":"beta_f","expr":"100","note":"assumed gain"},
                         {"name":"I_c","expr":"beta_f * 7.5uA","note":"collector current"}]}"#,
        );
        let gain = &solved.assumptions[0];
        assert_eq!((gain.sensitive, gain.moves), (true, Some(10)));
    }

    /// Two LEDs on 5 V, one through 220 Ω and one through 330 Ω.
    const TWO_LEDS: &str = r#""steps":[
        {"name":"I_D1","expr":"(5V - 2V) / 220ohm","note":"D1 current"},
        {"name":"I_D2","expr":"(5V - 2V) / 330ohm","note":"D2 current"}]"#;

    #[test]
    fn several_answers_are_listed() {
        let solved = solve_reply(&format!(r#"{{"state":"answer",{TWO_LEDS},"results":["I_D1","I_D2"]}}"#));
        assert_eq!(solved.layout, "list");
        let shown: Vec<String> = solved.results.iter().map(|v| format!("{} {} {}", v.name, v.number, v.unit)).collect();
        assert_eq!(shown, ["I_D1 13.64 mA", "I_D2 9.091 mA"]);
        // The first also serves as the main result.
        assert_eq!(solved.result.unwrap().number, "13.64");
    }

    #[test]
    fn a_comparison_gets_its_difference_from_the_engine() {
        let solved = solve_reply(&format!(r#"{{"state":"answer",{TWO_LEDS},"compare":["I_D1","I_D2"]}}"#));
        assert_eq!(solved.layout, "compare");
        assert_eq!(solved.results.len(), 2);
        let difference = solved.result.unwrap();
        assert_eq!((difference.number.as_str(), difference.unit.as_str()), ("4.545", "mA"));
        assert!((solved.percent.unwrap() - 50.0).abs() < 1e-9);

        // The other way round the difference is negative.
        let reversed = solve_reply(&format!(r#"{{"state":"answer",{TWO_LEDS},"compare":["I_D2","I_D1"]}}"#));
        assert_eq!(reversed.result.unwrap().number, "−4.545");
        assert!((reversed.percent.unwrap() + 100.0 / 3.0).abs() < 1e-9);
    }

    #[test]
    fn things_that_cannot_be_subtracted_are_listed_instead() {
        let solved = solve_reply(
            r#"{"state":"answer",
                "steps":[{"name":"V_a","expr":"5V","note":"a"},{"name":"I_a","expr":"2mA","note":"b"}],
                "compare":["V_a","I_a"]}"#,
        );
        assert_eq!(solved.layout, "list");
        assert_eq!(solved.results.len(), 2);
        assert_eq!(solved.percent, None);
    }

    #[test]
    fn a_flagged_formula_gives_both_results() {
        let solved = solve_reply(
            r#"{"state":"answer","formula":"f_c = 1/(2πRC)",
                "steps":[{"name":"t_typed","expr":"2*pi*4.7kohm*100nF","note":"as typed"},
                         {"name":"f_c","expr":"1/(2*pi*4.7kohm*100nF)","note":"usual form"}],
                "result":"t_typed",
                "flag":{"reason":"Seconds, not hertz.","standard":"f_c"}}"#,
        );
        assert_eq!(solved.state, "flagged");
        assert_eq!(solved.result.unwrap().unit, "ms");
        assert_eq!(solved.standard.unwrap().number, "338.6");
        assert_eq!(solved.flag.as_deref(), Some("Seconds, not hertz."));
    }

    #[test]
    fn only_a_failed_reply_is_sent_back() {
        let failed = solve_reply(r#"{"state":"answer","steps":[{"name":"x","expr":"5V","note":"given"}]}"#);
        let request = correction_request(&failed).unwrap();
        assert!(request.contains("x: the name cannot be used."), "{request}");
        assert!(request.contains("whole corrected JSON object"));

        let fine = solve_reply(r#"{"state":"answer","steps":[{"name":"V_a","expr":"5V","note":"given"}]}"#);
        assert_eq!(correction_request(&fine), None);
        // Blocked is Claude declining, not a mistake to correct.
        let blocked = solve_reply(r#"{"state":"blocked","needs":"Need the capacitor value."}"#);
        assert_eq!(correction_request(&blocked), None);
    }

    #[test]
    fn blocked_says_what_is_needed() {
        let solved = solve_reply(r#"{"state":"blocked","needs":"Need the capacitor value."}"#);
        assert_eq!(solved.state, "blocked");
        assert_eq!(solved.message.as_deref(), Some("Need the capacitor value."));
        assert!(solved.result.is_none());
    }

    #[test]
    fn a_bad_formula_fails_instead_of_showing_a_number() {
        let solved = solve_reply(
            r#"{"state":"answer","steps":[{"name":"V_x","expr":"5V + 3mA","note":"add"}]}"#,
        );
        assert_eq!(solved.state, "failed");
        assert_eq!(solved.message.as_deref(), Some("V_x: the expression could not be worked out."));
        assert!(solved.result.is_none());

        assert_eq!(solve_reply("I think the answer is 42.").state, "failed");
        assert_eq!(solve_reply(r#"{"state":"answer","steps":[]}"#).state, "failed");
    }
}
