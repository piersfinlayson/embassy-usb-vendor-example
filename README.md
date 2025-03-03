# embassy-usb-vendor-example

This project provides a working example of implementing a USB vendor class device using the Rust [`Embassy`](https://github.com/embassy-rs/embassy) project on a Raspberry Pi Pico. It was created as the basis for a required custom USB device implementation to experiment with embassy-rs, and is a complete, thorough, well commented, implementation which aims to be idiomatic and follow `Embassy` best practices.

it is likely that there are bugs, and that the author has deviated on occassion from the desired best practices and/or idiomatic Rust.  Please so feel free to submit issues/pull requests if you have any feedback to provide.

Hopefully this example is useful to others - creating it helped me learn a lot about using `Embassy`.

This implementation aims to be as identical as feasible with [`tinyusb-vendor-example`](https://github.com/piersfinlayson/tinyusb-vendor-example), implementing the same vendor and product IDs, compatible descriptor and control and bulk transfer implementations.

Where there are differences these are due to
* fundmental incompatibilities (one can't expose GCC version information when GCC wasn't used to compile the Rust code!)
* what are believed to be non-material differences between the USB stacks
* improvements in error and edge case handling.

## Quick Start

If you already have Rust, probe-rs, the RP2040 target (thumbv6m-none-eabi) and the various required build tools installed, all you need to do to build, flash and run this firmware is: 

```bash
git clone https://github.com/piersfinlayson/embassy-usb-vendor-example
cd embassy-usb-vendor-example
cargo run
```

You should see startup logs and then the following logs every ~5s:

```
...
Main loop
Bulk loop
...
```

These indicate that the primary runners are indeed running and waiting to do some work.

To test the example, use the scripts from [`tinyusb-vendor-example`](https://github.com/piersfinlayson/tinyusb-vendor-example):

```bash
git clone https://github.com/piersfinlayson/tinyusb-vendor-example
cd tinyusb-vendor-example/scripts
sudo ./install-udev-rules.sh  # To provide non-root USB device access
./pico-info.sh      # Returns some debug info via USB
./pico-init.sh      # Initializes protocol handling
./pico-shutdown.sh  # Uninitializes protocol handling
./pico-init.sh      # Re-initializes protocol handling
./pico-reset.sh     # Resets protocol handling
./pico-write.sh     # Issues a WRITE command, and "WRITE"s some data
./pico-read.sh      # Issues a READ command, and "READ"s some data
./pico-bootsel.sh   # Reboots the device into BOOTSEL (DFU) mode
``` 

## Pre-requisites

### Build Tools

You'll need various build tools to do everything in this README: 

```bash
sudo apt update
sudo apt install cmake gcc-arm-none-eabi libnewlib-arm-none-eabi build-essential libstdc++-arm-none-eabi-newlib libusb-1.0-0-dev pkg-config
```

### Debug Probe

You will need to have a debug probe in order to flash, and debug, this example.  You can find out more about this from [probe-rs](https://probe.rs).  If you want to use another Pico as a debug probe, see the section below on [Setting up a Pico Probe](#setting-up-a-pico-probe).  

### Install probe-rs

```bash
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/probe-rs/probe-rs/releases/latest/download/probe-rs-tools-installer.sh | sh
probe-rs complete install
```

### Install Rust

Skip if already installed.

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

You 

### Install Rust target support  

Assuming you have Rust already installed

```bash
rustup target add thumbv6m-none-eabi
```

## Build and Flash this Example

These commands clone this project, build it, and flash it to the Pico you have attached to your debug probe:

```bash
git clone https://github.com/piersfinlayson/embassy-usb-vendor-example
cd embassy-usb-vendor-example
cargo run
```

Once the image has been flashed to your device, your host will detect a new USB device with vendor ID/product iD 0x1209/0x0f0f.  Run ```dmesg```:

```
usb 1-1: New USB device found, idVendor=1209, idProduct=0f0f, bcdDevice= 0.10
usb 1-1: New USB device strings: Mfr=1, Product=2, SerialNumber=3
usb 1-1: Product: embassy-rs Vendor Example
usb 1-1: Manufacturer: piers.rocks
usb 1-1: SerialNumber: 000
```

## Debugging

embassy-rs applications expect to be debugged by Debug Probe.  See [Setting up a Pico Probe](#setting-up-a-pico-probe)

## Setting up a Pico Probe

Find another Pico - it can be a W, but there's no benefit, as it'll be controlled via USB.

### Building picoprobe

```bash
git clone https://github.com/raspberrypi/picoprobe
cd picoprobe
git submodule update --init
mkdir build
cd build
cmake -DDEBUG_ON_PICO=ON .. # It is really important you include -DDEBUF_ON_PICO=ON
```

### Flashing picoprobe

You'll need picotool to flash picoprobe to your Pico.  See [Installing picotool](#installing-picotool) for how to do that.  

Boot your pico into BOOTSEL mode (plugging in with button held).

```bash
picotool load debug_on_pico.elf
picotool reset
```

```dmesg``` should give output like this:

```
usb 1-2: New USB device found, idVendor=2e8a, idProduct=000c, bcdDevice= 2.21
usb 1-2: New USB device strings: Mfr=1, Product=2, SerialNumber=3
usb 1-2: Product: Debugprobe on Pico (CMSIS-DAP)
usb 1-2: Manufacturer: Raspberry Pi
usb 1-2: SerialNumber: E660581122334455
cdc_acm 1-2:1.1: ttyACM1: USB ACM device
```

```probe-rs list``` should give output like this:

```
The following debug probes were found:
[0]: Debugprobe on Pico (CMSIS-DAP) -- 2e8a:000c:E661416677889900 (CMSIS-DAP)
```

This means you've successfully set this Pico up as a debug probe.

### Set up udev

You need to set up udev so that your user will have permissions to access the probe:

```bash
cd /tmp/
wget https://probe.rs/files/69-probe-rs.rules
sudo cp /tmp/69-probe-rs.rules /etc/udev/rules.d/
sudo udevadm control --reload && sudo udevadm trigger
```

### Attaching the other Pico to your probe

You need to connect the debug probe Pico to the 3 debug pins on the Pico you want to flash this example onto.  You can find these 3 debug pins at the opposite end of the Pico to the USB connector.  They are labelled DEBUG on the top of the BOARD and SWDIO/GND/SWCLK on the bottom. 
* Probe pin 3 (GND) <-> Pico DEBUG GND
* Probe pin 4 (GP2) <-> Pico DEBUG SWCLK
* Probe pin 5 (GP3) <-> Pico DEBUG SWDIO 

Now when you run ```probe-rs info``` should give you this output (amongst some warnings about the lack of JTAG support):

```
ARM Chip with debug port Default:
No access ports found on this chip.
Debug Port: DPv2, MINDP, Designer: Raspberry Pi Trading Ltd, Part: 0x1002, Revision: 0x0, Instance: 0x0f
```

Here we can see that another Pico is connected to the probe Pico. 

## Installing picotool

To install picotool:

```bash
git clone https://github.com/raspberrypi/picotool
cd picotool
mkdir build
cd build
cmake ..
make -j 8
sudo make install
```

```picotool info``` will test you have a working picotool.  You should get output like:

```
No accessible RP-series devices in BOOTSEL mode were found.
```
