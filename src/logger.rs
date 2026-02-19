//! UART logging utilities for embedded applications.
//!
//! Provides macros and utilities for logging over UART.

/// Log a formatted message over UART.
///
/// # Example
/// ```ignore
/// log!(uart, "Hello, {}!", "world");
/// log!(uart, "Counter: {}", 42);
/// ```
#[macro_export]
macro_rules! log {
	($uart:expr, $($arg:tt)*) => {{
		use core::fmt::Write as _;
		let mut buf: heapless::String<256> = heapless::String::new();
		let _ = core::write!(&mut buf, $($arg)*);
		let _ = buf.push_str("\r\n");
		let _ = $uart.write(buf.as_bytes()).await;
	}};
}

/// Log an AT command exchange (command sent and response received).
///
/// # Example
/// ```ignore
/// log_at!(uart, "AT+CFUN?", response_str);
/// ```
#[macro_export]
macro_rules! log_at {
	($uart:expr, $cmd:expr, $resp:expr) => {{
		use core::fmt::Write as _;
		let mut buf: heapless::String<256> = heapless::String::new();
		let _ = core::write!(&mut buf, ">> {}", $cmd);
		let _ = buf.push_str("\r\n");
		let _ = $uart.write(buf.as_bytes()).await;

		let mut buf: heapless::String<256> = heapless::String::new();
		let _ = core::write!(&mut buf, "<< {}", $resp);
		let _ = buf.push_str("\r\n");
		let _ = $uart.write(buf.as_bytes()).await;
	}};
}

/// Send AT command via modem control and log the exchange.
///
/// # Arguments
/// * `control` - Modem control reference
/// * `uart` - UART interface for logging
/// * `cmd` - AT command string
///
/// # Example
/// ```ignore
/// send_at_logged!(control, uart, "AT+CFUN?");
/// ```
#[macro_export]
macro_rules! send_at_logged {
	($control:expr, $uart:expr, $cmd:expr) => {{
		use core::fmt::Write as _;

		// Log command
		let mut buf: heapless::String<256> = heapless::String::new();
		let _ = core::write!(&mut buf, ">> {}", $cmd);
		let _ = buf.push_str("\r\n");
		let _ = $uart.write(buf.as_bytes()).await;

		// Send command
		let mut resp_buf = [0u8; 256];
		let len = $control.at_command($cmd.as_bytes(), &mut resp_buf).await;

		// Log response
		if len > 0 {
			if let Ok(resp_str) = core::str::from_utf8(&resp_buf[..len]) {
				let mut buf: heapless::String<256> = heapless::String::new();
				let _ = core::write!(&mut buf, "<< {}", resp_str.trim());
				let _ = buf.push_str("\r\n");
				let _ = $uart.write(buf.as_bytes()).await;
			}
		}

		len
	}};
}
