//! Reference seed data ported from the web prototype (`INITIAL_ENTRIES`),
//! rebranded to Android18. Relative modification times are anchored to a
//! `now_ms` parameter so tests stay hermetic.

use crate::domain::device::{Device, DeviceStatus, Transport};
use crate::domain::entry::{ColorTag, Entry};
use crate::fs::paths::STORAGE_ROOT;

const MIN: i64 = 60_000;
const HOUR: i64 = 60 * MIN;
const DAY: i64 = 24 * HOUR;

struct Seed {
    now_ms: i64,
    entries: Vec<Entry>,
}

impl Seed {
    fn ago(&self, duration: i64) -> i64 {
        self.now_ms - duration
    }

    fn dir(&mut self, path: String, ago: i64, color: Option<ColorTag>, pinned: bool) {
        self.entries
            .push(Entry::dir(&path, self.ago(ago), color, pinned));
    }

    fn file(&mut self, path: String, size: u64, ago: i64, mime: &str, content: Option<String>) {
        self.entries
            .push(Entry::file(&path, size, self.ago(ago), Some(mime), content));
    }
}

/// Builds the seeded dataset (root included) anchored at `now_ms`.
pub fn seed_entries(now_ms: i64) -> Vec<Entry> {
    let mut s = Seed {
        now_ms,
        entries: Vec::new(),
    };
    let root = format!("{STORAGE_ROOT}/");
    let dcim = format!("{root}DCIM");
    let documents = format!("{root}Documents");
    let download = format!("{root}Download");
    let music = format!("{root}Music");
    let pictures = format!("{root}Pictures");
    let recordings = format!("{root}Recordings");
    let podcasts = format!("{root}Podcasts");

    // Storage root itself.
    s.entries.push(Entry {
        name: "0".into(),
        path: STORAGE_ROOT.into(),
        dir: true,
        size: 4096,
        mtime: now_ms,
        mime_type: None,
        extension: None,
        item_count: None,
        is_pinned: false,
        color_tag: None,
        content: None,
    });

    // Root folders.
    s.dir(dcim.clone(), 2 * HOUR, Some(ColorTag::Amber), true);
    s.dir(documents.clone(), 3 * DAY, Some(ColorTag::Blue), true);
    s.dir(download.clone(), 45 * MIN, Some(ColorTag::Emerald), true);
    s.dir(music.clone(), 10 * DAY, Some(ColorTag::Purple), true);
    s.dir(pictures.clone(), 5 * DAY, Some(ColorTag::Rose), true);
    s.dir(podcasts.clone(), 14 * DAY, None, false);
    s.dir(recordings.clone(), 12 * HOUR, None, false);
    s.dir(
        format!("{root}Android"),
        30 * DAY,
        Some(ColorTag::Slate),
        false,
    );

    // DCIM.
    s.dir(
        format!("{dcim}/Camera"),
        2 * HOUR,
        Some(ColorTag::Amber),
        true,
    );
    s.file(
        format!("{dcim}/Camera/IMG_20261001_143022.jpg"),
        4_210_800,
        2 * DAY,
        "image/jpeg",
        None,
    );
    s.file(
        format!("{dcim}/Camera/IMG_20261002_091510.jpg"),
        3_840_120,
        DAY,
        "image/jpeg",
        None,
    );
    s.file(
        format!("{dcim}/Camera/VID_20260928_182045.mp4"),
        89_420_000,
        5 * DAY,
        "video/mp4",
        None,
    );
    s.dir(format!("{dcim}/Screenshots"), 18 * HOUR, None, false);
    s.file(
        format!("{dcim}/Screenshots/Screenshot_2026-10-02-114512.png"),
        1_240_500,
        18 * HOUR,
        "image/png",
        None,
    );
    s.file(
        format!("{dcim}/Screenshots/Screenshot_2026-09-30-081200.png"),
        980_400,
        3 * DAY,
        "image/png",
        None,
    );

    // Documents.
    s.dir(
        format!("{documents}/Projects"),
        6 * HOUR,
        Some(ColorTag::Blue),
        false,
    );
    s.dir(
        format!("{documents}/Obsidian Vault"),
        14 * HOUR,
        Some(ColorTag::Purple),
        true,
    );
    s.dir(
        format!("{documents}/Receipts & Taxes"),
        7 * DAY,
        None,
        false,
    );
    s.file(
        format!("{documents}/Projects/Quarterly_Budget_FY27.xlsx"),
        48_300,
        6 * HOUR,
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        None,
    );
    s.file(
        format!("{documents}/Product_Roadmap_Q4.md"),
        14_200,
        DAY,
        "text/markdown",
        Some(
            "# Android18 Q4 Roadmap\n\n- Phone-as-Server: Ktor CIO\n- AI Semantic Search (Gemini 2.0 Flash)\n- Interactive Terminal CLI\n"
                .into(),
        ),
    );
    s.file(
        format!("{documents}/Obsidian Vault/Android18 Architecture.md"),
        3_800,
        14 * HOUR,
        "text/markdown",
        Some(
            "# Android18\n\nPhone-as-server: one plain HTTP/JSON API on the phone, native GPUI Kit desktop client.\n\n- Transport-agnostic: a transport is just a base URL.\n- Wi-Fi (mDNS `_android18._tcp`) and USB (adb forward) from day one.\n"
                .into(),
        ),
    );
    s.file(
        format!("{documents}/Obsidian Vault/Sync Notes.md"),
        1_250,
        15 * HOUR,
        "text/markdown",
        Some("- Pairing via QR planned for R7\n- Token auth: `X-Auth` header\n".into()),
    );
    s.file(
        format!("{documents}/Obsidian Vault/Vault TODO.md"),
        890,
        16 * HOUR,
        "text/markdown",
        Some(
            "- [ ] Migrate dashboard charts\n- [ ] Thumbnail cache keyed by (path, mtime)\n".into(),
        ),
    );
    s.file(
        format!("{documents}/Receipts & Taxes/receipt_october.pdf"),
        142_000,
        7 * DAY,
        "application/pdf",
        None,
    );
    s.file(
        format!("{documents}/Receipts & Taxes/tax_fy26_summary.pdf"),
        842_000,
        7 * DAY,
        "application/pdf",
        None,
    );

    // Root files.
    s.file(
        format!("{root}system_log.txt"),
        1_420,
        30 * MIN,
        "text/plain",
        Some(
            "[Ktor CIO] Server started on 0.0.0.0:8080\n[NsdManager] Registered service _android18._tcp on port 8080\n[TokenStore] Auth token validated (24 bytes)\n[Permission] MANAGE_EXTERNAL_STORAGE: GRANTED\n[Storage] Root mounted at /storage/emulated/0\n[Transfer] Listening for inbound desktop connections..."
                .into(),
        ),
    );

    // Download.
    s.file(
        format!("{download}/android18-android-debug.apk"),
        18_420_000,
        45 * MIN,
        "application/vnd.android.package-archive",
        None,
    );
    s.file(
        format!("{download}/firmware_update_v4.2.bin"),
        64_100_000,
        12 * HOUR,
        "application/octet-stream",
        None,
    );
    s.file(
        format!("{download}/dataset_telemetry_export.csv"),
        3_410_000,
        30 * HOUR,
        "text/csv",
        Some(
            "timestamp,sensor_id,voltage,current,temperature_c\n1770000001,sns_01,3.298,0.412,24.2\n1770000002,sns_01,3.297,0.415,24.3\n1770000003,sns_01,3.301,0.411,24.1".into(),
        ),
    );
    s.file(
        format!("{download}/Client_Brief_Revision_C.pdf"),
        2_150_000,
        48 * HOUR,
        "application/pdf",
        None,
    );

    // Music.
    s.dir(
        format!("{music}/Ambient Sessions"),
        10 * DAY,
        Some(ColorTag::Purple),
        false,
    );
    s.file(
        format!("{music}/Ambient Sessions/01_Solar_Drift.flac"),
        32_400_000,
        10 * DAY,
        "audio/flac",
        None,
    );
    s.file(
        format!("{music}/Ambient Sessions/02_Echoes_in_Orbit.mp3"),
        9_820_000,
        10 * DAY,
        "audio/mpeg",
        None,
    );

    // Pictures.
    s.dir(
        format!("{pictures}/Wallpapers"),
        5 * DAY,
        Some(ColorTag::Rose),
        false,
    );
    s.file(
        format!("{pictures}/Wallpapers/Minimal_Monochrome_4K.png"),
        6_240_000,
        5 * DAY,
        "image/png",
        None,
    );
    s.file(
        format!("{pictures}/Wallpapers/Architecture_Grid_Light.jpg"),
        4_100_000,
        4 * DAY,
        "image/jpeg",
        None,
    );

    // Recordings.
    s.file(
        format!("{recordings}/VoiceMemo_ArchitectureReview.m4a"),
        14_200_000,
        12 * HOUR,
        "audio/mp4",
        None,
    );
    s.file(
        format!("{recordings}/VoiceMemo_ProductRoadmap.m4a"),
        8_700_000,
        36 * HOUR,
        "audio/mp4",
        None,
    );

    // Podcasts.
    s.file(
        format!("{podcasts}/android18_deep_dive_ep01.mp3"),
        42_000_000,
        14 * DAY,
        "audio/mpeg",
        None,
    );

    s.entries
}

/// The demo devices shown in the connect dialog (one per transport).
pub fn mock_devices() -> Vec<Device> {
    vec![
        Device {
            id: "pixel-9-pro-wifi".into(),
            name: "Google Pixel 9 Pro".into(),
            model: "Pixel 9 Pro (Tensor G4)".into(),
            transport: Transport::Wifi,
            base_url: "http://192.168.1.142:8080".into(),
            token: "7f9c2d1b84e035a6bc8910fedcba4321".into(),
            status: DeviceStatus::Connected,
            ip_address: Some("192.168.1.142".into()),
            port: 8080,
            adb_serial: None,
            storage_used_bytes: 68_450_000_000,
            storage_total_bytes: 256_000_000_000,
            battery_percent: Some(88),
            android_version: "Android 15 (API 35)".into(),
        },
        Device {
            id: "galaxy-s24-ultra-usb".into(),
            name: "Samsung Galaxy S24 Ultra".into(),
            model: "SM-S928B (Snapdragon 8 Gen 3)".into(),
            transport: Transport::Usb,
            base_url: "http://127.0.0.1:43121".into(),
            token: "3a8d19f2c67b4e01a5d93e8f2107bc64".into(),
            status: DeviceStatus::Found,
            ip_address: Some("127.0.0.1".into()),
            port: 43121,
            adb_serial: Some("RFCT10XYZ99".into()),
            storage_used_bytes: 112_300_000_000,
            storage_total_bytes: 512_000_000_000,
            battery_percent: Some(94),
            android_version: "Android 14 (API 34)".into(),
        },
        Device {
            id: "galaxy-tab-s9-wifi".into(),
            name: "Samsung Galaxy Tab S9".into(),
            model: "SM-X710 (Snapdragon 8 Gen 2)".into(),
            transport: Transport::Wifi,
            base_url: "http://192.168.1.189:8080".into(),
            token: "89bc34de7102f5a6c8d9012ef3ab4578".into(),
            status: DeviceStatus::Found,
            ip_address: Some("192.168.1.189".into()),
            port: 8080,
            adb_serial: None,
            storage_used_bytes: 42_100_000_000,
            storage_total_bytes: 128_000_000_000,
            battery_percent: Some(71),
            android_version: "Android 14 (API 34)".into(),
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seed_is_rooted_and_unique() {
        let entries = seed_entries(1_000_000_000);
        assert!(entries.iter().any(|e| e.path == STORAGE_ROOT));
        let mut paths: Vec<&str> = entries.iter().map(|e| e.path.as_str()).collect();
        paths.sort_unstable();
        let count = paths.len();
        paths.dedup();
        assert_eq!(paths.len(), count, "seed paths must be unique");
    }

    #[test]
    fn seed_has_the_reference_shape() {
        let entries = seed_entries(1_000_000_000);
        assert!(
            entries
                .iter()
                .any(|e| e.path.ends_with("/DCIM") && e.is_pinned)
        );
        assert!(
            entries
                .iter()
                .any(|e| e.name == "Product_Roadmap_Q4.md" && e.content.is_some())
        );
        assert!(
            entries
                .iter()
                .any(|e| e.extension.as_deref() == Some("apk"))
        );
    }
}
