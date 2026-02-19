//! Network registration handling for nRF91 modems.
//!
//! This module provides CEREG (network registration) notification handling
//! using a signal-based pattern for async notification of registration changes.

#![allow(dead_code)]

use embassy_net_nrf91::Control;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::signal::Signal;

/// Network registration status from +CEREG responses.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RegistrationStatus {
	/// Not registered, MT is not currently searching for a network
	NotRegistered = 0,
	/// Registered, home network
	RegisteredHome = 1,
	/// Not registered, MT is currently searching for a network
	Searching = 2,
	/// Registration denied
	Denied = 3,
	/// Unknown (e.g., out of range)
	Unknown = 4,
	/// Registered, roaming
	RegisteredRoaming = 5,
}

impl RegistrationStatus {
	/// Parse registration status from numeric value.
	pub fn from_u8(val: u8) -> Self {
		match val {
			0 => Self::NotRegistered,
			1 => Self::RegisteredHome,
			2 => Self::Searching,
			3 => Self::Denied,
			5 => Self::RegisteredRoaming,
			_ => Self::Unknown,
		}
	}

	/// Check if this status represents a successful network registration.
	pub fn is_registered(self) -> bool {
		matches!(self, Self::RegisteredHome | Self::RegisteredRoaming)
	}

	/// Get a human-readable description of the status.
	pub fn as_str(self) -> &'static str {
		match self {
			Self::NotRegistered => "Not registered",
			Self::RegisteredHome => "Registered (home network)",
			Self::Searching => "Searching...",
			Self::Denied => "Registration denied",
			Self::Unknown => "Unknown",
			Self::RegisteredRoaming => "Registered (roaming)",
		}
	}
}

/// Global signal for CEREG registration status changes.
///
/// The monitor task signals this when registration status changes,
/// allowing other tasks to await registration events.
pub static REGISTRATION_SIGNAL: Signal<CriticalSectionRawMutex, RegistrationStatus> = Signal::new();

/// Parse +CEREG response to extract registration status.
///
/// Handles both query response format: `+CEREG: <n>,<stat>[,<tac>,<ci>,<AcT>]`
/// and URC format: `+CEREG: <stat>[,<tac>,<ci>,<AcT>]`
pub fn parse_cereg_response(response: &[u8]) -> Option<RegistrationStatus> {
	let resp_str = core::str::from_utf8(response).ok()?;

	// Find +CEREG: in the response
	let cereg_pos = resp_str.find("+CEREG:")?;
	let after_cereg = &resp_str[cereg_pos + 7..]; // Skip "+CEREG:"

	// Skip whitespace
	let trimmed = after_cereg.trim_start();

	// Parse the numbers - could be "<n>,<stat>" or just "<stat>" for URC
	let mut parts = trimmed.split(',');
	let first = parts.next()?.trim();

	// If there's a second part, first is <n> and second is <stat>
	// If only one part, it's the <stat> (URC format)
	let stat_str = if let Some(second) = parts.next() {
		second.split_whitespace().next().unwrap_or(second.trim())
	} else {
		first.split_whitespace().next().unwrap_or(first)
	};

	let stat: u8 = stat_str.parse().ok()?;
	Some(RegistrationStatus::from_u8(stat))
}

/// Registration monitor that tracks CEREG status and signals on changes.
pub struct RegistrationMonitor {
	last_status: RegistrationStatus,
}

impl RegistrationMonitor {
	/// Create a new registration monitor.
	pub fn new() -> Self {
		Self {
			last_status: RegistrationStatus::Unknown,
		}
	}

	/// Enable CEREG unsolicited result codes on the modem.
	///
	/// Sends AT+CEREG=2 to enable URCs with location information.
	pub async fn enable_urcs(&self, control: &Control<'_>) {
		let mut resp_buf = [0u8; 128];
		let _ = control.at_command(b"AT+CEREG=2", &mut resp_buf).await;
	}

	/// Query current registration status and signal if changed.
	///
	/// Returns the current status.
	pub async fn query_status(&mut self, control: &Control<'_>) -> RegistrationStatus {
		let mut resp_buf = [0u8; 256];
		let len = control.at_command(b"AT+CEREG?", &mut resp_buf).await;

		if len > 0 {
			if let Some(status) = parse_cereg_response(&resp_buf[..len]) {
				if status != self.last_status {
					self.last_status = status;
					REGISTRATION_SIGNAL.signal(status);
				}
				return status;
			}
		}

		self.last_status
	}

	/// Get the last known registration status.
	pub fn last_status(&self) -> RegistrationStatus {
		self.last_status
	}
}

impl Default for RegistrationMonitor {
	fn default() -> Self {
		Self::new()
	}
}

/// Wait for the network to become registered.
///
/// This async function blocks until the modem reports either
/// `RegisteredHome` or `RegisteredRoaming` status.
///
/// Returns the registration status that caused the function to return.
pub async fn wait_for_registration() -> RegistrationStatus {
	loop {
		let status = REGISTRATION_SIGNAL.wait().await;
		if status.is_registered() {
			return status;
		}
	}
}

/// Wait for any registration status change.
///
/// Returns the new status when it changes.
pub async fn wait_for_status_change() -> RegistrationStatus {
	REGISTRATION_SIGNAL.wait().await
}
