# embassy-usb-vendor-example

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

## Build and Flashing this Example

These commands should clone this project, build it, and flash it to the Pico you have attached to your debug probe:

```bash
git clone https://github.com/piersfinlayson/embassy-usb-vendor-example
cd embassy-usb-vendor-example
cargo run --release
```

Once the image has been flashed to your device, your computer should detect a new USB device with vendor ID/product iD 0x1209/0x0f0f.  Run ```dmesg```:

```

```

## Debugging

embassy-rs applications expect to be debugged by 

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
[149353.432365] usb 1-2: New USB device found, idVendor=2e8a, idProduct=000c, bcdDevice= 2.21
[149353.432374] usb 1-2: New USB device strings: Mfr=1, Product=2, SerialNumber=3
[149353.432377] usb 1-2: Product: Debugprobe on Pico (CMSIS-DAP)
[149353.432380] usb 1-2: Manufacturer: Raspberry Pi
[149353.432382] usb 1-2: SerialNumber: E660581122334455
[149353.445045] cdc_acm 1-2:1.1: ttyACM1: USB ACM device
```

```probe-rs list``` should give output like this:

```
The following debug probes were found:
[0]: Debugprobe on Pico (CMSIS-DAP) -- 2e8a:000c:E661416677889900 (CMSIS-DAP)
```

This means you've successfully set this Pico up as a debug probe.

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
