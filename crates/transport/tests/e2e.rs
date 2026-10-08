//! End-to-end test against a real phone service.
//!
//! Ignored by default; opt in with environment variables:
//!
//! ```text
//! ANDROID18_E2E_URL=http://192.168.1.42:8080 \
//! ANDROID18_E2E_TOKEN=<pairing token> \
//! cargo test -p android18-transport --test e2e -- --ignored
//! ```

use android18_core::port::DeviceBackend;
use android18_transport::HttpDevice;

#[test]
#[ignore = "requires a live phone: set ANDROID18_E2E_URL and ANDROID18_E2E_TOKEN"]
fn live_device_round_trip() {
    let Ok(url) = std::env::var("ANDROID18_E2E_URL") else {
        panic!("ANDROID18_E2E_URL is not set");
    };
    let Ok(token) = std::env::var("ANDROID18_E2E_TOKEN") else {
        panic!("ANDROID18_E2E_TOKEN is not set");
    };
    let device = HttpDevice::new(url, &token).expect("build http device");

    let info = futures::executor::block_on(device.device_info()).expect("device_info");
    assert!(!info.id.is_empty(), "phone must report an id");

    let entries =
        futures::executor::block_on(device.list("/storage/emulated/0", &token)).expect("list");
    assert!(!entries.is_empty(), "storage root must have children");

    let logs = device.request_log();
    assert!(logs.iter().any(|entry| entry.endpoint.starts_with("/info")));
    assert!(logs.iter().any(|entry| entry.endpoint.starts_with("/list")));
}
