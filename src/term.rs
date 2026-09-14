use std::io::IsTerminal;

pub fn green(s: &str) -> String {
    paint(s, "\x1b[32m", true)
}

pub fn gray(s: &str) -> String {
    paint(s, "\x1b[90m", true)
}

pub fn red(s: &str) -> String {
    paint(s, "\x1b[31m", false)
}

fn paint(s: &str, code: &str, stdout: bool) -> String {
    let tty = if stdout {
        std::io::stdout().is_terminal()
    } else {
        std::io::stderr().is_terminal()
    };
    if tty {
        format!("{code}{s}\x1b[0m")
    } else {
        s.to_string()
    }
}