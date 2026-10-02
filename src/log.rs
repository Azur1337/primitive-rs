//! Logging with a global verbosity level.
//!
//! Levels: 0 = silent, 1 = `v`, 2 = `vv`, 3 = `vvv`.

use std::sync::atomic::{AtomicI32, Ordering};

/// Global log level (0 = silent, 1 = v, 2 = vv, 3 = vvv).
pub static LOG_LEVEL: AtomicI32 = AtomicI32::new(0);

/// Set the global log level.
pub fn set_log_level(level: i32) {
    LOG_LEVEL.store(level, Ordering::SeqCst);
}

/// Substitute `{}` placeholders in `format` with the provided args.
fn render(format: &str, args: &[&str]) -> String {
    let mut result = String::new();
    let mut arg_iter = args.iter();
    let mut chars = format.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '{' && chars.peek() == Some(&'}') {
            chars.next();
            match arg_iter.next() {
                Some(a) => result.push_str(a),
                None => result.push_str("{}"),
            }
        } else {
            result.push(c);
        }
    }
    result
}

/// Print `format` (with `{}` args) when the level is at least `level`.
pub fn log(level: i32, format: &str, args: &[&str]) {
    if LOG_LEVEL.load(Ordering::SeqCst) >= level {
        println!("{}", render(format, args));
    }
}

/// Print at level >= 1.
pub fn v(format: &str, args: &[&str]) {
    log(1, format, args);
}

/// Print at level >= 2, indented by two spaces.
pub fn vv(format: &str, args: &[&str]) {
    let prefixed = format!("  {}", format);
    log(2, &prefixed, args);
}

/// Print at level >= 3, indented by four spaces.
pub fn vvv(format: &str, args: &[&str]) {
    let prefixed = format!("    {}", format);
    log(3, &prefixed, args);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_and_get_level() {
        let original = LOG_LEVEL.load(Ordering::SeqCst);
        set_log_level(2);
        assert_eq!(LOG_LEVEL.load(Ordering::SeqCst), 2);
        set_log_level(original);
    }

    #[test]
    fn render_substitutes_placeholders() {
        assert_eq!(render("a {} c", &["b"]), "a b c");
        assert_eq!(render("{} {} {}", &["1", "2", "3"]), "1 2 3");
        assert_eq!(render("no args", &[]), "no args");
    }
}
