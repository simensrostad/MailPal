#![no_std]
#![no_main]

mod error;
mod logger;
mod modem;
mod network;
mod pdp;
mod registration;

use panic_halt as _;

use embassy_executor::Spawner;
use embassy_net::tcp::TcpSocket;
use embassy_nrf::gpio::{Level, Output, OutputDrive};
use embassy_nrf::uarte::{self, Uarte};
use embassy_nrf::{bind_interrupts, peripherals};
use embassy_time::Timer;

use registration::wait_for_status_change;

bind_interrupts!(struct Irqs {
	SERIAL0 => uarte::InterruptHandler<peripherals::SERIAL0>;
});

#[embassy_executor::main]
async fn main(spawner: Spawner) {
	/* Initialize embassy-nrf peripherals and related libraries */
	let p = embassy_nrf::init(Default::default());

	/*  */
	let mut config = uarte::Config::default();

	config.parity = uarte::Parity::EXCLUDED;
	config.baudrate = uarte::Baudrate::BAUD115200;

	let mut uart = Uarte::new(p.SERIAL0, p.P0_26, p.P0_27, Irqs, config);
	let mut led = Output::new(p.P0_00, Level::Low, OutputDrive::Standard);

	log!(uart, "");
	log!(uart, "        ___     ,~~.");
	log!(uart, "   ,~~./   \\o  (  6 )-_,");
	log!(uart, "  (  6 )-_, \\   `--'    )-.___");
	log!(uart, "   `--'   )-.__       .'      `--,");
	log!(uart, "        .'              ,___(     )");
	log!(uart, "                      ,_|  _)    /");
	log!(uart, "          .--------.  |_|__|)   /");
	log!(uart, "         /          \\ |______| /");
	log!(uart, "        |   MAIL    | |      |/");
	log!(uart, "        |    PAL    |=|      |");
	log!(uart, "        |           | |  []  |");
	log!(uart, "        `----------'  |______|");
	log!(uart, "             |||         ||");
	log!(uart, "        _____|_|_____    ||");
	log!(uart, "       /////////////\\   _||_");
	log!(uart, "");

	// Startup LED indication
	for _ in 0..3 {
		led.set_high();
		Timer::after_millis(100).await;
		led.set_low();
		Timer::after_millis(100).await;
	}

	// Initialize modem with trace forwarding to UART1 (P0.29 TX at 1 Mbaud)
	// TX: P0.29 - Available as VCOM1 through USB
	log!(uart, "Initializing modem with traces...");
	let (device, control) = match modem::init_with_trace(&spawner, p.SERIAL1, p.P0_29).await {
		Ok(result) => result,
		Err(e) => {
			log!(uart, "FATAL: Modem init failed: {:?}", e);
			fatal_error!("Modem initialization failed")
		}
	};
	log!(uart, "Modem ready (traces on UART1 @ 1Mbaud)!");

	// Initialize network stack
	log!(uart, "Initializing network stack...");
	let stack = match network::init(&spawner, device).await {
		Ok(s) => s,
		Err(e) => {
			log!(uart, "FATAL: Network init failed: {:?}", e);
			fatal_error!("Network stack initialization failed")
		}
	};
	log!(uart, "Network stack initialized!");

	// Enable modem radio
	log!(uart, "");
	log!(uart, "Enabling modem (CFUN=1)...");
	if let Err(e) = modem::enable(control).await {
		log!(uart, "FATAL: Failed to enable modem: {:?}", e);
		fatal_error!("Modem enable (CFUN=1) failed");
	}
	log!(uart, "Modem enabled");

	Timer::after_millis(500).await;

	// Wait for network registration
	log!(uart, "");
	log!(uart, "Waiting for network registration...");

	loop {
		// Wait for registration status change (non-polling, event-driven)
		let status = wait_for_status_change().await;

		// Log status change
		log!(uart, "CEREG: {}", status.as_str());

		// Visual feedback
		led.set_high();
		Timer::after_millis(100).await;
		led.set_low();

		// Handle registration success
		if status.is_registered() {
			log!(uart, "");
			log!(uart, "Network registered!");
			break;
		}
	}

	// Wait for network stack to get IP config
	log!(uart, "");
	log!(uart, "Activating PDP context (data connection)...");

	// Activate PDP context and configure network stack
	let _ip = match pdp::activate(control).await {
		Ok(ip) => {
			log!(uart, "PDP context activated!");
			pdp::configure_stack(stack, ip, None);
			log!(uart, "IP address: {}", ip);
			ip
		}
		Err(e) => {
			log!(uart, "FATAL: PDP activation failed: {:?}", e);
			fatal_error!("PDP context activation failed");
		}
	};

	// Wait for stack configuration
	network::wait_for_config(stack).await;
	log!(uart, "Network ready!");

	// Demonstrate TCP socket connection
	log!(uart, "");
	log!(uart, "Testing TCP connection...");

	// Socket buffers
	let mut rx_buffer = [0u8; 1024];
	let mut tx_buffer = [0u8; 1024];

	let mut socket = TcpSocket::new(*stack, &mut rx_buffer, &mut tx_buffer);
	socket.set_timeout(Some(embassy_time::Duration::from_secs(10)));

	// Connect to httpbin.org (IP: 54.208.105.16) port 80
	// Note: For production, use DNS resolution
	let remote_endpoint = embassy_net::IpEndpoint::new(
		embassy_net::IpAddress::v4(54, 208, 105, 16), // httpbin.org
		80,
	);

	log!(uart, "Connecting to httpbin.org:80...");
	match socket.connect(remote_endpoint).await {
		Ok(()) => {
			log!(uart, "Connected!");

			// Send HTTP GET request
			let request = b"GET /ip HTTP/1.1\r\n\
				Host: httpbin.org\r\n\
				Connection: close\r\n\r\n";

			log!(uart, "Sending HTTP request...");

			// Write all data
			let mut written = 0;
			while written < request.len() {
				match socket.write(&request[written..]).await {
					Ok(0) => {
						log!(uart, "Write error: connection closed");
						break;
					}
					Ok(n) => written += n,
					Err(e) => {
						log!(uart, "Write error: {:?}", e);
						break;
					}
				}
			}

			if written == request.len() {
				log!(uart, "Request sent, reading response...");

				// Read response
				let mut response_buf = [0u8; 512];
				match socket.read(&mut response_buf).await {
					Ok(0) => log!(uart, "Connection closed by server"),
					Ok(n) => {
						if let Ok(response) =
							core::str::from_utf8(&response_buf[..n])
						{
							log!(uart, "Response ({} bytes):", n);
							// Print first few lines of response
							for line in response.lines().take(10) {
								log!(uart, "  {}", line);
							}
						}
					}
					Err(e) => log!(uart, "Read error: {:?}", e),
				}
			}

			socket.close();
		}
		Err(e) => {
			log!(uart, "Connection failed: {:?}", e);
		}
	}

	// Main application loop
	log!(uart, "");
	log!(uart, "Application running. Monitoring registration...");

	loop {
		// Monitor for registration changes
		let status = wait_for_status_change().await;
		log!(uart, "Registration changed: {}", status.as_str());

		led.set_high();
		Timer::after_millis(100).await;
		led.set_low();

		if !status.is_registered() {
			log!(uart, "Warning: Lost network registration!");
		}
	}
}
