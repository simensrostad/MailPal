//! PDP context management for cellular networking.
//!
//! This module handles PDP (Packet Data Protocol) context activation
//! which is required for IP connectivity over cellular networks.
//!
//! ## Error Handling
//! Functions return `Result<T, Error>` where errors should be handled
//! by the caller. For fatal errors, use the `fatal_error!` macro.

#![allow(dead_code)]

use crate::error::{Error, Result};

use embassy_net::{ConfigV4, Ipv4Address, Ipv4Cidr, Stack, StaticConfigV4};
use embassy_net_nrf91::Control;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::signal::Signal;

/// Signal for PDP context status changes.
pub static PDP_STATUS_SIGNAL: Signal<CriticalSectionRawMutex, PdpStatus> = Signal::new();

/// PDP context status.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PdpStatus {
	/// Context is deactivated
	Deactivated,
	/// Context is activated with IP address
	Activated { ip: Ipv4Address },
}

/// Activate PDP context (data connection).
///
/// For nRF91, the default PDP context (CID 0) is typically auto-activated
/// after network registration. This function waits for it and retrieves
/// the assigned IP address.
///
/// # Returns
/// `Ok(ip_address)` if activation was successful, `Err(Error::PdpActivation)`
/// if activation failed.
pub async fn activate<'a>(control: &Control<'a>) -> Result<Ipv4Address> {
	let mut resp_buf = [0u8; 256];

	// Give the modem time to establish data connection after registration
	embassy_time::Timer::after_millis(1000).await;

	// Check if we already have an IP (auto-activated context)
	if let Some(ip) = get_ip_address(control).await {
		return Ok(ip);
	}

	// If not auto-activated, try manual activation
	// Configure PDP context with default APN (uses SIM settings)
	let _ = control
		.at_command(b"AT+CGDCONT=0,\"IP\"", &mut resp_buf)
		.await;
	embassy_time::Timer::after_millis(100).await;

	// Activate PDP context
	let len = control.at_command(b"AT+CGACT=1,0", &mut resp_buf).await;
	embassy_time::Timer::after_millis(1000).await;

	if len > 0 {
		if let Ok(resp) = core::str::from_utf8(&resp_buf[..len]) {
			// Check for ERROR response
			if resp.contains("ERROR") {
				// Try again with longer wait - network might still be setting up
				embassy_time::Timer::after_millis(2000).await;
				return get_ip_address(control).await.ok_or(Error::PdpActivation);
			}
		}
	}

	// Query the assigned IP address
	get_ip_address(control).await.ok_or(Error::PdpActivation)
}

/// Deactivate PDP context.
///
/// # Returns
/// `Ok(())` on success, `Err(Error::PdpActivation)` on failure.
pub async fn deactivate<'a>(control: &Control<'a>) -> Result<()> {
	let mut resp_buf = [0u8; 128];
	let len = control.at_command(b"AT+CGACT=0,0", &mut resp_buf).await;

	if len > 0 {
		if let Ok(resp) = core::str::from_utf8(&resp_buf[..len]) {
			if resp.contains("OK") {
				return Ok(());
			}
		}
	}
	Err(Error::PdpActivation)
}

/// Get the IP address assigned to the PDP context.
pub async fn get_ip_address<'a>(control: &Control<'a>) -> Option<Ipv4Address> {
	let mut resp_buf = [0u8; 256];

	// Query PDP context addresses
	let len = control.at_command(b"AT+CGPADDR=0", &mut resp_buf).await;

	if len > 0 {
		if let Ok(resp) = core::str::from_utf8(&resp_buf[..len]) {
			return parse_cgpaddr_response(resp);
		}
	}
	None
}

/// Parse +CGPADDR response to extract IP address.
/// Format: +CGPADDR: 0,"10.160.x.x"
fn parse_cgpaddr_response(response: &str) -> Option<Ipv4Address> {
	// Find +CGPADDR: in response
	let cgpaddr_pos = response.find("+CGPADDR:")?;
	let after = &response[cgpaddr_pos + 9..];

	// Find the IP address in quotes
	let quote_start = after.find('"')? + 1;
	let quote_end = after[quote_start..].find('"')? + quote_start;
	let ip_str = &after[quote_start..quote_end];

	// Parse IP address
	parse_ipv4(ip_str)
}

/// Parse an IPv4 address string.
fn parse_ipv4(s: &str) -> Option<Ipv4Address> {
	let mut parts = s.split('.');
	let a: u8 = parts.next()?.parse().ok()?;
	let b: u8 = parts.next()?.parse().ok()?;
	let c: u8 = parts.next()?.parse().ok()?;
	let d: u8 = parts.next()?.parse().ok()?;

	if parts.next().is_some() {
		return None; // Too many parts
	}

	Some(Ipv4Address::new(a, b, c, d))
}

/// Configure the network stack with PDP context IP address.
pub fn configure_stack(stack: &Stack<'_>, ip: Ipv4Address, gateway: Option<Ipv4Address>) {
	let static_config = StaticConfigV4 {
		address: Ipv4Cidr::new(ip, 24),
		gateway,
		dns_servers: Default::default(),
	};
	stack.set_config_v4(ConfigV4::Static(static_config));
}

/// Task to monitor PDP context and configure network stack.
///
/// This task activates the PDP context after network registration
/// and configures the network stack with the assigned IP address.
#[embassy_executor::task]
pub async fn pdp_monitor_task(control: &'static Control<'static>, stack: &'static Stack<'static>) {
	use crate::registration::wait_for_status_change;

	// Wait for initial registration
	loop {
		let status = wait_for_status_change().await;
		if status.is_registered() {
			break;
		}
	}

	// Small delay after registration
	embassy_time::Timer::after_millis(500).await;

	// Activate PDP context
	match activate(control).await {
		Ok(ip) => {
			// Configure network stack
			configure_stack(stack, ip, None);
			PDP_STATUS_SIGNAL.signal(PdpStatus::Activated { ip });
		}
		Err(_) => {
			PDP_STATUS_SIGNAL.signal(PdpStatus::Deactivated);
		}
	}

	// Monitor for registration changes and reactivate if needed
	loop {
		let status = wait_for_status_change().await;

		if status.is_registered() {
			// Re-check PDP context
			embassy_time::Timer::after_millis(500).await;
			if let Some(ip) = get_ip_address(control).await {
				configure_stack(stack, ip, None);
				PDP_STATUS_SIGNAL.signal(PdpStatus::Activated { ip });
			}
		} else {
			PDP_STATUS_SIGNAL.signal(PdpStatus::Deactivated);
		}
	}
}

/// Wait for PDP context to be activated.
pub async fn wait_for_activation() -> PdpStatus {
	loop {
		let status = PDP_STATUS_SIGNAL.wait().await;
		if matches!(status, PdpStatus::Activated { .. }) {
			return status;
		}
	}
}
