# ESP32-S3 firmware

This is the ESP-IDF Rust firmware for the LOLIN S3. At startup, it briefly
lights the red and green status LEDs, then initializes the ILI9488 480x320 LCD
and displays white, red, green, blue, and black test bands. It monitors the
four action buttons, joystick button, and joystick axes, logging stable button
state changes and meaningful axis movement. Other pager hardware and
application features are not ported yet. Hardware details and the
implementation sequence are in
[`docs/esp32-port-plan.md`](../../docs/esp32-port-plan.md).

## Build

Install `espup`, then install the Xtensa toolchain and the build tools:

```sh
espup install
cargo install cargo-espflash
cargo install ldproxy
```

From this directory, load the environment created by `espup` and build:

```sh
. "$HOME/export-esp.sh"
cargo build
```

The project uses ESP-IDF `v6.1`. `esp-idf-sys` downloads and builds that version
on the first build. `ESP_IDF_TOOLS_INSTALL_DIR=global` keeps the ESP-IDF tools
in the standard global Espressif installation directory.

## Flash and monitor

Connect the ESP32-S3 over USB, then run from this directory:

```sh
cargo espflash flash --target xtensa-esp32s3-espidf --monitor
```

The serial monitor logs the LCD reset and initialization steps, confirms when
the color pattern is drawn, and reports uptime and free heap every 10 seconds.
The LCD wiring matches the known-working Arduino project: CS GPIO3, DC GPIO10,
reset GPIO9, backlight GPIO14, MOSI GPIO11, and SCK GPIO12. The current
SPI clock is 40 MHz. The current partition table reserves a 1,536,000-byte app
slot for firmware development.
