//! Network stack for nRF91 modem using embassy-net.
//!
//! This module provides TCP/IP networking over the cellular modem
//! using the embassy-net stack with embassy-net-nrf91 driver.
//!
//! ## Error Handling
//! Functions return `Result<T, Error>` where errors should be handled
//! by the caller. For fatal errors, use the `fatal_error!` macro.

#![allow(dead_code)]

use crate::error::{Error, Result};

use embassy_executor::Spawner;
use embassy_net::{ConfigV4, Ipv4Address, Ipv4Cidr, Stack, StackResources, StaticConfigV4};
use embassy_net_nrf91::NetDriver;
use static_cell::StaticCell;

/// Network stack resources.
/// Adjust socket count based on application needs.
const SOCKET_COUNT: usize = 4;

/// Task to run the embassy-net stack.
///
/// This task handles IP packet processing and must run continuously.
#[embassy_executor::task]
pub async fn net_task(mut runner: embassy_net::Runner<'static, NetDriver<'static>>) -> ! {
	runner.run().await
}

/// Initialize the network stack.
///
/// For cellular modems, IP configuration comes from the PDP context,
/// not DHCP. The stack starts with default config and needs to be
/// configured later when the modem provides IP info.
///
/// # Arguments
/// * `spawner` - Embassy spawner for task creation
/// * `device` - The nRF91 modem NetDriver from embassy-net-nrf91
///
/// # Returns
/// `Ok(&Stack)` on success, `Err(Error)` on failure
///
/// # Errors
/// Returns `Error::TaskSpawn` if the network task cannot be spawned.
pub async fn init(
	spawner: &Spawner,
	device: NetDriver<'static>,
) -> Result<&'static Stack<'static>> {
	// Network stack resources (sockets, etc.)
	static RESOURCES: StaticCell<StackResources<SOCKET_COUNT>> = StaticCell::new();
	let resources = RESOURCES.init(StackResources::new());

	// Create the network stack with default config
	// IP configuration will be set when PDP context is activated
	let config = embassy_net::Config::default();

	let seed = embassy_time::Instant::now().as_ticks();

	static STACK: StaticCell<Stack<'static>> = StaticCell::new();
	let (stack, runner) = embassy_net::new(device, config, resources, seed);
	let stack = STACK.init(stack);

	// Spawn the network task
	let token = net_task(runner).map_err(|_| Error::TaskSpawn)?;
	spawner.spawn(token);

	Ok(stack)
}

/// Set the IPv4 configuration on the stack.
///
/// Call this when the modem provides IP configuration from PDP context.
pub fn set_ipv4_config(stack: &Stack<'_>, address: Ipv4Address, gateway: Option<Ipv4Address>) {
	let static_config = StaticConfigV4 {
		address: Ipv4Cidr::new(address, 24), // Typical cellular prefix
		gateway,
		dns_servers: Default::default(),
	};
	stack.set_config_v4(ConfigV4::Static(static_config));
}

/// Wait for the network stack to have a valid IP configuration.
///
/// This waits until the modem provides an IP address through PDP context.
pub async fn wait_for_config(stack: &Stack<'_>) {
	loop {
		if stack.is_config_up() {
			break;
		}
		embassy_time::Timer::after_millis(100).await;
	}
}

/// Wait for the network link to be up (registered on network).
pub async fn wait_for_link(stack: &Stack<'_>) {
	loop {
		if stack.is_link_up() {
			break;
		}
		embassy_time::Timer::after_millis(100).await;
	}
}

/// Get the current IPv4 configuration if available.
pub fn get_ipv4_config(stack: &Stack<'_>) -> Option<StaticConfigV4> {
	stack.config_v4()
}
