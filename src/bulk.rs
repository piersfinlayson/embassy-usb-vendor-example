#[allow(unused_imports)]
use defmt::{debug, error, info, trace, warn};
use embassy_futures::select::{select, select3, Either, Either3};
use embassy_rp::peripherals::USB;
use embassy_rp::usb::{Endpoint, In, Out};
use embassy_time::{Instant, Timer};
use embassy_usb::driver::{Endpoint as DriverEndpoint, EndpointOut};
use crate::constants::{LOOP_LOG_INTERVAL, MAX_EP_PACKET_SIZE, MAX_WRITE_SIZE_USIZE, WATCHDOG_FEED_TIMER};
use crate::protocol::ProtocolHandler;
use crate::WATCHDOG;

/// The Bulk object contains the runner which handles bulk transfers on the
/// OUT endpoint.
///
/// A simple example:
///
/// ```ignore
/// let bulk = Bulk::new(ep_out, ep_in);
/// bulk.run().await();
/// ```
pub struct Bulk {
    read_ep: Endpoint<'static, USB, Out>,
    protocol: ProtocolHandler,
}

impl Bulk {
    /// Creates a new instance of the Bulk object.
    ///
    /// # Arguments
    /// - `out_ep` - OUT endpoint
    /// - `in_ep` = IN ep
    ///
    /// # Returns
    /// `Self`
    pub fn new(out_ep: Endpoint<'static, USB, Out>, in_ep: Endpoint<'static, USB, In>) -> Self {
        Self {
            read_ep: out_ep,
            protocol: ProtocolHandler::new(in_ep),
        }
    }

    /// Runs the OUT bulk handler, by
    /// * waiting until the OUT endpoint is enabled
    /// * reading in any data
    /// * handling it (echoing it back or some other handling)
    pub async fn run(&mut self) -> ! {
        let mut next_log_instant = Instant::now();
        loop {
            let now = Instant::now();
            if now >= next_log_instant {
                info!("Bulk loop");
                next_log_instant = now + LOOP_LOG_INTERVAL;
            }

            // We need to pause from waiting for the endpoint to be enabled
            // so we can feed the watchdog
            let either = select(
                self.read_ep.wait_enabled(),
                Timer::after(WATCHDOG_FEED_TIMER),
            )
            .await;

            // Feed the watchdog - whichever result we got.  We do this before
            // we process any endpoint activity, in case that takes a while.
            WATCHDOG.lock(|w| {
                w.borrow_mut()
                    .as_mut()
                    .expect("Watchdog doesn't exist - can't feed it")
                    .feed()
            });

            // If the endpoint was enabled, attempt to read in the data.
            if let Either::First(_) = either {
                info!("OUT Endpoint enabled");
                loop {
                    let now = Instant::now();
                    if now >= next_log_instant {
                        info!("Bulk loop");
                        next_log_instant = now + LOOP_LOG_INTERVAL;
                    }

                    // Set up a buffer to read data into.
                    let mut data = [0; MAX_EP_PACKET_SIZE as usize];

                    // Again, we need to pause from waiting for data, so we
                    // can feed the watchdog.
                    let either = select3(
                        self.read_ep.read(&mut data),
                        self.protocol.run(),
                        Timer::after(WATCHDOG_FEED_TIMER),
                    )
                    .await;

                    // Feed the watchog before doing anything else.
                    WATCHDOG.lock(|w| w.borrow_mut().as_mut().unwrap().feed());

                    if let Either3::First(Ok(size)) = either {
                        // We got bulk data.  Handle it.
                        if size <= MAX_WRITE_SIZE_USIZE {
                            self.protocol.received_data(&data, size as u16).await;
                        } else {
                            info!("Received more data than we can handle {} bytes - dropping", size);
                        }

                        // Try to read more data
                        continue;
                    } else if let Either3::Second(_) = either {
                        // Protocol handler exited.  It shouldn't do that.
                        info!("Protocol handler exited - resetting");

                        // Trigger a reset
                        WATCHDOG.lock(|w| {
                            w.borrow_mut()
                                .as_mut()
                                .expect("Watchdog doesn't exist - can't reset")
                                .trigger_reset()
                        });

                        // Failed to reset.  Try differently.
                        cortex_m::peripheral::SCB::sys_reset();
                    } else if let Either3::Third(_) = either {
                        // The timer expired, so try and read data again.
                        continue;
                    }

                    // We hit an error reading, so we're done reading from
                    // the endpoint this time.
                    break;
                }
                info!("Finished processing data on OUT Endpoint");
            }
        }
    }
}