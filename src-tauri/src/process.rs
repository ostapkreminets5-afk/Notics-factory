use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const POLL_INTERVAL: Duration = Duration::from_millis(200);
const DRAIN_GRACE: Duration = Duration::from_secs(5);
const ESCAPE: char = '\u{1b}';
const BELL: char = '\u{7}';

pub enum RunFailure {
    Spawn(std::io::Error),
    Cancelled,
    TimedOut,
    Wait(std::io::Error),
}

pub type LineSink = Arc<dyn Fn(String) + Send + Sync>;

pub fn hidden(command: &mut Command) -> &mut Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    command
}

pub fn capture(program: &Path, args: &[&str], timeout: Duration) -> Option<String> {
    let mut command = Command::new(program);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    hidden(&mut command);
    let mut child = command.spawn().ok()?;
    let stdout = collect(child.stdout.take());
    let stderr = collect(child.stderr.take());
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() < deadline => thread::sleep(POLL_INTERVAL),
            _ => {
                kill_tree(&mut child);
                return None;
            }
        }
    }
    let mut text = stdout.join().unwrap_or_default();
    text.push_str(&stderr.join().unwrap_or_default());
    Some(text)
}

pub fn stream(
    mut command: Command,
    input: Option<String>,
    on_line: LineSink,
    on_spawn: impl FnOnce(u32),
    cancelled: impl Fn() -> bool,
    timeout: Duration,
) -> Result<ExitStatus, RunFailure> {
    command
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    hidden(&mut command);
    let mut child = command.spawn().map_err(RunFailure::Spawn)?;
    on_spawn(child.id());
    let feeder = match (input, child.stdin.take()) {
        (Some(text), Some(mut stdin)) => Some(thread::spawn(move || {
            let _ = stdin.write_all(text.as_bytes());
        })),
        _ => None,
    };
    let readers = [
        forward(child.stdout.take(), Arc::clone(&on_line)),
        forward(child.stderr.take(), on_line),
    ];
    let deadline = Instant::now() + timeout;
    let outcome = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) => {}
            Err(error) => break Err(RunFailure::Wait(error)),
        }
        if cancelled() {
            kill_tree(&mut child);
            break Err(RunFailure::Cancelled);
        }
        if Instant::now() >= deadline {
            kill_tree(&mut child);
            break Err(RunFailure::TimedOut);
        }
        thread::sleep(POLL_INTERVAL);
    };
    let drained_by = Instant::now() + DRAIN_GRACE;
    for reader in readers.into_iter().flatten() {
        while !reader.is_finished() && Instant::now() < drained_by {
            thread::sleep(POLL_INTERVAL);
        }
        if reader.is_finished() {
            let _ = reader.join();
        }
    }
    if let Some(feeder) = feeder.filter(|feeder| feeder.is_finished()) {
        let _ = feeder.join();
    }
    outcome
}

pub fn kill_pid(pid: u32) {
    #[cfg(windows)]
    let mut command = {
        let mut command = Command::new("taskkill");
        command.args(["/PID", &pid.to_string(), "/T", "/F"]);
        command
    };
    #[cfg(unix)]
    let mut command = {
        let mut command = Command::new("kill");
        command.args(["-KILL", &format!("-{pid}")]);
        command
    };
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    hidden(&mut command);
    let _ = command.status();
}

fn kill_tree(child: &mut Child) {
    kill_pid(child.id());
    let _ = child.kill();
    let _ = child.wait();
}

fn collect<R: Read + Send + 'static>(stream: Option<R>) -> JoinHandle<String> {
    thread::spawn(move || {
        let mut bytes = Vec::new();
        if let Some(mut stream) = stream {
            let _ = stream.read_to_end(&mut bytes);
        }
        String::from_utf8_lossy(&bytes).into_owned()
    })
}

fn forward<R: Read + Send + 'static>(stream: Option<R>, on_line: LineSink) -> Option<JoinHandle<()>> {
    let stream = stream?;
    Some(thread::spawn(move || {
        let mut reader = BufReader::new(stream);
        let mut bytes = Vec::new();
        loop {
            bytes.clear();
            match reader.read_until(b'\n', &mut bytes) {
                Ok(0) | Err(_) => return,
                Ok(_) => {
                    let text = String::from_utf8_lossy(&bytes);
                    for piece in text.split(['\r', '\n']) {
                        let line = strip_ansi(piece);
                        if !line.trim().is_empty() {
                            on_line(line.trim_end().to_owned());
                        }
                    }
                }
            }
        }
    }))
}

pub fn strip_ansi(text: &str) -> String {
    let mut clean = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c != ESCAPE {
            clean.push(c);
            continue;
        }
        match chars.next() {
            Some('[') => {
                for code in chars.by_ref() {
                    if ('@'..='~').contains(&code) {
                        break;
                    }
                }
            }
            Some(']') => {
                while let Some(code) = chars.next() {
                    if code == BELL {
                        break;
                    }
                    if code == ESCAPE && chars.peek() == Some(&'\\') {
                        chars.next();
                        break;
                    }
                }
            }
            _ => {}
        }
    }
    clean
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removes_terminal_colors_and_titles() {
        assert_eq!(strip_ansi("\u{1b}[32mok\u{1b}[0m done"), "ok done");
        assert_eq!(strip_ansi("\u{1b}]0;title\u{7}build"), "build");
        assert_eq!(strip_ansi("plain"), "plain");
    }
}
