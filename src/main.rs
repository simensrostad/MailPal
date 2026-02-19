#![no_std]
#![no_main]

use core::mem::MaybeUninit;
use core::slice;
use core::fmt::Write as FmtWrite;

use panic_halt as _;

use embassy_executor::Spawner;
use embassy_nrf::gpio::{Level, Output, OutputDrive};
use embassy_nrf::uarte::{self, Uarte};
use embassy_nrf::{bind_interrupts, interrupt, peripherals};
use embassy_time::Timer;
use embassy_net_nrf91::{Runner, State, Control};
use heapless::String;
use static_cell::StaticCell;

bind_interrupts!(struct Irqs {
    SERIAL0 => uarte::InterruptHandler<peripherals::SERIAL0>;
});

// IPC interrupt handler required for modem communication
#[interrupt]
fn IPC() {
    embassy_net_nrf91::on_ipc_irq();
}

// External symbols for IPC memory region (defined in memory.x)
unsafe extern "C" {
    static __start_ipc: u8;
    static __end_ipc: u8;
}

// Task to run the modem driver
#[embassy_executor::task]
async fn modem_task(runner: Runner<'static>) -> ! {
    runner.run().await
}

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    // Initialize embassy-nrf peripherals
    let p = embassy_nrf::init(Default::default());

    // Set up UART for logging
    let mut config = uarte::Config::default();
    config.parity = uarte::Parity::EXCLUDED;
    config.baudrate = uarte::Baudrate::BAUD115200;

    let mut uart = Uarte::new(p.SERIAL0, p.P0_28, p.P0_29, Irqs, config);

    // Helper macro for UART logging
    macro_rules! log {
        ($uart:expr, $($arg:tt)*) => {{
            let mut buf: String<256> = String::new();
            let _ = core::write!(&mut buf, $($arg)*);
            let _ = buf.push_str("\r\n");
            let _ = $uart.write(buf.as_bytes()).await;
        }};
    }

    log!(uart, "nRF9151 Modem AT Command Test");
    log!(uart, "==============================");

    // LED for status indication
    let mut led = Output::new(p.P0_00, Level::Low, OutputDrive::Standard);

    // Blink LED to show we're starting
    for _ in 0..3 {
        led.set_high();
        Timer::after_millis(100).await;
        led.set_low();
        Timer::after_millis(100).await;
    }

    log!(uart, "Setting up IPC memory...");

    // Get IPC memory region from linker symbols
    let ipc_mem = unsafe {
        let ipc_start = &__start_ipc as *const u8 as *mut MaybeUninit<u8>;
        let ipc_end = &__end_ipc as *const u8 as *mut MaybeUninit<u8>;
        let ipc_len = ipc_end.offset_from(ipc_start) as usize;
        slice::from_raw_parts_mut(ipc_start, ipc_len)
    };

    log!(uart, "IPC memory: {} bytes", ipc_mem.len());
    log!(uart, "Initializing modem...");

    // Initialize the modem driver
    static STATE: StaticCell<State> = StaticCell::new();
    let (_device, control, runner) = embassy_net_nrf91::new(STATE.init(State::new()), ipc_mem).await;

    // Spawn modem task
    spawner.spawn(modem_task(runner).unwrap());

    log!(uart, "Modem initialized!");

    // Store control in static for use
    static CONTROL: StaticCell<Control<'static>> = StaticCell::new();
    let control = CONTROL.init(control);

    // Wait for modem to be ready for AT commands
    log!(uart, "Waiting for modem ready...");
    control.wait_init().await;
    log!(uart, "Modem ready!");

    // Helper to send AT command and log response
    async fn send_at<'a>(
        control: &Control<'a>,
        uart: &mut Uarte<'_>,
        cmd: &str,
    ) {
        let mut resp_buf = [0u8; 256];
        let cmd_bytes = cmd.as_bytes();

        // Log what we're sending
        let mut buf: String<256> = String::new();
        let _ = core::write!(&mut buf, ">> {}", cmd);
        let _ = buf.push_str("\r\n");
        let _ = uart.write(buf.as_bytes()).await;

        // Send command
        let len = control.at_command(cmd_bytes, &mut resp_buf).await;

        // Log response
        if len > 0 {
            if let Ok(resp_str) = core::str::from_utf8(&resp_buf[..len]) {
                let mut buf: String<256> = String::new();
                let _ = core::write!(&mut buf, "<< {}", resp_str.trim());
                let _ = buf.push_str("\r\n");
                let _ = uart.write(buf.as_bytes()).await;
            }
        }
    }

    log!(uart, "");
    log!(uart, "Sending AT commands...");
    log!(uart, "");

    // Test basic AT command
    send_at(control, &mut uart, "AT").await;
    Timer::after_millis(500).await;

    // Check current functional mode
    send_at(control, &mut uart, "AT+CFUN?").await;
    Timer::after_millis(500).await;

    // Enable modem (CFUN=1)
    log!(uart, "");
    log!(uart, "Enabling modem (CFUN=1)...");
    send_at(control, &mut uart, "AT+CFUN=1").await;
    Timer::after_millis(1000).await;

    // Verify CFUN is now 1
    send_at(control, &mut uart, "AT+CFUN?").await;
    Timer::after_millis(500).await;

    // Check network registration status
    log!(uart, "");
    log!(uart, "Checking network registration...");

    // Enable network registration URCs
    send_at(control, &mut uart, "AT+CEREG=2").await;
    Timer::after_millis(500).await;

    // Query current registration status
    send_at(control, &mut uart, "AT+CEREG?").await;
    Timer::after_millis(500).await;

    // Get modem firmware version
    log!(uart, "");
    log!(uart, "Modem info:");
    send_at(control, &mut uart, "AT+CGMR").await;
    Timer::after_millis(500).await;

    // Get IMEI
    send_at(control, &mut uart, "AT+CGSN").await;
    Timer::after_millis(500).await;

    // Poll CEREG periodically to check registration
    log!(uart, "");
    log!(uart, "Monitoring registration status...");
    log!(uart, "(CEREG: 0=not registered, 1=home, 2=searching, 5=roaming)");
    log!(uart, "");

    let mut counter = 0u32;
    loop {
        // Check registration status
        send_at(control, &mut uart, "AT+CEREG?").await;

        // Blink LED
        led.set_high();
        Timer::after_millis(100).await;
        led.set_low();

        counter += 1;
        log!(uart, "--- Poll {} ---", counter);

        // Wait 5 seconds before next check
        Timer::after_secs(5).await;
    }
}
