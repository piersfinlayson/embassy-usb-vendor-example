//! This is a complete, although basic, example of a (custom) USB Vendor
//! Device, developed to be run on the Raspberry Pi Pico.
//!
//! It has a C equivalent, using the Pico SDK, and TinyUSB, and can be foundh
//! [here](https://github.com/piersfinlayson/tinyusb-vendor-example)
//!
//! With probe.rs installed and configured, and a Debug Probe connected and
//! attached to the Pico, you can deploy this example with:
//!
//! ```bash
//! cargo run --release
//! ```

#![no_std]
#![no_main]

mod types;
mod constants;
mod rom;
mod control;
mod bulk;
mod protocol;

use core::cell::RefCell;
#[allow(unused_imports)]
use defmt::{debug, error, info, trace, warn};
use embassy_executor::Spawner;
use embassy_futures::select::{select, Either};
use embassy_rp::pac;
use embassy_rp::usb::{Driver as RpUsbDriver, Endpoint, In, InterruptHandler, Out};
use embassy_rp::watchdog::Watchdog;
use embassy_rp::{bind_interrupts, peripherals::USB};
use embassy_sync::blocking_mutex::{raw::CriticalSectionRawMutex, Mutex};
use embassy_time::Timer;
use embassy_usb::descriptor::{SynchronizationType, UsageType};
use embassy_usb::driver::{Driver, Endpoint as DriverEndpoint, EndpointType};
use embassy_usb::{Builder, Config, UsbDevice};
use static_cell::{ConstStaticCell, StaticCell};
use {defmt_rtt as _, panic_probe as _};

use constants::{
    IN_EP, LOOP_LOG_INTERVAL, MANUFACTURER, MAX_EP_PACKET_SIZE, MAX_PACKET_SIZE_0, OUT_EP, PRODUCT, PRODUCT_ID, SERIAL, USB_CLASS, USB_POWER_MA, USB_PROTOCOL, USB_SUB_CLASS, VENDOR_ID, WATCHDOG_TIMER
};
use protocol::ProtocolAction;
use bulk::Bulk;
use control::Control;

//
// Statics
//

// We set up statics primarily to avoid lifetime issues, and to allow us to
// spawn tasks (accessing these statics), and to split our code into
// separate modules.

// We use the WATCHDOG static to store the Watchdog object, so we can feed it
// from all of our tasks and objects.
//
// TODO - As we have multiple tasks running concurrently, we should probably
// implement a wrapper in order to ensure that one running runner (and other
// failed runners) don't keep the device alive. 
static WATCHDOG: Mutex<CriticalSectionRawMutex, RefCell<Option<Watchdog>>> =
    Mutex::new(RefCell::new(None));

// Our Control Handler handles Control requests that come in on the Control
// endpoint, and the USB stack calls control_in() and control_out() for us
// to handle them.  It also handles various USB device lifecycles events, such
// as enabled, reset, address, configured, etc.
static CONTROL: StaticCell<Control> = StaticCell::new();

// The BULK static contains our Bulk data handling object, which  a 
// Protocol Handler object.
//
// The Bulk object consists of a runner that listens for data on the OUT
// endpoint and then passes it to the ProtocolHandler to process.  It also
// runs the ProtocolHandler's runner, which listens ProtocolActions which are
// signaled by the Control object in response to Control requests from the
// host.  Think Initialize, Reset, etc.  The ProtocolHandler's runner also
// handles any data which needs to be sent to the host in response to Bulk
// commands from the host.
static BULK: StaticCell<Bulk> = StaticCell::new();

// The USB_DEVICE is primarily stored as a static to allow us to spawn a task
// using the USB runner.  (This is the runner that schedules our Control
// Handler.)  it is not used in other modules.
static USB_DEVICE: StaticCell<UsbDevice<'static, RpUsbDriver<'static, USB>>> = StaticCell::new();

// The PROTOCOL_ACTION static is a Mutex that is used to allow communication
// between the Control Handler and the Protocol Handler, so the Control
// Handler can instruct the Protocol Handler to perform actions in response to
// Control requests from the host.
static PROTOCOL_ACTION: Mutex<CriticalSectionRawMutex, RefCell<Option<ProtocolAction>>> =
    Mutex::new(RefCell::new(None));

// The following statics are used to store the USB descriptor buffers and
// control buffer.  We store them as statics to avoid lifetime issues when
// creating the USB builder.
static CONFIG_DESC: ConstStaticCell<[u8; 256]> = ConstStaticCell::new([0; 256]);
static BOS_DESC: ConstStaticCell<[u8; 256]> = ConstStaticCell::new([0; 256]);
static MSOS_DESC: ConstStaticCell<[u8; 256]> = ConstStaticCell::new([0; 256]);
static CONTROL_BUF: ConstStaticCell<[u8; 256]> = ConstStaticCell::new([0; 256]);

// Bind the hardware USB interrupt to the USB stack.  Interrupts are the
// primary mechanism the USB stack uses to receive data from hardware.
bind_interrupts!(struct Irqs {
    USBCTRL_IRQ => InterruptHandler<USB>;
});

// Our main function.  This is the entry point for our application and like
// all embedded implementations, we do not want it to exit as that would mean
// the device has halted.
#[embassy_executor::main]
async fn main(spawner: Spawner) -> ! {
    // Get device peripherals.  This gives us access to the hardware - like
    // the USB and Watchdog.
    let p = embassy_rp::init(Default::default());

    // Get the watchdog
    let watchdog = pac::WATCHDOG;

    // Get the last reset reason.  There's supposed to be a reset_reason()
    // function on Watchdog, but calling that doesn't compile;
    let reason = watchdog.reason().read();
    if reason.force() {
        info!("Last reset was forced");
    } else if reason.timer() {
        info!("Last reset was due to watchdog timer");
    } else {
        info!("Reason for last reset unknown");
    }

    // Set up the watchdog
    let mut watchdog = Watchdog::new(p.WATCHDOG);
    watchdog.start(WATCHDOG_TIMER);
    WATCHDOG.lock(|w| *w.borrow_mut() = Some(watchdog));

    // Create a new USB device
    let mut driver = RpUsbDriver::new(p.USB, Irqs);

    // Set up the USB device descriptor

    // First the VID/PID
    let mut config = Config::new(VENDOR_ID, PRODUCT_ID);
    config.manufacturer = Some(MANUFACTURER);
    config.product = Some(PRODUCT);
    config.serial_number = Some(SERIAL);
    config.max_power = USB_POWER_MA;
    config.max_packet_size_0 = MAX_PACKET_SIZE_0;

    // Set the device class, subclass, and protocol.
    config.device_class = USB_CLASS;
    config.device_sub_class = USB_SUB_CLASS;
    config.device_protocol = USB_PROTOCOL;

    // The default is composite with IADs, which gives use device class code
    // 0xEF, with is a miscellaneous device.
    config.composite_with_iads = false;

    let (ep_in, ep_out) = allocate_endpoints(&mut driver);

    // Create a USB builder
    let mut builder = Builder::new(
        driver,
        config,
        CONFIG_DESC.take(),
        BOS_DESC.take(),
        MSOS_DESC.take(),
        CONTROL_BUF.take(),
    );

    // Set up the function and interface for the Bulk class
    let mut func = builder.function(USB_CLASS, USB_SUB_CLASS, USB_PROTOCOL);
    let mut interface = func.interface();
    let if_num = interface.interface_number();
    let mut alt = interface.alt_setting(USB_CLASS, USB_SUB_CLASS, USB_PROTOCOL, None);

    // Set the endpoints to those we created above.
    alt.endpoint_descriptor(
        &ep_in.info(),
        SynchronizationType::NoSynchronization,
        UsageType::DataEndpoint,
        &[],
    );
    alt.endpoint_descriptor(
        &ep_out.info(),
        SynchronizationType::NoSynchronization,
        UsageType::DataEndpoint,
        &[],
    );

    // Drop func, to allow us to use the builder again.
    drop(func);

    // Create a handler for USB events.  We'll make it static to avoid
    // lifetime issues.
    let handler = Control::new(if_num);
    let handler = CONTROL.init(handler);

    // Set the handler.
    builder.handler(handler);

    // Build the builder.
    let usb = builder.build();
    let usb = USB_DEVICE.init(usb);

    // Now create a Bulk object, which will listen  for data on the OUT
    // endpoint and feed the watchdog.  We only feed it from Bulk::run()
    // because usb.run() is not cancel safe.
    let bulk = Bulk::new(ep_out, ep_in);
    let bulk = BULK.init(bulk);

    // Spawn a task to handle all USB activity
    match spawner.spawn(usb_task(usb, bulk)) {
        Ok(_) => (),
        Err(e) => {
            error!("Failed to spawn USB task: {}", e);
            WATCHDOG.lock(|w| {
                w.borrow_mut()
                    .as_mut()
                    .expect("Watchdog doesn't exist - can't reset")
                    .trigger_reset()
            });

            // Failed to reset.  Try differently.
            cortex_m::peripheral::SCB::sys_reset();
        }
    }

    // Log every so often
    loop {
        info!("Main loop");
        Timer::after(LOOP_LOG_INTERVAL).await;
    }
}

#[embassy_executor::task]
async fn usb_task(
    usb: &'static mut UsbDevice<'static, RpUsbDriver<'static, USB>>,
    bulk: &'static mut Bulk,
) -> ! {
    let either = select(usb.run(), bulk.run()).await;

    // If we got here one of our futures returned.  This is very bad news,
    // so we reboot the device.
    match either {
        Either::First(_) => error!("USB future returned"),
        Either::Second(_) => error!("Bulk future returned"),
    }

    WATCHDOG.lock(|w| {
        w.borrow_mut()
            .as_mut()
            .expect("Watchdog doesn't exist - can't reset")
            .trigger_reset()
    });

    // Failed to reset.  Try differently.
    cortex_m::peripheral::SCB::sys_reset();
}

fn allocate_endpoints(
    driver: &mut RpUsbDriver<'static, USB>,
) -> (Endpoint<'static, USB, In>, Endpoint<'static, USB, Out>) {
    // Allocate endpoints with specific numbers (IN: 0x83, OUT: 0x04)
    // We must do this before we move driver into Builder below.
    let ep_in: Endpoint<'static, USB, In> = loop {
        info!("Allocate an IN endpoint");
        let ep = driver
            .alloc_endpoint_in(EndpointType::Bulk, MAX_EP_PACKET_SIZE, 0)
            .expect("Unable to allocate IN endpoint");
        if ep.info().addr == IN_EP.into() {
            break ep;
        }
    };
    if ep_in.info().addr != IN_EP.into() {
        panic!("Unable to allocate 0x03 as OUT endpoint");
    }

    let ep_out: Endpoint<'static, USB, Out> = loop {
        let ep = driver
            .alloc_endpoint_out(EndpointType::Bulk, MAX_EP_PACKET_SIZE, 0)
            .expect("Unable to allocate OUT endpoint");
        if ep.info().addr == OUT_EP.into() {
            break ep;
        }
    };
    if ep_out.info().addr != OUT_EP.into() {
        panic!("Unable to allocate 0x04 as OUT endpoint");
    }

    (ep_in, ep_out)
}
