# nRF9151 Rust Modem Project

Rust project for the nRF9151 using Embassy async runtime with the `embassy-net-nrf91` modem driver.

## Features

- Embassy async framework for embedded Rust
- Direct AT command interface to the modem
- UART logging at 115200 baud
- Network registration monitoring (CEREG)

## Prerequisites

### 1. Install Rust target

```bash
rustup target add thumbv8m.main-none-eabihf
```

### 2. Install probe-rs for flashing/debugging

```bash
# Using cargo
cargo install probe-rs-tools

# Or using brew on macOS
brew install probe-rs
```

### 3. Flash modem firmware on the nRF9151

The nRF9151 requires Nordic's modem firmware to be flashed separately. Download it from:
https://www.nordicsemi.com/Products/nRF9151/Download

Use nRF Connect for Desktop's Programmer app to flash the modem firmware.

## Building

```bash
cargo build --release
```

## Flashing and Running

```bash
cargo run --release
```

## UART Logging

The application outputs logs via UART at **115200 baud**:
- **TX:** P0.29
- **RX:** P0.28

Connect a serial terminal to view the output.

## Project Structure

```
nrf9151-modem/
├── .cargo/
│   └── config.toml       # Target and runner configuration
├── src/
│   └── main.rs           # Application entry point with AT commands
├── Cargo.toml            # Dependencies (embassy-net-nrf91, embassy-nrf)
├── memory.x              # Memory layout (IPC + RAM regions)
└── README.md
```

## Memory Layout

The project uses the following memory layout for modem IPC:

| Region | Address      | Size  | Purpose                          |
|--------|--------------|-------|----------------------------------|
| FLASH  | 0x00000000   | 1024K | Application code                 |
| IPC    | 0x20000000   | 64K   | Modem shared memory (IPC)        |
| RAM    | 0x20010000   | 192K  | Application RAM                  |

## AT Commands

The example demonstrates basic AT command usage:

| Command      | Description                      |
|--------------|----------------------------------|
| `AT`         | Basic test command               |
| `AT+CFUN?`   | Query functional mode            |
| `AT+CFUN=1`  | Enable modem (full functionality)|
| `AT+CEREG=2` | Enable registration URCs         |
| `AT+CEREG?`  | Query registration status        |
| `AT+CGMR`    | Get modem firmware version       |
| `AT+CGSN`    | Get IMEI                         |

### CEREG Status Values

| Value | Meaning                                    |
|-------|--------------------------------------------|
| 0     | Not registered, not searching              |
| 1     | Registered, home network                   |
| 2     | Not registered, searching                  |
| 3     | Registration denied                        |
| 4     | Unknown                                    |
| 5     | Registered, roaming                        |

## Dependencies

This project uses Embassy git dependencies for compatible versions:

- `embassy-executor` - Async executor for Cortex-M
- `embassy-nrf` - nRF HAL with Embassy support (nrf9160-s feature)
- `embassy-net-nrf91` - Modem driver for nRF91 series
- `embassy-time` - Async timers

## Useful Resources

- [Embassy framework](https://embassy.dev/)
- [embassy-net-nrf91 source](https://github.com/embassy-rs/embassy/tree/main/embassy-net-nrf91)
- [Nordic nRF9151 documentation](https://www.nordicsemi.com/Products/nRF9151)
- [nRF AT Commands Reference](https://infocenter.nordicsemi.com/topic/ref_at_commands/REF/at_commands/intro.html)

## Troubleshooting

### "No probe found"
- Ensure your DK is connected
- Check USB cable (some are charge-only)
- Try `probe-rs list` to see connected probes

### AT commands hang / no response
- Ensure modem firmware is flashed on the device
- The modem firmware must be downloaded separately from Nordic
- Wait for "Modem ready!" message before sending commands

### CEREG shows 0 (not registered)
- Check that a SIM card is inserted
- Verify antenna is connected
- Check signal strength in your location
- Some networks may take time to register (wait 30+ seconds)

### Build errors with embassy crates
This project uses git dependencies from the Embassy repository. If you encounter version conflicts, ensure all embassy crates are from the same git revision.
