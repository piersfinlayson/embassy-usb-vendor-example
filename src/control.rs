//! This module implements the USB Control handler.

// Copyright (c) 2025 Piers Finlayson <piers@piers.rocks>
//
// MIT licensed - see https://opensource.org/licenses/MIT

#[allow(unused_imports)]
use defmt::{debug, error, info, trace, warn};

use embassy_usb::control::{InResponse, OutResponse, Recipient, Request, RequestType};
use embassy_usb::types::InterfaceNumber;
use embassy_usb::Handler;
use rp2040_rom::ROM;

use crate::protocol::ProtocolAction;
use crate::types::Direction;
use crate::PROTOCOL_ACTION;

/// Handle USB events.
pub struct Control {
    if_num: InterfaceNumber,
}

enum ControlError {
    Ignore,
    Invalid,
}

impl Handler for Control {
    /// Called when the USB device has been enabled or disabled.
    fn enabled(&mut self, enabled: bool) {
        match enabled {
            true => info!("USB device enabled"),
            false => info!("USB device disabled"),
        }
    }

    /// Called after a USB reset after the bus reset sequence is complete.
    fn reset(&mut self) {
        info!("USB device reset complete");
    }

    /// Called when the host has set the address of the device to `addr`.
    fn addressed(&mut self, addr: u8) {
        info!("USB device addressed: {}", addr);
    }

    /// Called when the host has enabled or disabled the configuration of the device.
    fn configured(&mut self, configured: bool) {
        match configured {
            true => info!("USB device configuration enabled"),
            false => info!("USB device configuration disabled"),
        }
    }

    /// Called when the bus has entered or exited the suspend state.
    fn suspended(&mut self, suspended: bool) {
        match suspended {
            true => {
                info!("USB device suspended");
            }
            false => {
                info!("USB device resumed");
            }
        }
    }

    /// Respond to HostToDevice control messages, where the host sends us a command and
    /// optionally some data, and we can only acknowledge or reject it.
    fn control_out<'a>(&'a mut self, req: Request, buf: &'a [u8]) -> Option<OutResponse> {
        // Log the request before filtering to help with debugging.
        info!("Got control_out, request={}, buf={:a}", req, buf);

        // Get the request type, and check for errors
        let request = match self.check_request(req, Direction::Out) {
            Err(ControlError::Ignore) => return None,
            Err(ControlError::Invalid) => return Some(OutResponse::Rejected),
            Ok(r) => r,
        };

        // Handle the request
        match self.handle_out(&request) {
            Err(ControlError::Ignore) => None,
            Err(ControlError::Invalid) => Some(OutResponse::Rejected),
            Ok(_) => Some(OutResponse::Accepted),
        }
    }

    /// Respond to DeviceToHost control messages, where the host requests some data from us.
    fn control_in<'a>(&'a mut self, req: Request, buf: &'a mut [u8]) -> Option<InResponse<'a>> {
        info!("Got control_in, request={}", req);

        // Get the request type, and check for errors
        let request = match self.check_request(req, Direction::In) {
            Err(ControlError::Ignore) => return None,
            Err(ControlError::Invalid) => return Some(InResponse::Rejected),
            Ok(r) => r,
        };

        // Handle the request and build the response
        match self.handle_in(&request, buf, req.length as usize) {
            Err(ControlError::Ignore) => None,
            Err(ControlError::Invalid) => Some(InResponse::Rejected),
            Ok(0) => Some(InResponse::Rejected),
            Ok(len) => Some(InResponse::Accepted(&buf[..len])),
        }
    }
}

// Our own functions
impl Control {
    pub fn new(if_num: InterfaceNumber) -> Self {
        Self { if_num }
    }

    fn check_request(&self, req: Request, dir: Direction) -> Result<ControlRequest, ControlError> {
        // Only handle Class request types to an Interface.
        if req.request_type != RequestType::Class || req.recipient != Recipient::Interface {
            info!(
                "Ignoring request type: {:?}, recipient: {:?}",
                req.request_type, req.recipient
            );
            return Err(ControlError::Ignore);
        }

        // Ignore requests to other interfaces.
        if req.index != self.if_num.0 as u16 {
            info!("Ignoring request to interface: {}", req.index);
            return Err(ControlError::Ignore);
        }

        // Check we got a supported request
        let request = match ControlRequest::try_from(req.request) {
            Ok(r) => r,
            Err(_) => {
                info!("Invalid control request: {}", req.request);
                return Err(ControlError::Invalid);
            }
        };

        // Check we got an In request
        if request.direction() != dir {
            info!(
                "Ignoring request {} with direction {}",
                request,
                request.direction()
            );
            return Err(ControlError::Invalid);
        }

        Ok(request)
    }

    // Handler for OUT rquests.
    fn handle_out(&mut self, request: &ControlRequest) -> Result<(), ControlError> {
        match request {
            ControlRequest::Reset => {
                // We only want to set the action to RESET if there's no action
                // outstanding.  If there's a reset outstanding, it's a no-op.
                // If we have initialize or uninitialize, they superceed.
                PROTOCOL_ACTION.lock(|action| {
                    let current_action = action.borrow().clone();
                    if current_action.is_none() {
                        *action.borrow_mut() = Some(ProtocolAction::Reset);
                    } else {
                        info!(
                            "Reset request received - ignoring as oustanding action: {}",
                            current_action
                        );
                    }
                });
            }
            ControlRequest::Shutdown => {
                // Overwrite any existing action as this takes precedence
                PROTOCOL_ACTION
                    .lock(|action| *action.borrow_mut() = Some(ProtocolAction::Uninitialize));
            }
            ControlRequest::EnterBootloader => {
                info!("Entering DFU bootloader");
                unsafe {
                    ROM::reset_usb_boot(0, 0);
                }
                // The reboot should happen immediately, so shouldn't reach
                // here.
            }
            ControlRequest::TapBreak => info!("{}", request),
            _ => unreachable!(),
        };
        Ok(())
    }

    fn handle_in(
        &self,
        request: &ControlRequest,
        buf: &mut [u8],
        len: usize,
    ) -> Result<usize, ControlError> {
        // Check the response length is correct for this request
        let response_len = request.response_len();
        if len < response_len {
            return Err(ControlError::Invalid);
        }

        // Zero out the number of bytes for this request's response
        buf[..len].fill(0);

        // Perform any required actions, and fill in the response
        match request {
            ControlRequest::Echo => buf[0] = 0x08,
            ControlRequest::Init => {
                // Overwrite any existing action as initialize will also
                // deinitialize first (and also reset if required)
                PROTOCOL_ACTION
                    .lock(|action| *action.borrow_mut() = Some(ProtocolAction::Initialize));

                // Build the response
                buf[0] = 0x08;
                buf[1] = 0x03;
            }
            ControlRequest::GitRev => buf[..8].copy_from_slice(b"git rev\0"),
            ControlRequest::RustcVer => {
                // There is no easy way to get the rustc version from within
                // the firmware.  There are a few crates that will do it, but
                // none of them are no_std.  To do this probably requires some
                // code in build.rs to get the compiler version and then
                // some code that this module can then use.
                let rustc_ver = "rustc";
                let copy_len = core::cmp::min(len, rustc_ver.len());
                buf[..copy_len].copy_from_slice(&rustc_ver.as_bytes()[..copy_len]);
            }
            ControlRequest::PkgVer => {
                // There is no libc version or SDK version within this
                // rust implementation, so we interpret this as our Cargo.toml
                // version (i.e. the version of this crate).
                let version = env!("CARGO_PKG_VERSION");
                let copy_len = core::cmp::min(len, version.len());
                buf[..copy_len].copy_from_slice(&version.as_bytes()[..copy_len]);
            }
            _ => unreachable!(),
        };

        Ok(response_len)
    }
}

// List of Control requests that we accept.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ControlRequest {
    Echo = 0x00,
    Init = 0x01,
    Reset = 0x02,
    Shutdown = 0x03,
    EnterBootloader = 0x04,
    TapBreak = 0x05,
    GitRev = 0x06,
    RustcVer = 0x07,
    PkgVer = 0x08,
}

// Control request functions.
impl ControlRequest {
    // Returns the supported direction of the request.
    pub fn direction(&self) -> Direction {
        match self {
            ControlRequest::Echo
            | ControlRequest::Init
            | ControlRequest::GitRev
            | ControlRequest::RustcVer
            | ControlRequest::PkgVer => Direction::In,
            _ => Direction::Out,
        }
    }

    // Returns the response length of the request, for OUT requests.
    fn response_len(&self) -> usize {
        match self {
            ControlRequest::Echo => 1,
            ControlRequest::Init => 8,
            ControlRequest::GitRev => 8,
            ControlRequest::RustcVer => 8,
            ControlRequest::PkgVer => 8,
            _ => 0,
        }
    }
}

// Implements try_from to create a ControlRequest from the USB control request
// byte
impl TryFrom<u8> for ControlRequest {
    type Error = ();

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0x00 => Ok(Self::Echo),
            0x01 => Ok(Self::Init),
            0x02 => Ok(Self::Reset),
            0x03 => Ok(Self::Shutdown),
            0x04 => Ok(Self::EnterBootloader),
            0x05 => Ok(Self::TapBreak),
            0x06 => Ok(Self::GitRev),
            0x07 => Ok(Self::RustcVer),
            0x08 => Ok(Self::PkgVer),
            _ => Err(()),
        }
    }
}

// Implements Format to allow defmt to print the ControlRequest.
impl defmt::Format for ControlRequest {
    fn format(&self, f: defmt::Formatter) {
        match self {
            ControlRequest::Echo => defmt::write!(f, "Echo"),
            ControlRequest::Init => defmt::write!(f, "Init"),
            ControlRequest::Reset => defmt::write!(f, "Reset"),
            ControlRequest::Shutdown => defmt::write!(f, "Shutdown"),
            ControlRequest::EnterBootloader => defmt::write!(f, "EnterBootloader"),
            ControlRequest::TapBreak => defmt::write!(f, "TapBreak"),
            ControlRequest::GitRev => defmt::write!(f, "GitRev"),
            ControlRequest::RustcVer => defmt::write!(f, "RustcVer"),
            ControlRequest::PkgVer => defmt::write!(f, "PkgVer"),
        }
    }
}
