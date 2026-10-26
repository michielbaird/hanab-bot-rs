//! Levelled, coloured console logging.
//!
//! The Scala bot elides logging at compile time (`compileTimePriority`, 0 by default) and
//! filters the rest at runtime. Here the `verbose-logs` feature plays the compile-time role;
//! without it the macros still type-check their arguments but never evaluate them.

use std::cell::Cell;
use std::sync::atomic::{AtomicU8, Ordering};

pub const WHITE: &str = "\x1b[37m";
pub const MAGENTA: &str = "\x1b[35m";
pub const CYAN: &str = "\x1b[36m";
pub const GREEN: &str = "\x1b[32m";
pub const YELLOW: &str = "\x1b[33m";
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

/// The process-wide log level, used by threads that haven't set their own.
static DEFAULT_LEVEL: AtomicU8 = AtomicU8::new(LogLevel::Info as u8);

thread_local! {
    /// This thread's log level, if it has overridden the default.
    static THREAD_LEVEL: Cell<Option<LogLevel>> = const { Cell::new(None) };
}

fn from_u8(n: u8) -> LogLevel {
    match n {
        0 => LogLevel::Off,
        1 => LogLevel::Error,
        2 => LogLevel::Warn,
        _ => LogLevel::Info,
    }
}

/// Sets the log level for the whole process (threads that haven't called [`set_level`]).
pub fn set_default_level(level: LogLevel) {
    DEFAULT_LEVEL.store(level as u8, Ordering::Relaxed);
}

/// Sets the log level for the current thread only.
///
/// Per-thread so that several bots (each deciding on its own thread) can temporarily quieten
/// their hypothetical games without affecting one another.
pub fn set_level(level: LogLevel) {
    THREAD_LEVEL.with(|l| l.set(Some(level)));
}

pub fn level() -> LogLevel {
    THREAD_LEVEL
        .with(Cell::get)
        .unwrap_or_else(|| from_u8(DEFAULT_LEVEL.load(Ordering::Relaxed)))
}

/// Sets the runtime log level of the current thread until dropped, then restores what it was.
///
/// Replaces the Scala pattern of `val level = Logger.level; Logger.setLevel(..); ...; Logger.setLevel(level)`.
pub struct LevelGuard(Option<LogLevel>);

impl LevelGuard {
    /// Lowers the level to at most `level`.
    pub fn at_most(level: LogLevel) -> Self {
        let prev = THREAD_LEVEL.with(Cell::get);
        set_level(self::level().min(level));
        Self(prev)
    }

    /// Sets the level to exactly `level`.
    pub fn set(level: LogLevel) -> Self {
        let prev = THREAD_LEVEL.with(Cell::get);
        set_level(level);
        Self(prev)
    }
}

impl Drop for LevelGuard {
    fn drop(&mut self) {
        THREAD_LEVEL.with(|l| l.set(self.0));
    }
}

#[doc(hidden)]
#[inline]
pub fn enabled(level: LogLevel) -> bool {
    level <= COMPILE_TIME_LEVEL && level <= self::level()
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

    #[test]
    fn levels_are_per_thread() {
        let a = std::thread::spawn(|| {
            set_level(LogLevel::Off);
            let guard = LevelGuard::set(LogLevel::Error);
            // Another thread changing its level and guards doesn't affect this one.
            std::thread::spawn(|| {
                let _quiet = LevelGuard::at_most(LogLevel::Warn);
                set_level(LogLevel::Info);
            })
            .join()
            .unwrap();
            assert_eq!(level(), LogLevel::Error);
            drop(guard);
            level()
        });
        assert_eq!(a.join().unwrap(), LogLevel::Off);
    }

    #[test]
    fn guards_restore_an_unset_level_to_the_default() {
        std::thread::spawn(|| {
            let default = level();
            {
                let _g = LevelGuard::at_most(LogLevel::Off);
                assert_eq!(level(), LogLevel::Off);
            }
            assert_eq!(level(), default);
        })
        .join()
        .unwrap();
    }
}
