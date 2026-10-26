//! Levelled, coloured console logging.
//!
//! The Scala bot elides logging at compile time (`compileTimePriority`, 0 by default) and
//! filters the rest at runtime. Here the `verbose-logs` feature plays the compile-time role;
//! without it the macros still type-check their arguments but never evaluate them.

use std::sync::atomic::{AtomicU8, Ordering};

pub const WHITE: &str = "\x1b[37m";
pub const MAGENTA: &str = "\x1b[35m";
pub const CYAN: &str = "\x1b[36m";
pub const RESET: &str = "\x1b[0m";

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum LogLevel {
    Off = 0,
    Error = 1,
    Warn = 2,
    Info = 3,
}

const COMPILE_TIME_LEVEL: LogLevel = if cfg!(feature = "verbose-logs") {
    LogLevel::Info
} else {
    LogLevel::Off
};

static RUNTIME_LEVEL: AtomicU8 = AtomicU8::new(LogLevel::Info as u8);

pub fn set_level(level: LogLevel) {
    RUNTIME_LEVEL.store(level as u8, Ordering::Relaxed);
}

pub fn level() -> LogLevel {
    match RUNTIME_LEVEL.load(Ordering::Relaxed) {
        0 => LogLevel::Off,
        1 => LogLevel::Error,
        2 => LogLevel::Warn,
        _ => LogLevel::Info,
    }
}

#[doc(hidden)]
#[inline]
pub fn enabled(level: LogLevel) -> bool {
    level <= COMPILE_TIME_LEVEL && level as u8 <= RUNTIME_LEVEL.load(Ordering::Relaxed)
}

/// Logs at a level and colour. Arguments are only evaluated if the level is enabled.
#[macro_export]
macro_rules! log_at {
    ($level:expr, $colour:expr, $($arg:tt)*) => {
        if $crate::logger::enabled($level) {
            println!("{}{}{}", $colour, format_args!($($arg)*), $crate::logger::RESET);
        }
    };
}

#[macro_export]
macro_rules! log_error {
    ($($arg:tt)*) => {
        $crate::log_at!($crate::logger::LogLevel::Error, $crate::logger::MAGENTA, $($arg)*)
    };
}

#[macro_export]
macro_rules! log_warn {
    ($($arg:tt)*) => {
        $crate::log_at!($crate::logger::LogLevel::Warn, $crate::logger::CYAN, $($arg)*)
    };
}

#[macro_export]
macro_rules! log_info {
    ($($arg:tt)*) => {
        $crate::log_at!($crate::logger::LogLevel::Info, $crate::logger::WHITE, $($arg)*)
    };
}

/// Info-level log in a chosen colour.
#[macro_export]
macro_rules! log_highlight {
    ($colour:expr, $($arg:tt)*) => {
        $crate::log_at!($crate::logger::LogLevel::Info, $colour, $($arg)*)
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_are_ordered() {
        assert!(LogLevel::Off < LogLevel::Error);
        assert!(LogLevel::Error < LogLevel::Warn);
        assert!(LogLevel::Warn < LogLevel::Info);
    }

    #[test]
    fn disabled_logs_do_not_evaluate_arguments() {
        // Runs with the default features, where compile-time logging is off.
        if cfg!(feature = "verbose-logs") {
            return;
        }
        let mut evaluated = false;
        log_info!("{}", {
            evaluated = true;
            1
        });
        assert!(!evaluated);
    }
}
