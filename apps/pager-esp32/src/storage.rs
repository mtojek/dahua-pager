//! Non-destructive microSD bring-up test using the LOLIN S3 HSPI wiring.

use esp_idf_svc::hal::{
    gpio::{AnyIOPin, InputPin, OutputPin},
    sd::{spi::SdSpiHostDriver, SdCardConfiguration, SdCardDriver},
    spi::{Dma, SpiDriver, SpiDriverConfig, SPI3},
};
use esp_idf_svc::{fs::fatfs::Fatfs, io::vfs::MountedFatfs};
use std::{
    fs::{self, File},
    io::{Read, Write},
    time::{SystemTime, UNIX_EPOCH},
};

const SD_SPI_KHZ: u32 = 4_000;
const TEST_CONTENT: &[u8] = b"dahua-pager-sd-self-test-v1\n";

type MountedSdFat = MountedFatfs<
    Fatfs<SdCardDriver<SdSpiHostDriver<'static, SpiDriver<'static>>>>,
>;

/// Keeps the SD-SPI host and FAT filesystem mounted for the firmware lifetime.
pub struct Storage {
    _mounted: MountedSdFat,
}

pub fn run_sd_self_test(
    spi_peripheral: SPI3<'static>,
    sck: impl OutputPin + 'static,
    mosi: impl OutputPin + 'static,
    miso: impl InputPin + 'static,
    cs: impl OutputPin + 'static,
) -> Result<Storage, String> {
    log::info!(
        "[STORAGE] initializing microSD over SPI3 at {} kHz; SCK=39 MISO=40 MOSI=41 CS=42",
        SD_SPI_KHZ
    );

    let spi = SpiDriver::new::<SPI3>(
        spi_peripheral,
        sck,
        mosi,
        Some(miso),
        &SpiDriverConfig::new().dma(Dma::Auto(4096)),
    )
    .map_err(|error| format!("SPI driver initialization failed: {error}"))?;

    let host = SdSpiHostDriver::new(
        spi,
        Some(cs),
        AnyIOPin::none(),
        AnyIOPin::none(),
        AnyIOPin::none(),
        None,
    )
    .map_err(|error| format!("SD-SPI host initialization failed: {error}"))?;

    let mut card_config = SdCardConfiguration::new();
    card_config.speed_khz = SD_SPI_KHZ;
    let card = SdCardDriver::new_spi(host, &card_config)
        .map_err(|error| format!("card initialization failed: {error}"))?;
    log::info!("[STORAGE] microSD card initialized; mounting FAT filesystem without formatting");

    let mut filesystem = Fatfs::new_sdcard(0, card)
        .map_err(|error| format!("FAT filesystem setup failed: {error}"))?;
    {
        filesystem.mount().map_err(|error| {
            format!("existing FAT volume could not be mounted (nothing formatted): {error}")
        })?;
    }
    log::info!("[STORAGE] existing FAT volume mount check passed");
    let _mounted = MountedFatfs::mount(filesystem, "/sdcard", 4)
        .map_err(|error| format!("FAT filesystem mount failed: {error}"))?;
    let root_entries = fs::read_dir("/sdcard")
        .map_err(|error| format!("mounted FAT root is not accessible through VFS: {error}"))?;
    let mut entry_count = 0;
    for entry in root_entries {
        entry.map_err(|error| format!("FAT root directory read failed: {error}"))?;
        entry_count += 1;
    }
    log::info!("[STORAGE] FAT filesystem mounted at /sdcard; root entries={entry_count}");

    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u32;
    let (path, mut file) = create_test_file(stamp)?;
    let mut created = false;

    let test_result = (|| {
        created = true;
        file.write_all(TEST_CONTENT)
            .map_err(|error| format!("test file write failed: {error}"))?;
        file.sync_all()
            .map_err(|error| format!("test file sync failed: {error}"))?;
        drop(file);
        log::info!("[STORAGE] test file written; bytes={}", TEST_CONTENT.len());

        let mut file = File::open(&path)
            .map_err(|error| format!("test file reopen failed: {error}"))?;
        let mut content = Vec::new();
        file.read_to_end(&mut content)
            .map_err(|error| format!("test file read failed: {error}"))?;
        if content != TEST_CONTENT {
            return Err(format!(
                "test file verification failed; expected={} bytes, read={} bytes",
                TEST_CONTENT.len(),
                content.len()
            ));
        }
        log::info!("[STORAGE] test file read and verified; bytes={}", content.len());
        Ok(())
    })();

    if created {
        fs::remove_file(&path).map_err(|error| {
            format!("test completed with {test_result:?}, but file removal failed: {error}")
        })?;
        log::info!("[STORAGE] temporary test file removed");
    }

    test_result?;
    log::info!("[STORAGE] microSD read/write/delete self-test passed");
    log::info!("[STORAGE] card and FAT filesystem remain mounted for runtime");
    Ok(Storage { _mounted })
}

fn create_test_file(stamp: u32) -> Result<(String, File), String> {
    for attempt in 0..32_u32 {
        let path = format!("/sdcard/PG{:06X}.TMP", stamp.wrapping_add(attempt) & 0x00FF_FFFF);
        match fs::symlink_metadata(&path) {
            Ok(_) => continue,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(format!("test file path check failed: {error}")),
        }
        let file = File::create(&path)
            .map_err(|error| format!("test file create failed: {error}"))?;
        return Ok((path, file));
    }
    Err("could not find an unused temporary test filename".into())
}
