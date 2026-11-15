//! Running an action: no shell, no surprises.

use std::collections::VecDeque;
use std::ffi::OsString;
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use crate::action::{file_label, Action};
use crate::error::ActionError;
use crate::keys::KeyCombo;

/// Keeps a console window from flashing when a console program is launched.
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// A command ready to run: a program and its arguments, already separated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandSpec {
    /// The program to run.
    pub program: OsString,
    /// The arguments, each one its own entry.
    pub args: Vec<OsString>,
}

impl CommandSpec {
    /// A command with no arguments.
    pub fn new(program: impl Into<OsString>) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
        }
    }

    /// Adds the arguments.
    pub fn with_args(mut self, args: impl IntoIterator<Item = OsString>) -> Self {
        self.args.extend(args);
        self
    }

    /// Starts the program, detached from our console.
    ///
    /// The returned [`Child`] is only a handle: dropping it does not stop the
    /// program, it just stops us watching it.
    pub fn spawn(&self) -> Result<std::process::Child, std::io::Error> {
        let mut command = Command::new(&self.program);
        command.args(&self.args);
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(CREATE_NO_WINDOW);
        }
        command.spawn()
    }
}

/// The command that runs an action.
///
/// Nothing goes through a shell: the program and its arguments stay separate,
/// so an argument can never turn into a second command.
///
/// A key combination has no command: it is sent, not started. Asking for one
/// here returns [`ActionError::KeysNotAProgram`] rather than a spec that starts
/// nothing.
pub fn command_for(action: &Action) -> Result<CommandSpec, ActionError> {
    let action = action.without_wrapping_quotes();
    action.validate()?;
    match &action {
        Action::Launch { program, args } => {
            let spec = CommandSpec::new(program.clone().into_os_string())
                .with_args(args.iter().map(OsString::from));
            Ok(apply_launch_rules(spec))
        }
        Action::OpenUrl { url } => Ok(open_url_spec(url)),
        Action::Keys { keys } => Err(ActionError::KeysNotAProgram {
            keys: keys.to_string(),
        }),
    }
}

/// Runs an action now, on this thread.
///
/// The program is started and left running; nothing waits for it to finish.
///
/// A key combination is not a program: this function only starts processes and
/// returns [`ActionError::KeysNotAProgram`] for one. Sending it belongs to a
/// [`KeySender`] installed on an [`ActionRunner`].
pub fn run_action(action: &Action) -> Result<(), ActionError> {
    launch(action).map(|launched| {
        // The handle is dropped on purpose: the program keeps running, we
        // just stop watching it.
        drop(launched.child);
    })
}

/// A program that was just started.
struct Launched {
    /// How to name the program in a message.
    label: String,
    /// The running process.
    child: std::process::Child,
    /// Whether its exit code says anything about the action.
    ///
    /// `explorer.exe` is a shell dispatcher: it returns a non-zero code even
    /// when it opened the file, so its code must never be read as a failure.
    exit_code_means_something: bool,
}

/// Starts the program an action asks for.
fn launch(action: &Action) -> Result<Launched, ActionError> {
    let action = action.without_wrapping_quotes();
    // A key combination is refused here, before anything is started.
    let spec = command_for(&action)?;
    let child = spec
        .spawn()
        .map_err(|source| start_error(&action, source))?;

    Ok(Launched {
        label: label_for(&action),
        child,
        exit_code_means_something: exit_code_means_something(&spec),
    })
}

/// How to name the action in a message.
fn label_for(action: &Action) -> String {
    match action {
        Action::Launch { program, .. } => file_label(program),
        Action::OpenUrl { url } => url.clone(),
        // `command_for` refuses a key combination before a program can be
        // started, so this arm is not reachable; it is here so the match stays
        // total, and it names the combination rather than inventing a label.
        Action::Keys { keys } => keys.to_string(),
    }
}

/// The error for a program that would not start.
fn start_error(action: &Action, source: std::io::Error) -> ActionError {
    match action {
        Action::Launch { program, .. } => ActionError::Launch {
            program: program.display().to_string(),
            source,
        },
        Action::OpenUrl { url } => ActionError::Open {
            url: url.clone(),
            source,
        },
        // Same reason as `label_for`: there is no program to start, so the
        // failure is reported as what it is instead of a made-up launch.
        Action::Keys { keys } => ActionError::KeysNotAProgram {
            keys: keys.to_string(),
        },
    }
}

/// Whether the exit code of this program can be read as a failure.
///
/// Explorer is the one program whose exit code lies: asked to open something it
/// hands the request to the running instance and leaves with a non-zero status
/// even when the window opened. Everywhere else an exit code means what it
/// says.
#[cfg(windows)]
fn exit_code_means_something(spec: &CommandSpec) -> bool {
    let name = std::path::Path::new(&spec.program)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    !(name == "explorer.exe" || name == "explorer")
}

/// Everywhere else there is no program whose exit code lies, so the argument
/// is not read.
#[cfg(not(windows))]
fn exit_code_means_something(_spec: &CommandSpec) -> bool {
    true
}

/// How long a started program is watched for a failure.
///
/// A program that cannot run — a missing runtime, a missing dependency — does
/// not die instantly: the .NET host, for instance, takes about four seconds
/// to print its complaint and leave. The window has to cover that.
const EXIT_WATCH: std::time::Duration = std::time::Duration::from_secs(6);

/// Watches a started program and reports it if it dies on its own.
///
/// The watch runs on its own thread and always ends: a program that is still
/// alive when the window closes is a program that worked.
fn watch_exit(
    label: String,
    mut child: std::process::Child,
    sink: Arc<Mutex<VecDeque<String>>>,
    window: std::time::Duration,
) {
    let spawned = std::thread::Builder::new()
        .name("ss550-action-watch".to_string())
        .spawn(move || {
            let deadline = std::time::Instant::now() + window;
            loop {
                match child.try_wait() {
                    Ok(Some(status)) => {
                        if !status.success() {
                            let message = match status.code() {
                                Some(code) => format!(
                                    "{label} exited with code {code} (0x{:08X})",
                                    code as u32
                                ),
                                None => format!("{label} was stopped by the system"),
                            };
                            if let Ok(mut queue) = sink.lock() {
                                queue.push_back(message);
                            }
                        }
                        return;
                    }
                    Ok(None) => {}
                    Err(error) => {
                        if let Ok(mut queue) = sink.lock() {
                            queue.push_back(format!("{label} could not be watched: {error}"));
                        }
                        return;
                    }
                }

                if std::time::Instant::now() >= deadline {
                    // Still running: the action worked. The handle is dropped
                    // and the program carries on.
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
        });
    // A watcher that cannot start is not worth reporting: the action itself
    // already ran.
    drop(spawned);
}

/// Windows cannot start `.bat`, `.cmd` or `.lnk` files with `CreateProcess`,
/// so those go through the program that can: `cmd` for scripts, `explorer`
/// for shortcuts.
#[cfg(windows)]
fn apply_launch_rules(spec: CommandSpec) -> CommandSpec {
    let extension = std::path::Path::new(&spec.program)
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();

    match extension.as_str() {
        "bat" | "cmd" => {
            // `call` is what makes this survive a path with spaces.
            //
            // `cmd /C` does not keep a quoted command line as it is: with more
            // than two quotes it strips the first and the last one and then
            // runs the **first word**, which for `C:\Program Files\…\run.bat`
            // is `C:\Program`. `cmd /C call "C:\Program Files\…\run.bat"`
            // keeps the whole path together and hands the script its
            // arguments the way a terminal would.
            let mut args = vec![
                OsString::from("/C"),
                OsString::from("call"),
                spec.program.clone(),
            ];
            args.extend(spec.args);
            CommandSpec::new(OsString::from("cmd")).with_args(args)
        }
        "lnk" => {
            let mut args = vec![spec.program.clone()];
            args.extend(spec.args);
            CommandSpec::new(OsString::from("explorer")).with_args(args)
        }
        _ => spec,
    }
}

/// Everywhere else the program runs as given.
#[cfg(not(windows))]
fn apply_launch_rules(spec: CommandSpec) -> CommandSpec {
    spec
}

/// Opens a URL with the system's default handler.
#[cfg(windows)]
fn open_url_spec(url: &str) -> CommandSpec {
    CommandSpec::new(OsString::from("explorer")).with_args([OsString::from(url)])
}

#[cfg(target_os = "macos")]
fn open_url_spec(url: &str) -> CommandSpec {
    CommandSpec::new(OsString::from("open")).with_args([OsString::from(url)])
}

#[cfg(all(unix, not(target_os = "macos")))]
fn open_url_spec(url: &str) -> CommandSpec {
    CommandSpec::new(OsString::from("xdg-open")).with_args([OsString::from(url)])
}

/// Sends a key combination to whatever window has the focus.
///
/// The engine does not type, and it must not: delivering a keystroke is a
/// platform call (`SendInput` on Windows), the one place where `unsafe` would
/// be unavoidable, and the crate that owns that exception is `ss550-tray`. The
/// UI hands its sender to [`ActionRunner::with_key_sender`]; the engine only
/// decides *when* a combination goes out, never *how*.
///
/// ```
/// use ss550_engine::{KeyCombo, KeySender};
///
/// /// A sender that only remembers what it was asked to send.
/// struct Remember(std::sync::Mutex<Vec<String>>);
///
/// impl KeySender for Remember {
///     fn send(&self, combo: &KeyCombo) -> Result<(), String> {
///         match self.0.lock() {
///             Ok(mut sent) => {
///                 sent.push(combo.to_string());
///                 Ok(())
///             }
///             // A sender that cannot record has not sent anything.
///             Err(_) => Err("the sender is unavailable".to_string()),
///         }
///     }
/// }
///
/// let sender = Remember(std::sync::Mutex::new(Vec::new()));
/// assert!(sender.send(&KeyCombo::parse("ctrl+m").expect("valid")).is_ok());
/// ```
pub trait KeySender: Send + Sync {
    /// Sends `combo` to the window that has the focus.
    ///
    /// The error is the reason, ready to read: the engine wraps it in
    /// [`ActionError::KeySend`] together with the combination it belongs to. A
    /// sender that returns `Ok` claims the keystroke was delivered; there is no
    /// way to say "nothing happened" by succeeding.
    fn send(&self, combo: &KeyCombo) -> Result<(), String>;
}

/// Runs actions on a background thread, so the UI never waits for one.
///
/// A runner built with [`ActionRunner::new`] starts programs and opens URLs but
/// cannot type: a key combination submitted to it is refused with
/// [`ActionError::NeedsKeySender`], never quietly accepted. Install a
/// [`KeySender`] when the platform one exists.
pub struct ActionRunner {
    commands: Sender<Job>,
    failures: Arc<Mutex<VecDeque<String>>>,
    key_sender: Option<Arc<dyn KeySender>>,
    worker: Option<JoinHandle<()>>,
}

impl Default for ActionRunner {
    fn default() -> Self {
        Self::new()
    }
}

impl ActionRunner {
    /// Starts the background worker, with no way to send keystrokes.
    pub fn new() -> Self {
        Self::build(EXIT_WATCH, None)
    }

    /// Starts the background worker with a custom window for spotting a
    /// program that dies on its own.
    ///
    /// The window is how long a started program is watched after it starts.
    /// A program that is still alive when the window closes is a program that
    /// worked; one that leaves earlier with a failure is reported.
    pub fn with_exit_watch(window: std::time::Duration) -> Self {
        Self::build(window, None)
    }

    /// Starts the background worker with the platform sender that delivers key
    /// combinations.
    ///
    /// The sender arrives from the UI, which gets it from the crate that owns
    /// the platform call. Without one, a combination is refused; with one, it
    /// is delivered on the worker thread and a failure is reported the same way
    /// a program that would not start is.
    ///
    /// ```
    /// use std::sync::Arc;
    /// use ss550_engine::{ActionRunner, KeyCombo, KeySender};
    ///
    /// struct Silent;
    ///
    /// impl KeySender for Silent {
    ///     fn send(&self, _combo: &KeyCombo) -> Result<(), String> {
    ///         Ok(())
    ///     }
    /// }
    ///
    /// let runner = ActionRunner::with_key_sender(Arc::new(Silent));
    /// assert!(runner.is_alive());
    /// ```
    pub fn with_key_sender(sender: Arc<dyn KeySender>) -> Self {
        Self::build(EXIT_WATCH, Some(sender))
    }

    /// Starts the worker with the settings every constructor shares.
    fn build(window: std::time::Duration, key_sender: Option<Arc<dyn KeySender>>) -> Self {
        let (commands, inbox) = mpsc::channel();
        let failures = Arc::new(Mutex::new(VecDeque::new()));
        let sink = Arc::clone(&failures);
        // The worker needs the sender to deliver with; the runner keeps its own
        // handle so `submit` can refuse a combination before it is queued.
        let sender = key_sender.clone();

        let worker = std::thread::Builder::new()
            .name("ss550-actions".to_string())
            .spawn(move || {
                for job in inbox {
                    match job {
                        Job::Run(action) => match launch(&action) {
                            Ok(launched) => {
                                if launched.exit_code_means_something {
                                    watch_exit(
                                        launched.label,
                                        launched.child,
                                        Arc::clone(&sink),
                                        window,
                                    );
                                }
                            }
                            Err(error) => report(&sink, error),
                        },
                        Job::Keys(combo) => deliver(sender.as_ref(), &combo, &sink),
                    }
                }
            })
            .ok();

        Self {
            commands,
            failures,
            key_sender,
            worker,
        }
    }

    /// Hands an action to the worker.
    ///
    /// The action is checked before it is queued: a broken action is reported
    /// to the caller instead of failing later, out of sight. A key combination
    /// needs an installed [`KeySender`]; without one it is refused here, on
    /// this thread, so "nothing was sent" cannot look like success.
    pub fn submit(&self, action: Action) -> Result<(), ActionError> {
        action.validate()?;
        let job = match action {
            Action::Keys { keys } => {
                self.check_sender(&keys)?;
                Job::Keys(keys)
            }
            action => Job::Run(action),
        };
        self.commands
            .send(job)
            .map_err(|_| ActionError::RunnerStopped)
    }

    /// Sends a key combination now, through the installed sender.
    ///
    /// The send happens on the worker thread, so a slow platform call never
    /// stalls the window. This is the entry point for a combination that is not
    /// bound to a deck key — a test button, a palette action being tried out.
    ///
    /// ```
    /// use ss550_engine::{ActionRunner, KeyCombo};
    ///
    /// let runner = ActionRunner::new();
    /// let combo = KeyCombo::parse("ctrl+shift+m").expect("valid");
    /// assert!(runner.send_keys(combo).is_err(), "no sender means no send");
    /// ```
    pub fn send_keys(&self, combo: KeyCombo) -> Result<(), ActionError> {
        self.check_sender(&combo)?;
        self.commands
            .send(Job::Keys(combo))
            .map_err(|_| ActionError::RunnerStopped)
    }

    /// Refuses a combination when there is nothing to deliver it with.
    fn check_sender(&self, combo: &KeyCombo) -> Result<(), ActionError> {
        if self.key_sender.is_some() {
            Ok(())
        } else {
            Err(ActionError::NeedsKeySender {
                keys: combo.to_string(),
            })
        }
    }

    /// Takes the oldest failure the worker has reported, if any.
    pub fn take_failure(&self) -> Option<String> {
        self.failures
            .lock()
            .ok()
            .and_then(|mut queue| queue.pop_front())
    }

    /// Whether the worker is still alive.
    pub fn is_alive(&self) -> bool {
        self.worker
            .as_ref()
            .is_some_and(|handle| !handle.is_finished())
    }
}

/// What the worker thread does next.
///
/// The queue keeps the two apart because a keystroke is not an action to run:
/// it is delivered, and its failure is reported the way a failed start is.
enum Job {
    /// Start the program an action names.
    Run(Action),
    /// Deliver a key combination.
    Keys(KeyCombo),
}

/// Puts a failure where the window will find it.
fn report(sink: &Arc<Mutex<VecDeque<String>>>, error: ActionError) {
    if let Ok(mut queue) = sink.lock() {
        queue.push_back(error.to_string());
    }
}

/// Hands a combination to the sender and reports a failure it names.
fn deliver(
    sender: Option<&Arc<dyn KeySender>>,
    combo: &KeyCombo,
    sink: &Arc<Mutex<VecDeque<String>>>,
) {
    let outcome = match sender {
        Some(sender) => sender.send(combo).map_err(|reason| ActionError::KeySend {
            keys: combo.to_string(),
            reason,
        }),
        // `submit` and `send_keys` refuse a combination when no sender is
        // installed, so the queue never holds one. If it ever did, it is
        // reported: a send that did not happen is never silence.
        None => Err(ActionError::NeedsKeySender {
            keys: combo.to_string(),
        }),
    };
    if let Err(error) = outcome {
        report(sink, error);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn missing_program() -> Action {
        Action::Launch {
            program: PathBuf::from("ss550-no-such-program-8f3a.exe"),
            args: Vec::new(),
        }
    }

    #[test]
    fn a_launch_action_keeps_its_arguments_apart() {
        let action = Action::Launch {
            program: PathBuf::from("C:\\Program Files\\App\\app.exe"),
            args: vec!["--profile".to_string(), "Live".to_string()],
        };
        let spec = command_for(&action).expect("valid action");
        assert_eq!(
            spec.program,
            OsString::from("C:\\Program Files\\App\\app.exe")
        );
        assert_eq!(
            spec.args,
            vec![OsString::from("--profile"), OsString::from("Live")]
        );
    }

    #[test]
    fn a_url_is_opened_with_the_platform_handler() {
        let action = Action::OpenUrl {
            url: "https://example.com".to_string(),
        };
        let spec = command_for(&action).expect("valid action");
        assert!(spec.args.iter().any(|arg| arg == "https://example.com"));
        assert!(!spec.program.is_empty());
    }

    #[test]
    fn a_bare_string_is_never_opened() {
        let action = Action::OpenUrl {
            url: "example.com".to_string(),
        };
        assert!(command_for(&action).is_err());
    }

    #[test]
    fn a_quoted_program_loses_its_quotes_before_it_runs() {
        let action = Action::Launch {
            program: "\"C:\\Program Files\\App\\app.exe\"".into(),
            args: Vec::new(),
        };
        let spec = command_for(&action).expect("valid action");
        assert_eq!(
            spec.program,
            OsString::from("C:\\Program Files\\App\\app.exe")
        );
    }

    #[cfg(windows)]
    #[test]
    fn a_quoted_shortcut_still_routes_through_explorer() {
        let action = Action::Launch {
            program: "\"C:\\ProgramData\\Start Menu\\Programs\\Brave.lnk\"".into(),
            args: Vec::new(),
        };
        let spec = command_for(&action).expect("valid action");
        assert_eq!(spec.program, OsString::from("explorer"));
        assert_eq!(
            spec.args[0],
            OsString::from("C:\\ProgramData\\Start Menu\\Programs\\Brave.lnk")
        );
    }

    #[test]
    fn a_quoted_url_loses_its_quotes_before_it_opens() {
        let action = Action::OpenUrl {
            url: "\"https://example.com\"".to_string(),
        };
        let spec = command_for(&action).expect("valid action");
        assert!(spec.args.iter().any(|arg| arg == "https://example.com"));
    }

    /// A program that cannot run exits on its own, and that has to be
    /// reported: the spawn succeeded, so nothing else would say so.
    #[cfg(windows)]
    #[test]
    fn a_program_that_dies_on_its_own_is_reported() {
        let runner = ActionRunner::new();
        runner
            .submit(Action::Launch {
                program: PathBuf::from("C:\\Windows\\System32\\cmd.exe"),
                args: vec!["/C".to_string(), "exit 3".to_string()],
            })
            .expect("submitted");

        let failure = (0..100).find_map(|_| {
            std::thread::sleep(std::time::Duration::from_millis(50));
            runner.take_failure()
        });
        assert!(
            failure
                .as_deref()
                .is_some_and(|message| message.contains("exited with code 3")),
            "the failure should name the exit code: {failure:?}"
        );
    }

    #[cfg(windows)]
    #[test]
    fn a_program_that_succeeds_is_not_reported() {
        let runner = ActionRunner::new();
        runner
            .submit(Action::Launch {
                program: PathBuf::from("C:\\Windows\\System32\\cmd.exe"),
                args: vec!["/C".to_string(), "exit 0".to_string()],
            })
            .expect("submitted");

        std::thread::sleep(std::time::Duration::from_millis(800));
        assert!(
            runner.take_failure().is_none(),
            "a program that worked is not a failure"
        );
    }

    #[cfg(windows)]
    #[test]
    fn a_program_that_keeps_running_is_not_reported() {
        // A short watch window: the program outlives it, which is exactly
        // what "it worked" looks like.
        let runner = ActionRunner::with_exit_watch(std::time::Duration::from_millis(400));
        runner
            .submit(Action::Launch {
                program: PathBuf::from("C:\\Windows\\System32\\cmd.exe"),
                args: vec!["/C".to_string(), "ping -n 4 127.0.0.1 > nul".to_string()],
            })
            .expect("submitted");

        std::thread::sleep(std::time::Duration::from_millis(1200));
        assert!(
            runner.take_failure().is_none(),
            "a program that is still running is not a failure"
        );
    }

    #[test]
    fn a_missing_program_reports_the_failure() {
        let error = run_action(&missing_program()).expect_err("must fail");
        assert!(matches!(error, ActionError::Launch { .. }));
    }

    #[cfg(windows)]
    #[test]
    fn a_batch_file_goes_through_cmd() {
        let action = Action::Launch {
            program: PathBuf::from("C:\\tools\\thing.bat"),
            args: vec!["now".to_string()],
        };
        let spec = command_for(&action).expect("valid action");
        assert_eq!(spec.program, OsString::from("cmd"));
        assert_eq!(spec.args[0], OsString::from("/C"));
        assert_eq!(spec.args[1], OsString::from("call"));
        assert_eq!(spec.args[2], OsString::from("C:\\tools\\thing.bat"));
        assert_eq!(spec.args[3], OsString::from("now"));
    }

    /// A batch file has to really run, arguments and all, even when its path
    /// carries a space — that is where `cmd /C` alone falls apart.
    #[cfg(windows)]
    #[test]
    fn a_batch_file_with_spaces_in_its_path_runs_with_its_arguments() {
        let dir = tempfile::tempdir().expect("temp dir");
        let folder = dir.path().join("with space");
        std::fs::create_dir_all(&folder).expect("folder");
        let script = folder.join("run script.bat");
        std::fs::write(&script, "@echo off\r\necho [%1] [%2]>\"%~dp0out.txt\"\r\n")
            .expect("write script");
        let out = folder.join("out.txt");

        run_action(&Action::Launch {
            program: script,
            args: vec!["first arg".to_string(), "second".to_string()],
        })
        .expect("the script must start");

        let written = (0..100).any(|_| {
            std::thread::sleep(std::time::Duration::from_millis(50));
            out.exists()
        });
        assert!(written, "the script never wrote {}", out.display());

        let content = std::fs::read_to_string(&out).expect("read output");
        assert!(
            content.contains("first arg") && content.contains("second"),
            "the script did not get its arguments: {content:?}"
        );
    }

    /// The exit code of a batch file has to survive `cmd /C call`, otherwise
    /// a failing script would look like a working one.
    #[cfg(windows)]
    #[test]
    fn a_batch_file_that_fails_keeps_its_exit_code() {
        let dir = tempfile::tempdir().expect("temp dir");
        let script = dir.path().join("fail.bat");
        std::fs::write(&script, "@echo off\r\nexit /b 7\r\n").expect("write script");

        let runner = ActionRunner::new();
        runner
            .submit(Action::Launch {
                program: script,
                args: Vec::new(),
            })
            .expect("submitted");

        let failure = (0..100).find_map(|_| {
            std::thread::sleep(std::time::Duration::from_millis(50));
            runner.take_failure()
        });
        assert!(
            failure
                .as_deref()
                .is_some_and(|message| message.contains("exited with code 7")),
            "the script's exit code must survive: {failure:?}"
        );
    }

    #[cfg(windows)]
    #[test]
    fn a_shortcut_goes_through_explorer() {
        let action = Action::Launch {
            program: PathBuf::from("C:\\Users\\me\\Desktop\\App.lnk"),
            args: Vec::new(),
        };
        let spec = command_for(&action).expect("valid action");
        assert_eq!(spec.program, OsString::from("explorer"));
        assert_eq!(
            spec.args[0],
            OsString::from("C:\\Users\\me\\Desktop\\App.lnk")
        );
    }

    #[test]
    fn the_runner_reports_a_failing_action() {
        let runner = ActionRunner::new();
        runner.submit(missing_program()).expect("submitted");
        let failure = (0..100).find_map(|_| {
            std::thread::sleep(std::time::Duration::from_millis(20));
            runner.take_failure()
        });
        assert!(
            failure
                .as_deref()
                .is_some_and(|message| message.contains("ss550-no-such-program-8f3a.exe")),
            "the failure should name the program: {failure:?}"
        );
    }

    #[test]
    fn the_runner_refuses_a_broken_action() {
        let runner = ActionRunner::new();
        let broken = Action::OpenUrl {
            url: "not a url".to_string(),
        };
        assert!(runner.submit(broken).is_err());
    }

    /// A valid program really has to start, not just produce a spec.
    ///
    /// `cmd /C echo` is the harmless program used here: it writes a file and
    /// exits, so the test neither opens a window nor depends on a desktop.
    ///
    /// The redirect target is left unquoted on purpose: `tempfile` paths carry
    /// no spaces, and `cmd.exe` does not understand the `\"` escaping that
    /// `Command` applies to an argument containing quotes.
    #[cfg(windows)]
    #[test]
    fn a_valid_program_actually_starts() {
        let dir = tempfile::tempdir().expect("temp dir");
        let stamp = dir.path().join("started.txt");

        let action = Action::Launch {
            program: PathBuf::from("C:\\Windows\\System32\\cmd.exe"),
            args: vec![
                "/C".to_string(),
                format!("echo started> {}", stamp.display()),
            ],
        };

        run_action(&action).expect("a real program must start");

        let started = (0..100).any(|_| {
            std::thread::sleep(std::time::Duration::from_millis(50));
            stamp.exists()
        });
        assert!(
            started,
            "the program never wrote {}; the spawn produced no process",
            stamp.display()
        );
    }

    #[test]
    fn the_runner_starts_alive() {
        let runner = ActionRunner::new();
        assert!(runner.is_alive());
        assert!(runner.take_failure().is_none());
    }

    /// A sender that records what it was asked to send, and can be told to fail.
    struct Recorder {
        sent: Mutex<Vec<String>>,
        failure: Option<String>,
    }

    impl Recorder {
        fn sending() -> Arc<Self> {
            Arc::new(Self {
                sent: Mutex::new(Vec::new()),
                failure: None,
            })
        }

        fn failing(reason: &str) -> Arc<Self> {
            Arc::new(Self {
                sent: Mutex::new(Vec::new()),
                failure: Some(reason.to_string()),
            })
        }

        fn sent(&self) -> Vec<String> {
            self.sent
                .lock()
                .map(|sent| sent.clone())
                .unwrap_or_default()
        }
    }

    impl KeySender for Recorder {
        fn send(&self, combo: &KeyCombo) -> Result<(), String> {
            if let Some(reason) = &self.failure {
                return Err(reason.clone());
            }
            if let Ok(mut sent) = self.sent.lock() {
                sent.push(combo.to_string());
            }
            Ok(())
        }
    }

    fn combo(text: &str) -> KeyCombo {
        KeyCombo::parse(text).expect("valid combination")
    }

    /// The whole point of the seam: a runner with nowhere to send must refuse
    /// a keystroke on the spot, never accept it and report nothing later.
    #[test]
    fn a_combination_without_a_sender_is_refused_on_the_spot() {
        let runner = ActionRunner::new();
        let error = runner
            .submit(Action::Keys {
                keys: combo("ctrl+shift+m"),
            })
            .expect_err("a runner with no sender must refuse the action");

        assert!(
            matches!(error, ActionError::NeedsKeySender { .. }),
            "{error}"
        );
        assert!(
            runner.take_failure().is_none(),
            "nothing was queued, so nothing can fail later either"
        );
    }

    #[test]
    fn a_submitted_combination_reaches_the_sender() {
        let recorder = Recorder::sending();
        let sender: Arc<dyn KeySender> = recorder.clone();
        let runner = ActionRunner::with_key_sender(sender);

        runner
            .submit(Action::Keys {
                keys: combo("ctrl+shift+m"),
            })
            .expect("submitted");

        let delivered = (0..200).any(|_| {
            std::thread::sleep(std::time::Duration::from_millis(10));
            !recorder.sent().is_empty()
        });
        assert!(delivered, "the sender was never asked to send");
        assert_eq!(recorder.sent(), vec!["Ctrl+Shift+M".to_string()]);
        assert!(
            runner.take_failure().is_none(),
            "a delivered key is not a failure"
        );
    }

    #[test]
    fn send_keys_goes_through_the_same_sender() {
        let recorder = Recorder::sending();
        let sender: Arc<dyn KeySender> = recorder.clone();
        let runner = ActionRunner::with_key_sender(sender);

        runner.send_keys(combo("win+l")).expect("queued");

        let delivered = (0..200).any(|_| {
            std::thread::sleep(std::time::Duration::from_millis(10));
            !recorder.sent().is_empty()
        });
        assert!(delivered, "the sender was never asked to send");
        assert_eq!(recorder.sent(), vec!["Win+L".to_string()]);
    }

    #[test]
    fn send_keys_without_a_sender_is_refused() {
        let runner = ActionRunner::new();
        let error = runner.send_keys(combo("alt+tab")).expect_err("no sender");

        assert!(
            matches!(error, ActionError::NeedsKeySender { .. }),
            "{error}"
        );
    }

    #[test]
    fn a_send_the_platform_refused_is_reported() {
        let sender: Arc<dyn KeySender> = Recorder::failing("the window is not listening");
        let runner = ActionRunner::with_key_sender(sender);

        runner
            .submit(Action::Keys {
                keys: combo("win+l"),
            })
            .expect("queued");

        let failure = (0..200)
            .find_map(|_| {
                std::thread::sleep(std::time::Duration::from_millis(10));
                runner.take_failure()
            })
            .expect("a refused send must be reported");

        assert!(
            failure.contains("Win+L"),
            "the failure names the combination: {failure}"
        );
        assert!(
            failure.contains("the window is not listening"),
            "and the reason it was refused: {failure}"
        );
    }

    /// A combination is sent, not started: neither path that starts a process
    /// may quietly do nothing and return success.
    #[test]
    fn a_combination_is_never_started_as_a_program() {
        let action = Action::Keys {
            keys: combo("ctrl+c"),
        };

        assert!(
            matches!(
                command_for(&action),
                Err(ActionError::KeysNotAProgram { .. })
            ),
            "a combination has no command"
        );
        assert!(
            matches!(
                run_action(&action),
                Err(ActionError::KeysNotAProgram { .. })
            ),
            "a combination has no process to start"
        );
    }
}
