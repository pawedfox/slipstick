//! The Claude Code route: problems are answered by the user's own installed,
//! unmodified Claude Code, signed in however the user set it up.
//!
//! Slipstick never sees a credential. It starts the `claude` program the
//! user installed and signed in to, hands it one problem, and reads back one
//! reply. Tools are switched off, so Claude can only answer with text.
//!
//! How Claude Code signs in is the user's business and is left alone: the
//! program is run as it is published, with the environment it would have
//! anyway. Anthropic's conditions for products that run Claude Code require
//! this (see "Running Claude Code" in SPEC.md).
//!
//! Starting Claude Code takes a few seconds, so a process is started ahead
//! of time (`warm`) and waits for a problem. Each process answers exactly one
//! problem and is then replaced, which keeps every problem independent.

use std::{
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
    sync::{Arc, Mutex},
    time::Duration,
};

/// A picture pasted in with, or in place of, the typed problem.
#[derive(Clone, Debug, serde::Deserialize)]
pub struct Image {
    /// For example `image/png`.
    pub media_type: String,
    /// The picture itself, base64-encoded.
    pub data: String,
}

/// What Claude is asked when pictures arrive with no words.
const PICTURES_ONLY: &str = "Solve the problem shown in the picture or pictures.";

/// Environment variables that make Claude Code sign in with a key or token
/// in place of the user's Claude account. They are never changed and their
/// values are never read: the app only looks at whether one is set, to say
/// so when sign-in is refused.
const KEY_VARIABLES: [&str; 2] = ["ANTHROPIC_API_KEY", "ANTHROPIC_AUTH_TOKEN"];

/// The name of a key variable that is set for this app, if any.
fn key_variable_in_use() -> Option<&'static str> {
    KEY_VARIABLES.into_iter().find(|name| std::env::var_os(name).is_some_and(|value| !value.is_empty()))
}

/// How long to wait for an answer before giving up.
const TIME_LIMIT: Duration = Duration::from_secs(60);

// ---- when Claude cannot be asked ------------------------------------------
//
// Each message is one plain line: what happened, then what to do next.

/// The error returned when the user stopped the question. The window never
/// shows it: by then it has moved on.
pub const STOPPED: &str = "Stopped.";

const TOO_SLOW: &str = "Claude did not answer within a minute. Press Enter to try again.";
const OFFLINE: &str =
    "Claude could not be reached. Check your internet connection, then press Enter to try again.";
const BUSY: &str = "Claude is busy or having trouble right now. Press Enter to try again in a moment.";
const NOT_SIGNED_IN: &str = "Claude Code is not signed in. Open a terminal, run claude and sign in, \
     then press Enter here to try again.";
const WOULD_NOT_START: &str = "Claude Code would not start. Check that claude runs in a terminal, \
     then press Enter here to try again.";
const NO_ANSWER: &str = "Claude Code stopped without answering. Press Enter to try again.";

/// While it cannot connect, Claude Code tries again by itself for several
/// minutes and says nothing. The app gives up for it after this many tries
/// (about three seconds), which is long enough for a brief drop-out.
const TRIES_WHEN_UNREACHABLE: u64 = 3;
/// When the service answers but is overloaded or failing, a later try often
/// works, so more are allowed (about ten seconds).
const TRIES_WHEN_BUSY: u64 = 5;

/// Claude Code's own wording, kept short enough for one line of the window.
fn in_its_words(text: &str) -> String {
    const LONGEST: usize = 140;
    let text = text.trim().trim_end_matches('.');
    if text.chars().count() <= LONGEST {
        text.to_string()
    } else {
        format!("{}…", text.chars().take(LONGEST).collect::<String>().trim_end())
    }
}

/// What to say when Claude Code is refused sign-in. With a key variable set,
/// Claude Code uses that key in place of the user's account, so the key is
/// what was refused. A variable changed in Windows does not reach an app
/// that is already running, hence the restart.
fn sign_in_refused(key_variable: Option<&str>) -> String {
    match key_variable {
        Some(name) => format!(
            "Claude Code was refused sign-in. It is using the key in {name} on this PC. \
             Correct or remove that variable, restart Slipstick, then try again."
        ),
        None => NOT_SIGNED_IN.to_string(),
    }
}

/// Turns a failure reported by Claude Code into a message for the user.
/// `kind` is the name Claude Code gives the failure (empty if it gave none),
/// `text` is its own message, and `key_variable` is the key variable set
/// for this app, if there is one.
fn explain(kind: &str, text: &str, key_variable: Option<&str>) -> String {
    let said = in_its_words(text);
    // Older versions name no kind; these two are recognised by their words.
    let signed_out = text.contains("Not logged in") || text.contains("/login") || text.contains("API key");
    match kind {
        "authentication_failed" => sign_in_refused(key_variable),
        "rate_limit" => format!("Your Claude usage limit has been reached ({said}). Plain maths still works."),
        "overloaded" | "server_error" => BUSY.to_string(),
        "model_not_found" => "That model is not available to your Claude Code. Choose the other one \
             under Model in settings (Ctrl+,), then press Enter."
            .to_string(),
        "max_output_tokens" => {
            "Claude's reply was cut short. Try a shorter problem, or split it in two.".to_string()
        }
        "billing_error" | "account_on_hold" | "verification_required" | "oauth_org_not_allowed"
        | "cloud_credential_error" => {
            format!("Your Claude account needs attention before it can be used here. Claude Code says: {said}.")
        }
        "invalid_request" => format!("Claude could not take that problem. Claude Code says: {said}."),
        _ if signed_out => sign_in_refused(key_variable),
        _ if said.is_empty() => NO_ANSWER.to_string(),
        _ => format!("Claude Code reported a problem: {said}. Press Enter to try again."),
    }
}

pub struct ClaudeCode {
    program: PathBuf,
    /// File holding the instructions given to Claude.
    instructions: PathBuf,
    /// An empty folder to run in, so no project instructions are picked up.
    folder: PathBuf,
    /// The model and effort each new process is started with.
    choice: Mutex<(&'static str, &'static str)>,
    /// A process that has been started and is waiting for a problem.
    ready: Mutex<Option<Process>>,
    /// The id of the process answering a problem right now, if any.
    asking: Mutex<Option<u32>>,
    /// The id of the last process stopped for taking too long, so that it
    /// can be told apart from one the user stopped.
    too_slow: std::sync::atomic::AtomicU32,
}

struct Process {
    child: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
}

/// Looks for an installed Claude Code: on the user's PATH, then in the two
/// folders its installers use. The second look matters when Claude Code was
/// installed while the app was running, because the app's own PATH is the
/// one it was started with.
pub fn find_program() -> Option<PathBuf> {
    let on_path = std::env::var_os("PATH").map(|path| std::env::split_paths(&path).collect::<Vec<_>>());
    let usual = [
        std::env::var_os("USERPROFILE").map(|home| PathBuf::from(home).join(".local").join("bin")),
        std::env::var_os("APPDATA").map(|roaming| PathBuf::from(roaming).join("npm")),
    ];
    on_path
        .into_iter()
        .flatten()
        .chain(usual.into_iter().flatten())
        .flat_map(|folder| ["claude.exe", "claude.cmd", "claude"].map(|name| folder.join(name)))
        .find(|candidate| candidate.is_file())
}

fn hidden(mut command: Command) -> Command {
    // Without this, Windows flashes a console window for every process.
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    command
}

/// Ends a process and everything it started. Claude Code is launched through
/// a small script on Windows, so ending only the script would leave the real
/// program running.
fn end_tree(id: u32) {
    #[cfg(windows)]
    {
        let mut command = hidden(Command::new("taskkill"));
        command.args(["/PID", &id.to_string(), "/T", "/F"]);
        let _ = command.stdout(Stdio::null()).stderr(Stdio::null()).status();
    }
    #[cfg(not(windows))]
    {
        let _ = Command::new("kill").arg(id.to_string()).status();
    }
}

impl ClaudeCode {
    pub fn new(program: PathBuf, instructions: PathBuf, folder: PathBuf) -> Self {
        Self {
            program,
            instructions,
            folder,
            // Chosen on accuracy against the reference set. See SPEC.md.
            choice: Mutex::new(("sonnet", "low")),
            ready: Mutex::new(None),
            asking: Mutex::new(None),
            too_slow: std::sync::atomic::AtomicU32::new(0),
        }
    }

    /// Chooses the model and effort for problems asked from now on. A
    /// process already waiting was started with the old choice, so it is
    /// stopped.
    pub fn set_model(&self, model: &'static str, effort: &'static str) {
        let changed = self.choice.lock().is_ok_and(|mut choice| {
            let changed = *choice != (model, effort);
            *choice = (model, effort);
            changed
        });
        if changed {
            self.rest();
        }
    }

    fn start(&self) -> std::io::Result<Process> {
        let (model, effort) = self.choice.lock().map(|choice| *choice).unwrap_or(("sonnet", "low"));
        let mut command = hidden(Command::new(&self.program));
        command
            .args(["-p", "--input-format", "stream-json", "--output-format", "stream-json", "--verbose"])
            .args(["--model", model, "--effort", effort])
            // No tools at all: Claude may only reply with text.
            .args(["--tools", "", "--strict-mcp-config", "--no-session-persistence"])
            .arg("--system-prompt-file")
            .arg(&self.instructions)
            .current_dir(&self.folder)
            // The environment is handed on as it is. Nothing in it is added,
            // removed or read, so Claude Code signs in exactly as it would
            // if the user ran it themselves.
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());

        let mut child = command.spawn()?;
        let (Some(input), Some(output)) = (child.stdin.take(), child.stdout.take()) else {
            end_tree(child.id());
            return Err(std::io::Error::other("no pipes"));
        };
        Ok(Process { child, input, output: BufReader::new(output) })
    }

    /// Starts a process ahead of time, if one is not already waiting.
    pub fn warm(&self) {
        let Ok(mut ready) = self.ready.lock() else { return };
        if ready.is_none() {
            *ready = self.start().ok();
        }
    }

    /// Stops the waiting process, if there is one.
    pub fn rest(&self) {
        let waiting = self.ready.lock().ok().and_then(|mut ready| ready.take());
        if let Some(process) = waiting {
            end_tree(process.child.id());
        }
    }

    /// Stops the problem being answered, if there is one.
    pub fn cancel(&self) {
        let id = self.asking.lock().ok().and_then(|mut asking| asking.take());
        if let Some(id) = id {
            end_tree(id);
        }
    }

    /// Starts on one problem, with any pictures, and returns Claude's first
    /// reply together with the conversation, so a correction can be asked
    /// for without sending the problem again.
    pub fn begin(
        self: &Arc<Self>,
        problem: &str,
        images: &[Image],
    ) -> Result<(String, Conversation), String> {
        let waiting = self.ready.lock().ok().and_then(|mut ready| ready.take());
        let process = match waiting {
            Some(process) => process,
            None => self.start().map_err(|_| WOULD_NOT_START.to_string())?,
        };
        let Process { child, input, output } = process;
        let id = child.id();
        if let Ok(mut asking) = self.asking.lock() {
            *asking = Some(id);
        }

        // Give up if no answer comes within the time limit.
        let watch = Arc::clone(self);
        std::thread::spawn(move || {
            std::thread::sleep(TIME_LIMIT);
            let still_waiting = watch.asking.lock().is_ok_and(|asking| *asking == Some(id));
            if still_waiting {
                watch.too_slow.store(id, std::sync::atomic::Ordering::Relaxed);
                watch.cancel();
            }
        });

        let content = if images.is_empty() {
            serde_json::json!(problem)
        } else {
            // The pictures first, in the order they were pasted, then the words.
            let mut blocks: Vec<serde_json::Value> = images
                .iter()
                .map(|image| {
                    serde_json::json!({
                        "type": "image",
                        "source": { "type": "base64", "media_type": image.media_type, "data": image.data },
                    })
                })
                .collect();
            let words = if problem.trim().is_empty() { PICTURES_ONLY } else { problem };
            blocks.push(serde_json::json!({ "type": "text", "text": words }));
            serde_json::Value::Array(blocks)
        };

        let mut conversation = Conversation {
            claude: Arc::clone(self),
            id,
            child: Some(child),
            input,
            output,
            unread_results: 0,
        };
        let reply = conversation.say(content)?;
        Ok((reply, conversation))
    }

    /// Sends one problem and returns Claude's reply as text.
    pub fn ask(self: &Arc<Self>, problem: &str, images: &[Image]) -> Result<String, String> {
        self.begin(problem, images).map(|(reply, _conversation)| reply)
    }
}

/// One problem being answered. Claude Code remembers the exchange so far for
/// as long as this lives, and is told to finish when it is dropped.
pub struct Conversation {
    claude: Arc<ClaudeCode>,
    id: u32,
    child: Option<Child>,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
    /// Replies already returned whose closing "result" line has not been
    /// read yet. Those lines must be skipped, not mistaken for a new reply.
    unread_results: usize,
}

impl Conversation {
    fn say(&mut self, content: serde_json::Value) -> Result<String, String> {
        let message = serde_json::json!({
            "type": "user",
            "message": { "role": "user", "content": content },
        });
        let sent = writeln!(self.input, "{message}").and_then(|_| self.input.flush());
        let reply = match sent {
            Ok(()) => read_reply(&mut self.output, &mut self.unread_results, key_variable_in_use()),
            Err(_) => Err(NO_ANSWER.to_string()),
        };
        // If `asking` no longer holds this process, it was stopped: by the
        // user, or for taking too long.
        let still_ours = self.claude.asking.lock().is_ok_and(|asking| *asking == Some(self.id));
        if !still_ours {
            let too_slow = self.claude.too_slow.load(std::sync::atomic::Ordering::Relaxed) == self.id;
            return Err(if too_slow { TOO_SLOW } else { STOPPED }.to_string());
        }
        if reply.is_err() {
            // Nothing more can come of this conversation. Claude Code may
            // still be trying to connect, and would go on for minutes.
            end_tree(self.id);
        }
        reply
    }

    /// Says something more in the same conversation and returns the reply.
    pub fn follow_up(&mut self, text: &str) -> Result<String, String> {
        self.say(serde_json::json!(text))
    }
}

impl Drop for Conversation {
    fn drop(&mut self) {
        if let Ok(mut asking) = self.claude.asking.lock()
            && *asking == Some(self.id)
        {
            *asking = None;
        }
        // The input closes when this is dropped, which lets Claude Code
        // finish and exit by itself. Wait for it so it is cleaned up.
        if let Some(mut child) = self.child.take() {
            std::thread::spawn(move || {
                let _ = child.wait();
            });
        }
    }
}

/// Whether an "assistant" line is Claude Code speaking for itself, not
/// Claude. When a request fails (a usage limit, a server error, no access to
/// the model), Claude Code prints its own message in the same form as a
/// reply. That message must never be taken for Claude's working.
fn is_error_message(event: &serde_json::Value) -> bool {
    event["is_api_error_message"].as_bool().unwrap_or(false)
        || !event["error"].is_null()
        || event["message"]["model"] == "<synthetic>"
}

/// Reads Claude Code's output until Claude's next reply appears.
fn read_reply(
    output: &mut impl BufRead,
    unread_results: &mut usize,
    key_variable: Option<&str>,
) -> Result<String, String> {
    let mut line = String::new();
    loop {
        line.clear();
        match output.read_line(&mut line) {
            Ok(0) | Err(_) => return Err(NO_ANSWER.to_string()),
            Ok(_) => {}
        }
        let Ok(event) = serde_json::from_str::<serde_json::Value>(&line) else { continue };

        match event["type"].as_str() {
            Some("assistant") => {
                let text: String = event["message"]["content"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter(|block| block["type"] == "text")
                    .filter_map(|block| block["text"].as_str())
                    .collect();
                if !text.trim().is_empty() {
                    // Its closing "result" line is still to come.
                    *unread_results += 1;
                    return if is_error_message(&event) {
                        Err(explain(event["error"].as_str().unwrap_or_default(), &text, key_variable))
                    } else {
                        Ok(text)
                    };
                }
            }
            Some("result") => {
                if *unread_results > 0 {
                    // The end of a reply that has already been returned.
                    *unread_results -= 1;
                    continue;
                }
                let text = event["result"].as_str().unwrap_or_default();
                return if event["is_error"].as_bool().unwrap_or(false) || text.trim().is_empty() {
                    Err(explain("", text, key_variable))
                } else {
                    Ok(text.to_string())
                };
            }
            // Claude Code is trying again after a failed request. With no
            // status the request never arrived: there is no connection.
            Some("system") if event["subtype"] == "api_retry" => {
                let tries = event["attempt"].as_u64().unwrap_or(0);
                match event["error_status"].as_u64() {
                    None if tries >= TRIES_WHEN_UNREACHABLE => return Err(OFFLINE.to_string()),
                    Some(_) if tries >= TRIES_WHEN_BUSY => return Err(BUSY.to_string()),
                    _ => {}
                }
            }
            _ => {}
        }
    }
}

/// Writes the instructions to a file Claude Code can read, and makes the
/// empty folder it runs in. Returns `None` if either cannot be created.
pub fn prepare(storage: &Path, instructions: &str) -> Option<(PathBuf, PathBuf)> {
    let folder = storage.join("claude-code");
    std::fs::create_dir_all(&folder).ok()?;
    let file = storage.join("instructions.md");
    std::fs::write(&file, instructions).ok()?;
    Some((file, folder))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(lines: &[&str]) -> Result<String, String> {
        let mut output = std::io::Cursor::new(lines.join("
"));
        // As on a PC with no key variable set.
        read_reply(&mut output, &mut 0, None)
    }

    #[test]
    fn a_reply_from_claude_is_returned() {
        let reply = read(&[
            r#"{"type":"system","subtype":"init"}"#,
            r#"{"type":"assistant","message":{"model":"claude-sonnet","content":[{"type":"text","text":"{\"state\":\"answer\"}"}]}}"#,
        ]);
        assert_eq!(reply.as_deref(), Ok(r#"{"state":"answer"}"#));
    }

    #[test]
    fn claude_codes_own_error_is_not_a_reply() {
        // The shape Claude Code prints when a request fails.
        let failed = read(&[
            r#"{"type":"assistant","message":{"model":"<synthetic>","content":[{"type":"text","text":"You have hit your limit."}]},"error":"rate_limit","is_api_error_message":true}"#,
            r#"{"type":"result","subtype":"success","is_error":true,"result":"You have hit your limit."}"#,
        ]);
        assert_eq!(
            failed,
            Err("Your Claude usage limit has been reached (You have hit your limit). Plain maths still works."
                .to_string())
        );

        // Any one of the three signs is enough.
        for line in [
            r#"{"type":"assistant","message":{"model":"<synthetic>","content":[{"type":"text","text":"Oops"}]}}"#,
            r#"{"type":"assistant","message":{"model":"m","content":[{"type":"text","text":"Oops"}]},"error":"unknown"}"#,
            r#"{"type":"assistant","message":{"model":"m","content":[{"type":"text","text":"Oops"}]},"is_api_error_message":true}"#,
        ] {
            assert_eq!(
                read(&[line]),
                Err("Claude Code reported a problem: Oops. Press Enter to try again.".to_string())
            );
        }
    }

    #[test]
    fn an_error_result_with_no_reply_is_an_error() {
        let failed = read(&[r#"{"type":"result","is_error":true,"result":"Not logged in"}"#]);
        assert_eq!(failed, Err(NOT_SIGNED_IN.to_string()));
        assert_eq!(read(&[]), Err(NO_ANSWER.to_string()));
        assert_eq!(read(&[r#"{"type":"result","is_error":false,"result":""}"#]), Err(NO_ANSWER.to_string()));
    }

    /// One line as Claude Code prints it for a failed request.
    fn failure(kind: &str, text: &str) -> String {
        serde_json::json!({
            "type": "assistant",
            "message": { "model": "<synthetic>", "content": [{ "type": "text", "text": text }] },
            "error": kind,
            "is_api_error_message": true,
        })
        .to_string()
    }

    #[test]
    fn each_kind_of_failure_says_what_to_do() {
        let message = |kind: &str, text: &str| read(&[&failure(kind, text)]).unwrap_err();

        // The two the user met in the installed app, as Claude Code worded them.
        assert_eq!(message("authentication_failed", "Invalid API key · Fix external API key"), NOT_SIGNED_IN);
        assert_eq!(message("authentication_failed", "Not logged in · Please run /login"), NOT_SIGNED_IN);

        assert_eq!(message("overloaded", "API Error: Overloaded"), BUSY);
        assert_eq!(message("server_error", "API Error: 500"), BUSY);
        assert!(message("model_not_found", "There's an issue with the selected model (opus).")
            .contains("under Model in settings"));
        assert!(message("max_output_tokens", "too long").contains("cut short"));
        assert!(message("rate_limit", "You've hit your session limit · resets 7pm").contains("resets 7pm"));
        assert_eq!(
            message("billing_error", "Credit balance is too low."),
            "Your Claude account needs attention before it can be used here. \
             Claude Code says: Credit balance is too low."
        );
        assert!(message("invalid_request", "Image too large").starts_with("Claude could not take that problem."));
        // A kind this version does not know still gets Claude Code's words.
        assert_eq!(
            message("something_new", "A new sort of trouble."),
            "Claude Code reported a problem: A new sort of trouble. Press Enter to try again."
        );
        // Very long wording is cut to fit a line of the window.
        let long = message("something_new", &"word ".repeat(100));
        assert!(long.chars().count() < 220 && long.contains('…'), "{long}");
    }

    #[test]
    fn a_refused_key_variable_is_named() {
        // What Claude Code prints when the key in ANTHROPIC_API_KEY is
        // refused. Seen in a real install.
        let line = failure("authentication_failed", "Invalid API key · Fix external API key");
        let read_with = |variable| {
            let mut output = std::io::Cursor::new(line.clone());
            read_reply(&mut output, &mut 0, variable).unwrap_err()
        };
        assert_eq!(
            read_with(Some("ANTHROPIC_API_KEY")),
            "Claude Code was refused sign-in. It is using the key in ANTHROPIC_API_KEY on this PC. \
             Correct or remove that variable, restart Slipstick, then try again."
        );
        assert!(read_with(Some("ANTHROPIC_AUTH_TOKEN")).contains("ANTHROPIC_AUTH_TOKEN"));
        // With no such variable, it is the user's own sign-in that failed.
        assert_eq!(read_with(None), NOT_SIGNED_IN);
    }

    /// One line as Claude Code prints it before trying a request again.
    fn retry(attempt: u64, status: Option<u64>) -> String {
        serde_json::json!({ "type": "system", "subtype": "api_retry", "attempt": attempt, "error_status": status })
            .to_string()
    }

    #[test]
    fn gives_up_early_when_claude_cannot_be_reached() {
        // No status means the request never arrived. Two tries are let go.
        let answer = r#"{"type":"assistant","message":{"model":"m","content":[{"type":"text","text":"{}"}]}}"#;
        assert_eq!(read(&[&retry(1, None), &retry(2, None), answer]), Ok("{}".to_string()));
        assert_eq!(read(&[&retry(1, None), &retry(2, None), &retry(3, None), answer]), Err(OFFLINE.to_string()));
    }

    #[test]
    fn waits_longer_when_claude_is_busy() {
        let answer = r#"{"type":"assistant","message":{"model":"m","content":[{"type":"text","text":"{}"}]}}"#;
        let busy: Vec<String> = (1..=4).map(|attempt| retry(attempt, Some(529))).collect();
        let mut lines: Vec<&str> = busy.iter().map(String::as_str).collect();
        lines.push(answer);
        assert_eq!(read(&lines), Ok("{}".to_string()));

        let fifth = retry(5, Some(529));
        lines.insert(4, &fifth);
        assert_eq!(read(&lines), Err(BUSY.to_string()));
    }
}
