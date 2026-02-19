//! Modem initialization and management for nRF91 series.
//!
//! This module provides modem initialization, task management,
//! and AT command utilities.
//!
//! ## Modem Traces
//! Modem traces are forwarded to UART1 at 1 Mbaud.
//! Use `init_with_trace()` to enable trace forwarding.
//! Connect a trace tool to UART1 TX pin to capture modem debug output.
//!
//! ## Error Handling
//! Functions return `Result<T, Error>` where errors should be handled
//! by the caller. For fatal errors, use the `fatal_error!` macro.

#![allow(dead_code)]

use crate::error::{Error, Result};

use core::mem::MaybeUninit;
use core::ptr::addr_of_mut;
use core::slice;

use embassy_executor::Spawner;
use embassy_net_nrf91::{Control, NetDriver, Runner, State, TraceBuffer, TraceReader};
use embassy_nrf::buffered_uarte::{self, BufferedUarteTx};
use embassy_nrf::gpio::Pin;
use embassy_nrf::interrupt;
use embassy_nrf::uarte::Baudrate;
use embassy_nrf::{bind_interrupts, peripherals, uarte, Peri};
use embassy_time::Timer;
use static_cell::StaticCell;

use crate::registration::RegistrationMonitor;

// External symbols for IPC memory region (defined in memory.x)
unsafe extern "C" {
	static __start_ipc: u8;
	static __end_ipc: u8;
}

/// IPC interrupt handler required for modem communication.
/// Must be called from the IPC interrupt vector.
#[interrupt]
fn IPC() {
	embassy_net_nrf91::on_ipc_irq();
}

bind_interrupts!(struct TraceIrqs {
	SERIAL1 => buffered_uarte::InterruptHandler<peripherals::SERIAL1>;
});

// Static buffer for trace UART TX
static mut TRACE_UART_BUF: [u8; 4096] = [0u8; 4096];

/// Task to run the modem driver.
///
/// This task must be spawned and will run forever, handling
/// modem IPC communication.
#[embassy_executor::task]
pub async fn modem_runner_task(runner: Runner<'static>) -> ! {
	runner.run().await
}

/// Task to forward modem traces to UART1.
///
/// Reads trace data from the modem and writes it to UART at 1 Mbaud.
#[embassy_executor::task]
pub async fn trace_task(mut uart: BufferedUarteTx<'static>, reader: TraceReader<'static>) -> ! {
	let mut rx = [0u8; 1024];
	loop {
		let n = reader.read(&mut rx[..]).await;
		// Write all data using inherent method
		let mut offset = 0;
		while offset < n {
			match uart.write(&rx[offset..n]).await {
				Ok(written) => offset += written,
				Err(_) => break,
			}
		}
	}
}

/// Task to monitor CEREG registration status.
///
/// This task enables CEREG URCs and monitors for registration
/// status changes, signaling through REGISTRATION_SIGNAL.
#[embassy_executor::task]
pub async fn registration_monitor_task(control: &'static Control<'static>) {
	let mut monitor = RegistrationMonitor::new();

	// Enable CEREG URCs
	monitor.enable_urcs(control).await;
	Timer::after_millis(100).await;

	// Do initial query to get current status
	monitor.query_status(control).await;

	// Note: The nRF91 modem sends +CEREG URCs when status changes.
	// With AT+CEREG=2, these are delivered automatically.
	// The embassy-net-nrf91 driver's at_command interface may receive
	// these as part of responses. For true event-driven handling,
	// we'd need direct URC subscription which isn't exposed in the API.
	//
	// This implementation queries status after enabling URCs.
	// In a production system, you might use the network stack's
	// built-in connectivity handling instead.

	// The task stays alive to handle any future monitoring needs
	loop {
		// Wait for external trigger or timeout
		// In a real implementation with URC subscription, we'd await here
		Timer::after_secs(30).await;
		monitor.query_status(control).await;
	}
}

/// Get the IPC memory region from linker symbols.
///
/// # Safety
/// This function reads from linker-defined symbols and creates
/// a mutable slice from them. The caller must ensure the memory
/// region is valid and not accessed elsewhere.
pub unsafe fn get_ipc_memory() -> &'static mut [MaybeUninit<u8>] {
	let ipc_start = &__start_ipc as *const u8 as *mut MaybeUninit<u8>;
	let ipc_end = &__end_ipc as *const u8 as *mut MaybeUninit<u8>;
	let ipc_len = ipc_end.offset_from(ipc_start) as usize;
	slice::from_raw_parts_mut(ipc_start, ipc_len)
}

/// Initialize the modem and spawn required tasks.
///
/// Returns tuple of (NetDriver for network stack, Control for AT commands).
/// This variant does not enable modem traces.
///
/// # Arguments
/// * `spawner` - Embassy spawner for task creation
///
/// # Returns
/// `Ok((NetDriver, Control))` on success, `Err(Error)` on failure
///
/// # Errors
/// Returns `Error::TaskSpawn` if task spawning fails.
pub async fn init(spawner: &Spawner) -> Result<(NetDriver<'static>, &'static Control<'static>)> {
	// Get IPC memory
	let ipc_mem = unsafe { get_ipc_memory() };

	// Initialize the modem driver (without traces)
	static STATE: StaticCell<State> = StaticCell::new();
	let (device, control, runner) =
		embassy_net_nrf91::new(STATE.init(State::new()), ipc_mem).await;

	// Spawn modem runner task
	let token = modem_runner_task(runner).map_err(|_| Error::TaskSpawn)?;
	spawner.spawn(token);

	// Store control in static
	static CONTROL: StaticCell<Control<'static>> = StaticCell::new();
	let control = CONTROL.init(control);

	// Wait for modem to be ready
	control.wait_init().await;

	// Spawn registration monitor
	let token = registration_monitor_task(control).map_err(|_| Error::TaskSpawn)?;
	spawner.spawn(token);

	Ok((device, control))
}

/// Initialize the modem with trace forwarding to UART1.
///
/// Modem traces will be output on UART1 TX pin at 1 Mbaud.
///
/// # Arguments
/// * `spawner` - Embassy spawner for task creation
/// * `serial1` - SERIAL1 peripheral for trace UART
/// * `trace_tx_pin` - TX pin for trace output (typically P0.01 on DK)
///
/// # Returns
/// `Ok((NetDriver, Control))` on success, `Err(Error)` on failure
///
/// # Errors
/// Returns `Error::TaskSpawn` if task spawning fails.
pub async fn init_with_trace(
	spawner: &Spawner,
	serial1: Peri<'static, peripherals::SERIAL1>,
	trace_tx_pin: Peri<'static, impl Pin>,
) -> Result<(NetDriver<'static>, &'static Control<'static>)> {
	// Get IPC memory
	let ipc_mem = unsafe { get_ipc_memory() };

	// Initialize the modem driver with trace support
	static STATE: StaticCell<State> = StaticCell::new();
	static TRACE_BUF: StaticCell<TraceBuffer> = StaticCell::new();

	let (device, control, runner, trace_reader) = embassy_net_nrf91::new_with_trace(
		STATE.init(State::new()),
		ipc_mem,
		TRACE_BUF.init(TraceBuffer::new()),
	)
	.await;

	// Set up trace UART at 1 Mbaud
	let mut trace_config = uarte::Config::default();
	trace_config.baudrate = Baudrate::BAUD1M;

	let trace_uart =
		BufferedUarteTx::new(serial1, trace_tx_pin, TraceIrqs, trace_config, unsafe {
			&mut *addr_of_mut!(TRACE_UART_BUF)
		});

	// Spawn trace forwarding task
	let token = trace_task(trace_uart, trace_reader).map_err(|_| Error::TaskSpawn)?;
	spawner.spawn(token);

	// Spawn modem runner task
	let token = modem_runner_task(runner).map_err(|_| Error::TaskSpawn)?;
	spawner.spawn(token);

	// Store control in static
	static CONTROL_TRACE: StaticCell<Control<'static>> = StaticCell::new();
	let control = CONTROL_TRACE.init(control);

	// Wait for modem to be ready
	control.wait_init().await;

	// Enable modem trace output
	let mut resp_buf = [0u8; 64];
	let _ = control
		.at_command(b"AT%XMODEMTRACE=1,2", &mut resp_buf)
		.await;

	// Spawn registration monitor
	let token = registration_monitor_task(control).map_err(|_| Error::TaskSpawn)?;
	spawner.spawn(token);

	Ok((device, control))
}

/// Send an AT command and return the response.
///
/// # Arguments
/// * `control` - Modem control interface
/// * `cmd` - AT command string (without trailing CR/LF)
/// * `resp_buf` - Buffer to store the response
///
/// # Returns
/// Number of bytes written to response buffer
pub async fn at_command<'a>(control: &Control<'a>, cmd: &str, resp_buf: &mut [u8]) -> usize {
	control.at_command(cmd.as_bytes(), resp_buf).await
}

/// Send an AT command and check if response contains "OK".
///
/// # Returns
/// `Ok(())` if response contains "OK", `Err(Error::AtCommand)` otherwise.
pub async fn at_command_ok<'a>(control: &Control<'a>, cmd: &str) -> Result<()> {
	let mut resp_buf = [0u8; 128];
	let len = at_command(control, cmd, &mut resp_buf).await;

	if len > 0 {
		if let Ok(resp) = core::str::from_utf8(&resp_buf[..len]) {
			if resp.contains("OK") {
				return Ok(());
			}
		}
	}
	Err(Error::AtCommand)
}

/// Enable the modem (CFUN=1).
///
/// # Returns
/// `Ok(())` on success, `Err(Error::AtCommand)` on failure.
pub async fn enable<'a>(control: &Control<'a>) -> Result<()> {
	at_command_ok(control, "AT+CFUN=1").await
}

/// Disable the modem (CFUN=0).
///
/// # Returns
/// `Ok(())` on success, `Err(Error::AtCommand)` on failure.
pub async fn disable<'a>(control: &Control<'a>) -> Result<()> {
	at_command_ok(control, "AT+CFUN=0").await
}

/// Get modem firmware version.
pub async fn get_firmware_version<'a, 'b>(
	control: &Control<'a>,
	buf: &'b mut [u8],
) -> Option<&'b str> {
	let len = at_command(control, "AT+CGMR", buf).await;
	if len > 0 {
		core::str::from_utf8(&buf[..len]).ok()
	} else {
		None
	}
}

/// Get IMEI.
pub async fn get_imei<'a, 'b>(control: &Control<'a>, buf: &'b mut [u8]) -> Option<&'b str> {
	let len = at_command(control, "AT+CGSN", buf).await;
	if len > 0 {
		core::str::from_utf8(&buf[..len]).ok()
	} else {
		None
	}
}
