//! Paired Bluetooth devices on this Mac.

#[cfg(target_os = "macos")]
use std::collections::HashSet;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PairedDevice {
    pub address: String,
    pub name: String,
    pub kind: &'static str,
    pub connected: bool,
}

/// Eight seconds. A Bluetooth page slot is 0.625 ms, so 0x3200 is one try.
const PAGE_SLOTS: u16 = 0x3200;

pub fn disconnect_all_then<R: Send + 'static>(
    addresses: &[String],
    then: impl FnOnce(bool) -> R + Send + 'static,
) -> R {
    #[cfg(target_os = "macos")]
    {
        let addresses = addresses.to_vec();
        on_main(move || {
            for address in &addresses {
                close_here(address);
            }
            let gone = addresses.iter().all(|address| !connected_here(address));
            then(gone)
        })
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = addresses;
        then(false)
    }
}

pub fn connect_all(addresses: &[String]) -> bool {
    #[cfg(target_os = "macos")]
    {
        let addresses = addresses.to_vec();
        on_main(move || {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(16);
            loop {
                let ready = addresses.iter().all(|address| connect_here(address));
                if ready || std::time::Instant::now() >= deadline {
                    return ready;
                }
            }
        })
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = addresses;
        false
    }
}

#[cfg(target_os = "macos")]
fn connected_here(address: &str) -> bool {
    use objc::{msg_send, sel, sel_impl};
    with_device(address, |device| unsafe {
        let connected: bool = msg_send![device, isConnected];
        connected
    })
    .unwrap_or(false)
}

#[cfg(target_os = "macos")]
fn close_here(address: &str) {
    use objc::{msg_send, sel, sel_impl};
    let _ = with_device(address, |device| unsafe {
        let result: i32 = msg_send![device, closeConnection];
        result == 0
    });
}

#[cfg(target_os = "macos")]
fn connect_here(address: &str) -> bool {
    use objc::runtime::Object;
    use objc::{msg_send, sel, sel_impl};
    if connected_here(address) {
        return true;
    }
    let _ = with_device(address, |device| unsafe {
        let target: *mut Object = std::ptr::null_mut();
        let result: i32 = msg_send![
            device,
            openConnection: target
            withPageTimeout: PAGE_SLOTS
            authenticationRequired: false
        ];
        result == 0
    });
    connected_here(address)
}

#[cfg(target_os = "macos")]
fn on_main<R: Send + 'static>(work: impl FnOnce() -> R + Send + 'static) -> R {
    use block::ConcreteBlock;

    unsafe extern "C" {
        fn pthread_main_np() -> i32;
        fn dispatch_get_main_queue() -> *mut std::ffi::c_void;
        fn dispatch_sync(queue: *mut std::ffi::c_void, block: *const std::ffi::c_void);
    }

    if unsafe { pthread_main_np() } != 0 {
        return work();
    }
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    let work = std::sync::Mutex::new(Some(work));
    let block = ConcreteBlock::new(move || {
        let Some(work) = work.lock().ok().and_then(|mut slot| slot.take()) else {
            return;
        };
        let _ = tx.send(work());
    });
    let block = block.copy();
    unsafe {
        dispatch_sync(
            dispatch_get_main_queue(),
            &*block as *const block::Block<(), ()> as *const std::ffi::c_void,
        );
    }
    rx.recv().unwrap_or_else(|_| {
        panic!("Bluetooth work on the main thread did not finish");
    })
}

pub fn paired_devices() -> Result<Vec<PairedDevice>, String> {
    #[cfg(target_os = "macos")]
    {
        paired_devices_mac()
    }
    #[cfg(not(target_os = "macos"))]
    {
        Err("Paired Bluetooth devices are listed on a Mac.".into())
    }
}

pub(crate) fn kind_label(major: u32, minor: u32) -> &'static str {
    const COMPUTER: u32 = 0x01;
    const PHONE: u32 = 0x02;
    const NETWORK: u32 = 0x03;
    const AUDIO: u32 = 0x04;
    const PERIPHERAL: u32 = 0x05;
    const IMAGING: u32 = 0x06;
    const WEARABLE: u32 = 0x07;
    const TOY: u32 = 0x08;
    const HEALTH: u32 = 0x09;
    const KEYBOARD: u32 = 0x10;
    const POINTING: u32 = 0x20;
    const COMBO: u32 = 0x30;

    match major {
        COMPUTER => "Computer",
        PHONE => "Phone",
        NETWORK => "Network",
        AUDIO => "Audio",
        IMAGING => "Camera or printer",
        WEARABLE => "Wearable",
        TOY => "Toy",
        HEALTH => "Health",
        PERIPHERAL => match minor & 0x30 {
            COMBO => "Keyboard and mouse",
            KEYBOARD => "Keyboard",
            POINTING => match minor & 0x0f {
                0x01 => "Joystick",
                0x02 => "Gamepad",
                0x03 => "Remote",
                0x04 => "Sensor",
                0x05 => "Tablet",
                0x07 => "Pen",
                _ => "Mouse",
            },
            _ => match minor & 0x0f {
                0x01 => "Joystick",
                0x02 => "Gamepad",
                0x03 => "Remote",
                0x05 => "Tablet",
                _ => "Peripheral",
            },
        },
        _ => "Bluetooth",
    }
}

#[cfg(target_os = "macos")]
fn paired_devices_mac() -> Result<Vec<PairedDevice>, String> {
    use objc::runtime::{Class, Object};
    use objc::{class, msg_send, sel, sel_impl};

    load_framework();
    let Some(device_class) = Class::get("IOBluetoothDevice") else {
        return Err("Hop could not read Bluetooth.".into());
    };

    unsafe {
        let pool: *mut Object = msg_send![class!(NSAutoreleasePool), new];
        let list: *mut Object = msg_send![device_class, pairedDevices];
        let devices = if list.is_null() {
            Vec::new()
        } else {
            read_devices(list)
        };
        if !pool.is_null() {
            let _: () = msg_send![pool, drain];
        }
        Ok(devices)
    }
}

#[cfg(target_os = "macos")]
unsafe fn read_devices(list: *mut objc::runtime::Object) -> Vec<PairedDevice> {
    use objc::runtime::Object;
    use objc::{msg_send, sel, sel_impl};

    let count: usize = unsafe { msg_send![list, count] };
    let mut devices = Vec::with_capacity(count);
    let mut seen = HashSet::new();
    for index in 0..count {
        let device: *mut Object = unsafe { msg_send![list, objectAtIndex: index] };
        if device.is_null() {
            continue;
        }
        let Some(address) = (unsafe { ns_string(msg_send![device, addressString]) }) else {
            continue;
        };
        let address = address.trim().to_ascii_lowercase();
        if address.is_empty() || !seen.insert(address.clone()) {
            continue;
        }
        let name = unsafe { ns_string(msg_send![device, nameOrAddress]) }
            .unwrap_or_else(|| address.clone());
        let name = if name.trim().is_empty() {
            address.clone()
        } else {
            name
        };
        let major: u32 = unsafe { msg_send![device, deviceClassMajor] };
        let minor: u32 = unsafe { msg_send![device, deviceClassMinor] };
        let connected: bool = unsafe { msg_send![device, isConnected] };
        devices.push(PairedDevice {
            address,
            name,
            kind: kind_label(major, minor),
            connected,
        });
    }
    devices.sort_by(|left, right| {
        left.name
            .to_lowercase()
            .cmp(&right.name.to_lowercase())
            .then(left.address.cmp(&right.address))
    });
    devices
}

#[cfg(target_os = "macos")]
unsafe fn ns_string(value: *mut objc::runtime::Object) -> Option<String> {
    use objc::{msg_send, sel, sel_impl};
    use std::ffi::CStr;

    if value.is_null() {
        return None;
    }
    let bytes: *const i8 = unsafe { msg_send![value, UTF8String] };
    if bytes.is_null() {
        return None;
    }
    Some(
        unsafe { CStr::from_ptr(bytes) }
            .to_string_lossy()
            .into_owned(),
    )
}

#[cfg(target_os = "macos")]
fn with_device<T>(address: &str, f: impl FnOnce(*mut objc::runtime::Object) -> T) -> Option<T> {
    use objc::runtime::{Class, Object};
    use objc::{class, msg_send, sel, sel_impl};

    load_framework();
    let device_class = Class::get("IOBluetoothDevice")?;
    let want = crate::wire::canon(address);
    if want.is_empty() {
        return None;
    }
    unsafe {
        let pool: *mut Object = msg_send![class!(NSAutoreleasePool), new];
        let list: *mut Object = msg_send![device_class, pairedDevices];
        let mut found = None;
        if !list.is_null() {
            let count: usize = msg_send![list, count];
            for index in 0..count {
                let device: *mut Object = msg_send![list, objectAtIndex: index];
                if device.is_null() {
                    continue;
                }
                let Some(have) = ns_string(msg_send![device, addressString]) else {
                    continue;
                };
                if crate::wire::canon(&have) == want {
                    found = Some(f(device));
                    break;
                }
            }
        }
        if !pool.is_null() {
            let _: () = msg_send![pool, drain];
        }
        found
    }
}

#[cfg(target_os = "macos")]
fn load_framework() {
    use objc::runtime::Object;
    use objc::{class, msg_send, sel, sel_impl};
    use std::ffi::CString;
    use std::sync::Once;

    static ONCE: Once = Once::new();
    ONCE.call_once(|| unsafe {
        let path = CString::new("/System/Library/Frameworks/IOBluetooth.framework")
            .expect("framework path");
        let path: *mut Object = msg_send![class!(NSString), stringWithUTF8String: path.as_ptr()];
        let bundle: *mut Object = msg_send![class!(NSBundle), bundleWithPath: path];
        if !bundle.is_null() {
            let _: bool = msg_send![bundle, load];
        }
    });
}

#[cfg(test)]
mod tests {
    use super::kind_label;

    #[test]
    fn labels_common_devices() {
        assert_eq!(kind_label(0x05, 0x20), "Mouse");
        assert_eq!(kind_label(0x05, 0x10), "Keyboard");
        assert_eq!(kind_label(0x05, 0x30), "Keyboard and mouse");
        assert_eq!(kind_label(0x04, 0), "Audio");
        assert_eq!(kind_label(0x02, 0), "Phone");
        assert_eq!(kind_label(0x1f, 0), "Bluetooth");
    }
}
