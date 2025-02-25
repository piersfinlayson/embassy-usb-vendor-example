//! This module implements the Protocol handler, which decoodes and actions
//! Bulk transfers on the OUT endpoint, and returns any requested data on the
//! IN endpoint.

// Copyright (c) 2025 Piers Finlayson <piers@piers.rocks>
//
// MIT licensed - see https://opensource.org/licenses/MIT

#[allow(unused_imports)]
use defmt::{debug, error, info, trace, warn};

use embassy_rp::peripherals::USB;
use embassy_rp::usb::{Endpoint, In};
use embassy_time::Timer;
use embassy_usb::driver::EndpointIn;
use heapless::Vec;

use crate::constants::{
    MAX_EP_PACKET_SIZE, MAX_READ_SIZE, MAX_WRITE_SIZE, MAX_WRITE_SIZE_USIZE, PROTOCOL_HANDLER_TIMER,
};
use crate::PROTOCOL_ACTION;

// ProtocolState is used by ProtocolHandler to store its state.
#[derive(PartialEq)]
enum ProtocolState {
    // The protcocol handler is initialized, and will accept Bulk transfers.
    Initialized,

    // The protocol handler is uninitialized, and will not accept Bulk
    // transfers.
    Uninitialized,
}

// Implement Format for ProtocolState so it can be printed with defmt.
impl defmt::Format for ProtocolState {
    fn format(&self, f: defmt::Formatter) {
        match self {
            ProtocolState::Initialized => defmt::write!(f, "Initialized"),
            ProtocolState::Uninitialized => defmt::write!(f, "Uninitialized"),
        }
    }
}

/// ProtocolAction is used to flag to the ProtocolHandler that there is a
/// state change requested.  This is driven by the Control handler based on
/// Control transfers.
#[derive(PartialEq, Clone)]
pub enum ProtocolAction {
    Initialize,
    Uninitialize,
    Reset,
}

// Implement Format for ProtocolAction so it can be printed with defmt.
impl defmt::Format for ProtocolAction {
    fn format(&self, f: defmt::Formatter) {
        match self {
            ProtocolAction::Initialize => defmt::write!(f, "Initialize"),
            ProtocolAction::Uninitialize => defmt::write!(f, "Uninitialize"),
            ProtocolAction::Reset => defmt::write!(f, "Reset"),
        }
    }
}

// A Transfer operation.  This is a read or write of data using a Bulk
// transfer and is triggerd by a Bulk command from the Host on the OUT
// endoint.  Writes also take take on the OUT endpoint (as they are host to
// device).  Reads are sent on the IN endpoint (as they are device to host).
enum Transfer {
    Read(Read),
    Write(Write),
}

// A Write operation.  This is created when a WRITE command is received via
// the bulk OUT endpoint, used to manage the reception of the data to be
// written, and create the Status to be returned after data reception.
struct Write {
    // Number of bytes of data we are expecting the host to write to us,
    // (and we will not process any other protocol commands until that comes
    // in).
    expected_len: u16,

    // Number of bytes of write data we've received so far, and are stored
    // in data.
    received_bytes: u16,

    // A buffer to hold all of the data to be written.  As we are running on
    // an embedded device, and we are no_std, we are using heapless::Vec.
    // So long as Write is within Transfer, which is within ProtocolHandler,
    // which is within Bulk, and so long as Bulk is in a StaticCell, then
    // this Vec is allocated statically.
    data: Vec<u8, MAX_WRITE_SIZE_USIZE>,
}

impl Write {
    // Create a new Write transfer, with a buffer large enough to store all
    // possible incoming data.
    fn new(expected_len: u16) -> Result<Self, Status> {
        if expected_len < MAX_WRITE_SIZE {
            Ok(Self {
                expected_len,
                received_bytes: 0,
                data: Vec::new(),
            })
        } else {
            info!(
                "Received WRITE command for too many bytes: {}",
                expected_len
            );
            Err(Status {
                code: StatusCode::Error,
                value: expected_len as u16,
            })
        }
    }

    // Called when data has been received from the OUT endpoint for this
    // transfer.  Stores the data in the buffer.
    fn receive_data(&mut self, data: &[u8], len: u16) -> Result<(), ()> {
        if self.received_bytes + len > self.expected_len {
            return Err(());
        }

        // We know this can't fail as expected_len <= MAX_WRITE_SIZE
        let _ = self.data.extend_from_slice(&data[..(len as usize)]);
        self.received_bytes += len;
        Ok(())
    }

    // Called to find out if the WRITE operation has completed yet.  If so a
    // Some(Status) is returned, which can then be used to construct and send
    // a status response.
    fn complete(&self) -> Option<Status> {
        if self.received_bytes >= self.expected_len {
            Some(Status {
                code: StatusCode::Ok,
                value: self.received_bytes as u16,
            })
        } else {
            None
        }
    }
}

// A Read operation.  This is created when a READ command is received via
// the bulk OUT endpoint, used to manage the sending of the data to the IN
// endpoint.
struct Read {
    // Number of bytes the host is expecting us to send.  The transfer is not
    // complete until we have sent this number of bytes (or the device is
    // reset, uninitialized, or initialized.
    len: u16,

    // Total number of bytes of data which have been sent so far.
    sent_bytes: u16,

    // Buffer used to send the next set of packets out of the IN endpoint.
    // This buffer is MAX_EP_PACKET_SIZE (64) so the maximum amount of data
    // can be sent in a single chunk.
    buffer: [u8; MAX_EP_PACKET_SIZE as usize],
}

impl Read {
    // Create a new Read transfer.  As this example implementation sends dummy
    // data we refill the buffer with all 'x's.  In a real implementation this
    // would need to be enhanced.
    fn new(len: u16) -> Result<Self, ()> {
        if len <= MAX_READ_SIZE {
            Ok(Self {
                len,
                sent_bytes: 0,
                buffer: [b'x'; MAX_EP_PACKET_SIZE as usize],
            })
        } else {
            info!("Received READ command for too many bytes: {}", len);
            Err(())
        }
    }

    // Retrieves the data to be sent in the next send operation.  This
    // function will just send all 'x's until the right amount of data has
    // been supplied.  An empty buffer means no more data to send.
    fn send_data(&mut self) -> &[u8] {
        let remaining = self.len - self.sent_bytes;
        if remaining == 0 {
            return &[];
        }

        let to_send = remaining.min(MAX_EP_PACKET_SIZE as u16);
        self.sent_bytes += to_send;
        &self.buffer[..(to_send as usize)]
    }

    // Whether the transfer is complete.  Unlike Write this does not return an
    // Option<Status> as no status is returned on completion of a READ.
    fn complete(&self) -> bool {
        self.sent_bytes >= self.len
    }
}

/// ProtocolHandler handles requests that come in via bulk transfers on the
/// OUT endpoint.
///
/// It is initialized, uninitialized and reset via control requests, and these
/// are signalled via ProtocolAction.
pub struct ProtocolHandler {
    // The state of the ProtocolHandler.
    state: ProtocolState,

    // The IN endpoint to use to send status responses and READ data.
    write_ep: Endpoint<'static, USB, In>,

    // The current transfer operation.  This is either a Read or Write
    // operation, and is set when a new command is received, and cleared when
    // the operation is complete, or `Self::state` is changed.
    transfer: Option<Transfer>,
}

impl ProtocolHandler {
    /// Create a new ProtocolHandler instance.  Takes an IN (write) endpoint
    /// as argument, so it can send responses directly to the IN endpoint.
    /// The Bulk object retains the OUT (read) endpoint as this must be read
    /// from within the main Bulk future runner (which hands bulk OUT
    /// transfers).
    pub fn new(write_ep: Endpoint<'static, USB, In>) -> Self {
        Self {
            state: ProtocolState::Uninitialized,
            write_ep,
            transfer: None,
        }
    }

    /// This is a runner that checkes periodically if there's an action to
    /// perform.  It never returns - it is intended that it runs
    /// continuously once the device has been started.  
    ///
    /// This action may either be an action stored in PROTOCOL_ACTION,
    /// or data which the host has requested to be read from us, which needs
    /// to be served.
    ///
    /// PROTOCOL_ACTIONs are set by the Control object in response to the
    /// appropriate incoming host Control request.
    ///
    /// Data which has been requested by the host has been setup using a
    /// Transfer::Read type, and was initialized using `received_data()`.
    pub async fn run(&mut self) -> ! {
        loop {
            // If there is an oustanding Control action, perform it.
            self.perform_action();

            // If there is a READ transfer underway, send some data.
            self.progress_read().await;

            // Pause briefly in order to allow others to lock PROTOCOL_ACTION.
            Timer::after(PROTOCOL_HANDLER_TIMER).await;
        }
    }

    // Called from the main runner to perform an action.  This is called every
    // spin of the main runner, and will perform an action if one is
    // waiting.
    //
    // These actions primarily come from the Control object, which sets them
    // in response to Control requests from the host.
    fn perform_action(&mut self) {
        // Check whether there's an action from Control to perform.
        let action = PROTOCOL_ACTION.lock(|action| {
            // Take the action out of the shared action static
            let perform_action = action.take();
            *action.borrow_mut() = None;
            perform_action
        });

        // If there is one, perform it.
        match action {
            // We can initialize the ProtocolHandler when it's in any state.
            Some(ProtocolAction::Initialize) => self.initialize(),

            // We can unintialize the ProtocolHandler when it's in any state.
            Some(ProtocolAction::Uninitialize) => self.uninitialize(),

            // We can only reset the ProtocolHandler when it's initialized.
            Some(ProtocolAction::Reset) => {
                if self.state == ProtocolState::Initialized {
                    self.reset();
                } else {
                    info!("Received Reset action when not initialized - ignoring");
                }
            }

            // No oustanding action to perform
            None => (),
        }
    }

    // Called from the main runner to send a chunk of data from a READ
    // transfer.  This is called every spin of the main runner, and will
    // send data if a READ transfer is underway (and there is data to send).
    async fn progress_read(&mut self) {
        // Send some data if a READ transfer is outstanding.
        if let Some(Transfer::Read(read)) = self.transfer.as_mut() {
            // Get some data to send.
            let data = read.send_data();
            // If there is no data to send, this is not an error
            // condition - in a future implementation when data is
            // actually sourced from somewhere useful, we may not
            // have managed to source the data since we last sent
            // some data.
            if !data.is_empty() {
                match self.write_ep.write(data).await {
                    Ok(_) => {
                        // The transfer of this chunk of data
                        // succeeded, so see if this READ is now
                        // complete.
                        if read.complete() {
                            info!("READ complete");
                            self.transfer = None;
                        }
                    }
                    Err(_) => {
                        // Writing the data failed, so either we
                        // or the host are likely in a bad state.
                        // We'll drop the Read transfer.
                        info!("READ transfer failed - dropping READ");
                        self.transfer = None;
                    }
                }
            }
        }
    }

    /// Called when data has been received on the OUT endpoint.  This function
    /// is the main high-level protocol handler, deciding whether it is the
    /// beginning of a new command, or the continuation of a previous command,
    /// and performing the appropriate actions, specifically:
    ///
    /// * If no command (existing transfer) is underway, treat as a new
    ///   command.  A new command must be 4 bytes long.
    ///   * Byte 0 is the command (0x08 for READ, 0x09 for WRITE)
    ///   * Byte 1 is the protocol (0x10 only supported)
    ///   * Bytes 2 nd 3 contain the data length (for READ and WRITE
    ///     commands), in little endian format.
    ///   Based on the command, a new transfer of the appropriate (R/W) type
    ///   is created.
    ///  
    /// * If command (transfer) is underway, handles the incoming data as part
    ///   of that transfer.
    ///   * For a WRITE transfer, the data is read into a buffer.  Once all of
    ///     the expected data has been received, a 3 byte Status is returned
    ///     using the IN endpoint, and the current comamnd is cleared.
    ///   * For a READ transfer, the appropriate number of bytes is sent.  No
    ///     status is sent after the successful completion of a READ.
    pub async fn received_data(&mut self, data: &[u8], len: u16) {
        // Take the transfer out here, so we don't borrow self mutably in
        // the match.  We have to put it back again, if it was Some(Transfer)
        // and it hasn't been dropped, but we will let the match arm function
        // handle that.
        let transfer = self.transfer.take();
        match transfer {
            // No transfer is underway - this is the start of a new command.
            None => {
                // Parse and handle the new command.
                self.handle_new_command(data, len).await;
            }

            // A WRITE transfer is underway - handle the incoming data.
            Some(Transfer::Write(write)) => {
                // Put the transfer back in self before calling the function
                // to handle the write data, so the function can access it
                // mutably.
                self.transfer = Some(Transfer::Write(write));
                self.handle_write_data(data, len).await;
            }

            // A READ transfer is underway - ignore the incoming data because
            // only a WRITE or READ can be underway at once.  Because we're
            // here a READ is outstanding, and hence we should be sending data
            // out, not receiving it.
            Some(Transfer::Read(_)) => {
                // We're just ignoring it when we receive (WRITE) data during
                // a READ transfer, and we consider the READ transfer ongoing.
                // Hence we put tranfer back in self.
                info!("Received data during READ - ignoring");
                self.transfer = transfer;
            }
        }
    }

    // Handles a new command, by creating the appropriate transfer and, in
    // the case of a WRITE command, sending a
    async fn handle_new_command(&mut self, data: &[u8], len: u16) {
        // Parse the command and get a new Command object.  We handle errors
        // by dropping the request.
        let command = match Command::new(data, len) {
            Ok(command) => command,
            Err(_) => {
                info!("Failed to parse command - drop it");
                return;
            }
        };

        match command.command {
            // Create a new Write transfer.  If the creationf fails, we will
            // attempt to send a status response.
            CommandType::Write => {
                info!("Host to WRITE {} bytes", command.len);
                match Write::new(command.len) {
                    Ok(write) => self.transfer = Some(Transfer::Write(write)),
                    Err(status) => match self.write_ep.write(&status.to_bytes()).await {
                        Ok(_) => info!("Failed to handle WRITE - sent error response"),
                        Err(_) => info!("Failed to send WRITE error response"),
                    },
                }
            }
            // Create a new Read transfer.  If the creation fails, we silently
            // drop the request, as Read commands do no support a status
            // response.
            CommandType::Read => {
                info!("Host to READ {} bytes", command.len);
                match Read::new(command.len) {
                    Ok(read) => self.transfer = Some(Transfer::Read(read)),
                    Err(_) => info!("Failed to handle READ - drop it"),
                }
            }
        }
    }

    // Handles receipt of a chunk of data from the host for a WRITE transfer.
    // This function demands that self.transfer is of type Transfer::Write.
    async fn handle_write_data(&mut self, data: &[u8], len: u16) {
        let transfer = self
            .transfer
            .as_mut()
            .expect("Internal error - expected WRITE transfer");
        let write = match transfer {
            Transfer::Write(write) => write,
            _ => unreachable!(),
        };

        // Handle the incoming data using the Transfer object.
        match write.receive_data(data, len) {
            Ok(_) => {
                if let Some(status) = write.complete() {
                    // Write complete - try to send a status response.
                    info!("WRITE complete");
                    match self.write_ep.write(&status.to_bytes()).await {
                        Ok(_) => info!("Sent WRITE response"),
                        Err(_) => info!("Failed to send WRITE response"),
                    }

                    self.transfer = None
                } else {
                    // Incomplete - leave transfer in place.
                }
            }
            Err(_) => {
                // The only error from write.receive() is if too much data
                // was received.
                info!("Too much data received - dropping WRITE");
                self.transfer = None;
            }
        }
    }

    // (Re-)Initialize the ProtocolHandler.  Any oustanding transfer is
    // cleared.
    fn initialize(&mut self) {
        info!("Protocol Handler - initialized");
        self.state = ProtocolState::Initialized;
        self.transfer = None
    }

    // Uninitialize the ProtocolHandler.  Any outstanding transfer is cleared.
    fn uninitialize(&mut self) {
        info!("Protocol Handler - uninitialized");
        self.state = ProtocolState::Uninitialized;
        self.transfer = None
    }

    // Reset the ProtocolHandler.  Any outstanding transfer is cleared.
    fn reset(&mut self) {
        info!("Protocol Handler - reset");
        self.transfer = None
    }
}

// The type of Command received from the host.
pub enum CommandType {
    // The host is requesting data from the device.
    Read = 0x08,

    // The host is sending data to the device.
    Write = 0x09,
}

// Implement TryFrom for CommandType so we can parse an imcoming byte into a
// CommandType.
impl TryFrom<u8> for CommandType {
    type Error = ();

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0x08 => Ok(Self::Read),
            0x09 => Ok(Self::Write),
            _ => Err(()),
        }
    }
}

/// A Command object is created from the incoming Bulk data, and used to
/// determine the type of command, the protocol, and the length of any data
/// associated with the command.
pub struct Command {
    // The type of command.  This is the first byte of the incoming command
    // packet.
    command: CommandType,

    // The protocol used by the command.  This is the second byte of the
    // command packet.
    _protocol: Protocol,

    // The length of any data associated with the command.  This is encoded
    // in the third and fourth bytes of the command packet as a little endian
    // u16.
    len: u16,
}

impl Command {
    /// The length
    const LEN: u16 = 4;

    /// Create a new Command object from an incoming data packet.
    ///
    /// This function checks the data length and contents are valid, and, if
    /// so, return a Command object.
    pub fn new(bytes: &[u8], size: u16) -> Result<Self, ()> {
        // Check the data size is the length of a command.
        if size != Self::LEN {
            info!("Unexpected data length: {}", size);
            return Err(());
        }

        // Check the buffer has enough bytes in it.
        if bytes.len() < size as usize {
            info!("Too few bytes to parse command: {} < {}", bytes.len(), size);
            return Err(());
        }

        // Get the command itself.
        let command = match CommandType::try_from(bytes[0]) {
            Ok(cmd) => cmd,
            Err(_) => return Err(()),
        };

        // Check the protocol is supported.
        let _protocol = match Protocol::try_from(bytes[1]) {
            Ok(proto) => proto,
            Err(_) => return Err(()),
        };

        // Get the length of any data associated with this command.
        let len = u16::from_le_bytes([bytes[2], bytes[3]]);

        // Return the new Command object.
        Ok(Self {
            command,
            _protocol,
            len,
        })
    }
}

/// Supported Bulk command protocols.
pub enum Protocol {
    // The only currently supported protocol.
    Default = 0x10,
}

/// Implement TryFrom for Protocol in order to parse an incoming byte into a
/// Protocol.
impl TryFrom<u8> for Protocol {
    type Error = ();

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0x10 => Ok(Self::Default),
            _ => Err(()),
        }
    }
}

/// The status code used in the response to certain terns of Bulk Commands.
#[repr(u8)]
#[derive(Clone, defmt::Format)]
#[allow(dead_code)]
pub enum StatusCode {
    /// The device is busy.  The host may retry later.
    Busy = 0x01,

    /// The command was successful.
    Ok = 0x02,

    /// The command failed, or was invalid.
    Error = 0x03,
}

/// A Status object is used to return a status response to the host after
/// certin commands have been received and handled.  It is returned as
/// stream of bytes on the IN endpoint.
pub struct Status {
    /// The status code to return.
    code: StatusCode,

    /// The value to return.  This is typically the number of bytes
    /// processed, or an error code.
    value: u16,
}

impl Status {
    /// Convert the Status object into a stream of bytes to be sent on the IN
    /// endpoint.
    pub fn to_bytes(&self) -> [u8; 3] {
        let mut bytes = [0; 3];
        bytes[0] = self.code.clone() as u8;
        bytes[1..3].copy_from_slice(&self.value.to_le_bytes());
        bytes
    }
}
