//! This module contains constants for the embassy-rs Vendor Example.

// Copyright (c) 2025 Piers Finlayson <piers@piers.rocks>
//
// MIT licensed - see https://opensource.org/licenses/MIT
use embassy_time::Duration;

// How often we aim to log from our primary loops to prove they are still
// alive.
pub const LOOP_LOG_INTERVAL: Duration = Duration::from_secs(5);

/// Watchdog timer - the watchdog resets the system if it isn't feed at
/// least this frequently.
pub const WATCHDOG_TIMER: Duration = Duration::from_secs(1);

/// How often the runner threads aim to feed the watchdog timer so it doesn't
/// reset the device.
pub const WATCHDOG_FEED_TIMER: Duration = Duration::from_millis(100);

/// Timer for the ProtocolHandler to pause between loops of its main runner.
/// This is a low value, to ensure we apply Control driven changes quickly,
/// and serve any outstanding data rapidly.
pub const PROTOCOL_HANDLER_TIMER: Duration = Duration::from_millis(1);

/// USB Descriptor information - what current in mA this device draws.
pub const USB_POWER_MA: u16 = 100;

/// USB Descriptor information - maximum endpoint 0 (control endpoint)
/// packet size.
pub const MAX_PACKET_SIZE_0: u8 = 64;

/// USB Descriptor information - maximum vendor endpoint packet sizes.
pub const MAX_EP_PACKET_SIZE: u16 = 64;

/// OUT (Host to Devive) endpoint number.  Note on the Pi this cannot be
/// larger than 0x0F, as the Pi only supports up to 16 endpoints in its
/// hardware registers.  If it is, the firmware will panic during the
/// endpoint allocation, as the endpoint cannot be allocated.
pub const OUT_EP: u8 = 0x04;

/// IN (Device to Host) endpoint number/  As above, this cannot be larger
/// that 0x0F.
pub const IN_EP: u8 = 0x83;

/// USB Descriptor information - Vendor ID and Product ID
pub const VENDOR_ID: u16 = 0x1209; // Not officially assigned
pub const PRODUCT_ID: u16 = 0x0f0f; // Not officially assigned

/// USB Descriptor information - manufacturer string
pub const MANUFACTURER: &str = "piers.rocks";

/// USB Descriptor info - product string
pub const PRODUCT: &str = "embassy-rs Vendor Example";

/// USB Descriptor info - serial number string
pub const SERIAL: &str = "000";

/// USB Descriptor info - device class, subclass, and protocol
pub const USB_CLASS: u8 = 0xff;
pub const USB_SUB_CLASS: u8 = 0;
pub const USB_PROTOCOL: u8 = 0;

/// Maximum sise of a Write command
pub const MAX_WRITE_SIZE: u16 = 32768;
pub const MAX_WRITE_SIZE_USIZE: usize = MAX_WRITE_SIZE as usize;

/// Maximum size of a Read command
pub const MAX_READ_SIZE: u16 = 32768;
#[allow(dead_code)]
pub const MAX_READ_SIZE_USIZE: usize = MAX_READ_SIZE as usize;
