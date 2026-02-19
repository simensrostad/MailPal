//! Error handling for nRF9151 modem application.
//!
//! Provides error types and fatal error handling inspired by the Nordic
//! Asset Tracker Template pattern: log error, then halt/panic.

use core::fmt;

/// Application error type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub enum Error {
	/// Modem initialization failed
	ModemInit,
	/// AT command failed or returned ERROR
	AtCommand,
	/// Network registration failed
	Registration,
	/// PDP context activation failed
	PdpActivation,
	/// Network stack initialization failed
	NetworkInit,
	/// TCP/IP socket error
	Socket,
	/// Timeout waiting for operation
	Timeout,
	/// Invalid response from modem
	InvalidResponse,
	/// Task spawn failed
	TaskSpawn,
	/// Configuration error
	Config,
}

impl fmt::Display for Error {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		match self {
			Error::ModemInit => write!(f, "Modem initialization failed"),
			Error::AtCommand => write!(f, "AT command failed"),
			Error::Registration => write!(f, "Network registration failed"),
			Error::PdpActivation => write!(f, "PDP context activation failed"),
			Error::NetworkInit => write!(f, "Network stack initialization failed"),
			Error::Socket => write!(f, "Socket error"),
			Error::Timeout => write!(f, "Operation timed out"),
			Error::InvalidResponse => write!(f, "Invalid response from modem"),
			Error::TaskSpawn => write!(f, "Failed to spawn task"),
			Error::Config => write!(f, "Configuration error"),
		}
	}
}

/// Result type alias for this application.
pub type Result<T> = core::result::Result<T, Error>;

/// Halt the application with a fatal error.
///
/// This function logs the error location and halts the CPU in an infinite
/// loop. In debug builds, it will panic to show the backtrace.
///
/// # Safety
/// This function never returns.
#[inline(never)]
#[cold]
pub fn fatal_error(file: &str, line: u32, msg: &str) -> ! {
	// In a real implementation, you might want to:
	// - Log to persistent storage
	// - Trigger a watchdog reset
	// - Send error telemetry
	// For now, we panic which will be caught by panic_halt
	panic!("FATAL ERROR at {}:{}: {}", file, line, msg);
}

/// Macro to trigger a fatal error with file/line information.
///
/// Usage:
/// ```ignore
/// fatal_error!("Modem initialization failed");
/// ```
///
/// This will log the error and halt the application, similar to
/// SEND_FATAL_ERROR() in the Nordic C SDK.
#[macro_export]
macro_rules! fatal_error {
	($msg:expr) => {
		$crate::error::fatal_error(file!(), line!(), $msg)
	};
	($fmt:expr, $($arg:tt)*) => {
		$crate::error::fatal_error(
			file!(),
			line!(),
			&alloc::format!($fmt, $($arg)*)
		)
	};
}

/// Macro to check a Result and trigger fatal error on Err.
///
/// Usage:
/// ```ignore
/// let value = check_fatal!(some_operation(), "Operation failed");
/// ```
///
/// On error, logs the error and halts the application.
#[macro_export]
macro_rules! check_fatal {
	($result:expr, $msg:expr) => {
		match $result {
			Ok(val) => val,
			Err(e) => {
				$crate::fatal_error!(concat!($msg, ": {:?}"), e)
			}
		}
	};
}

/// Macro to assert a condition and trigger fatal error if false.
///
/// Usage:
/// ```ignore
/// assert_fatal!(condition, "Condition check failed");
/// ```
#[macro_export]
macro_rules! assert_fatal {
	($cond:expr, $msg:expr) => {
		if !$cond {
			$crate::fatal_error!($msg)
		}
	};
}
