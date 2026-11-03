# ESP32-S3 firmware

This is the ESP-IDF Rust firmware for the LOLIN S3. It currently boots and
prints `Hello, world!` to the serial console; pager features are not ported yet.
Hardware details and the implementation sequence are in
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

The serial monitor should show `Hello, world!` after startup. The current
partition table reserves a 1,536,000-byte app slot for firmware development.
