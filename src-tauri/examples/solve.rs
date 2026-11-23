//! Reads one of Claude's replies on standard input, runs it through the
//! engine, and prints what the window would show as JSON. Used by the
//! reference-set runner (`tests/reference/run.mjs`).
//!
//! With `--ask`, reads a problem instead, asks Claude through the installed
//! Claude Code, and prints the solved result and how long it took. The
//! problem is plain text, or JSON with a picture:
//! `{"input": "...", "images": [{"media_type": "image/png", "data": "<base64>"}]}`.
//!
//! With `--ask --force-second-try`, the first reply is treated as if a step
//! had failed, to test that asking again works. With `--ask --show-replies`,
//! Claude's own replies are printed too, to see why one could not be used.
//!
//! With `--instant`, reads typed input instead and prints the instant path's
//! answer, or `null` if it has none. With `--why`, reads typed input and
//! prints the reason the instant path gives for refusing it, or `null` if
//! it gives none. With `--keep`, reads typed input and prints what Enter
//! would keep for the session: `{"kept": …}`, `{"reason": "…"}`, or `null`
//! where the input is not one to keep. `--kept JSON` gives any of those
//! three the values already kept, as a list of what `--keep` printed. With
//! `--instructions`, prints the instructions given to Claude.
//!
//! `--figures N` shows answers to N significant figures (3 to 6), and
//! `--deeper` asks the slower model, as the two settings do in the app.

use std::io::Read;

use std::sync::Arc;

use slipstick_lib::{calc, claude_code, solver};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let figures = args
        .iter()
        .position(|arg| arg == "--figures")
        .and_then(|at| args.get(at + 1)?.parse().ok())
        .unwrap_or(calc::DEFAULT_SIG_FIGS);
    calc::with_sig_figs(figures, run);
}

fn run() {
    if std::env::args().any(|arg| arg == "--instructions") {
        print!("{}", solver::INSTRUCTIONS);
        return;
    }
    let mut reply = String::new();
    std::io::stdin().read_to_string(&mut reply).expect("could not read standard input");

    // The values kept for the session, as the window holds them.
    let args: Vec<String> = std::env::args().collect();
    let kept: Vec<calc::Kept> = args
        .iter()
        .position(|arg| arg == "--kept")
        .and_then(|at| serde_json::from_str(args.get(at + 1)?).ok())
        .unwrap_or_default();

    if std::env::args().any(|arg| arg == "--keep") {
        let outcome = match calc::keep(&reply, &kept) {
            Ok(entry) => serde_json::json!({ "kept": entry }),
            Err(Some(reason)) => serde_json::json!({ "reason": reason }),
            Err(None) => serde_json::Value::Null,
        };
        println!("{outcome}");
        return;
    }

    if std::env::args().any(|arg| arg == "--instant") {
        let found = calc::evaluate_with_value_in(&reply, &kept).map(|(answer, si_value)| {
            // The answer as the window gets it, with the value for checking.
            let mut found = serde_json::to_value(&answer).expect("could not write JSON");
            found["si_value"] = si_value.into();
            found
        });
        println!("{}", serde_json::to_string(&found).expect("could not write JSON"));
        return;
    }

    if std::env::args().any(|arg| arg == "--why") {
        println!("{}", serde_json::to_string(&calc::why_not_in(&reply, &kept)).expect("could not write JSON"));
        return;
    }

    if std::env::args().any(|arg| arg == "--ask") {
        let program = claude_code::find_program().expect("Claude Code is not installed");
        let storage = std::env::temp_dir().join("slipstick-ask");
        let (file, folder) =
            claude_code::prepare(&storage, solver::INSTRUCTIONS).expect("could not prepare files");
        let claude = Arc::new(claude_code::ClaudeCode::new(program, file, folder));
        if std::env::args().any(|arg| arg == "--deeper") {
            claude.set_model("opus", "medium");
        }

        // Start it ahead of time, as the app does, then time only the answer.
        claude.warm();
        std::thread::sleep(std::time::Duration::from_secs(6));
        let started = std::time::Instant::now();
        // Plain text is the problem itself; JSON may carry a picture too.
        #[derive(serde::Deserialize)]
        struct Request {
            #[serde(default)]
            input: String,
            #[serde(default)]
            images: Vec<claude_code::Image>,
        }
        let request = serde_json::from_str::<Request>(reply.trim())
            .unwrap_or_else(|_| Request { input: reply.trim().to_string(), images: Vec::new() });
        let answer = if std::env::args().any(|arg| arg == "--force-second-try") {
            // Pretend the first reply failed, whatever it was, and ask again.
            claude.begin(&request.input, &request.images).and_then(|(first, mut conversation)| {
                eprintln!("first reply after {} ms: {first}", started.elapsed().as_millis());
                let pretend = solver::solve_reply(r#"{"state":"answer","steps":[{"name":"x","expr":"1","note":""}]}"#);
                let correction = solver::correction_request(&pretend).expect("a failed reply");
                conversation.follow_up(&correction).map(|second| {
                    let mut solved = solver::solve_reply(&second);
                    solved.second_try = true;
                    solved
                })
            })
        } else if std::env::args().any(|arg| arg == "--show-replies") {
            // The same as the app does, printing Claude's own words on the way.
            claude.begin(&request.input, &request.images).and_then(|(first, mut conversation)| {
                eprintln!("first reply:\n{first}\n");
                let solved = solver::solve_reply(&first);
                let Some(correction) = solver::correction_request(&solved) else { return Ok(solved) };
                eprintln!("asked again with:\n{correction}\n");
                conversation.follow_up(&correction).map(|second| {
                    eprintln!("second reply:\n{second}\n");
                    let mut solved = solver::solve_reply(&second);
                    solved.second_try = true;
                    solved
                })
            })
        } else {
            solver::ask_and_solve(&claude, &request.input, &request.images)
        };
        eprintln!("answered in {} ms", started.elapsed().as_millis());
        match answer {
            Ok(solved) => println!("{}", serde_json::to_string(&solved).expect("could not write JSON")),
            Err(error) => println!("error: {error}"),
        }
        return;
    }

    let solved = solver::solve_reply(&reply);
    println!("{}", serde_json::to_string(&solved).expect("could not write JSON"));
}
