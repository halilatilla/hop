//! Paired Bluetooth devices on this Mac.

#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

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

#[cfg(target_os = "macos")]
static PAIR_SEND: std::sync::Mutex<Option<std::sync::mpsc::Sender<bool>>> =
    std::sync::Mutex::new(None);
#[cfg(target_os = "macos")]
static PAIR_HOLD: std::sync::Mutex<usize> = std::sync::Mutex::new(0);
#[cfg(target_os = "macos")]
static DELEGATE_HOLD: std::sync::Mutex<usize> = std::sync::Mutex::new(0);

pub fn release_all(addresses: &[String]) -> bool {
    #[cfg(target_os = "macos")]
    {
        let addresses = addresses.to_vec();
        on_main({
            let addresses = addresses.clone();
            move || {
                for address in &addresses {
                    release_here(address);
                }
            }
        });
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        loop {
            let gone = on_main({
                let addresses = addresses.clone();
                move || addresses.iter().all(|address| !connected_here(address))
            });
            if gone || std::time::Instant::now() >= deadline {
                return gone;
            }
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = addresses;
        false
    }
}

pub fn connected_ones(addresses: &[String]) -> Vec<String> {
    #[cfg(target_os = "macos")]
    {
        let mut held = Vec::new();
        for address in addresses {
            let address = address.clone();
            if on_main({
                let address = address.clone();
                move || connected_here(&address)
            }) {
                held.push(address);
            }
        }
        held
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = addresses;
        Vec::new()
    }
}

pub fn connect_all(addresses: &[String]) -> bool {
    #[cfg(target_os = "macos")]
    {
        addresses.iter().all(|address| connect_one(address))
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
fn release_here(address: &str) {
    use objc::{msg_send, sel, sel_impl};
    let _ = with_device(address, |device| unsafe {
        let connected: bool = msg_send![device, isConnected];
        if !connected {
            return;
        }
        let remove = sel!(remove);
        let can_remove: bool = msg_send![device, respondsToSelector: remove];
        if can_remove {
            let _: () = msg_send![device, remove];
        } else {
            let _: i32 = msg_send![device, closeConnection];
        }
    });
}

#[cfg(target_os = "macos")]
fn paired_here(address: &str) -> bool {
    use objc::{msg_send, sel, sel_impl};
    with_device(address, |device| unsafe {
        let paired: bool = msg_send![device, isPaired];
        paired
    })
    .unwrap_or(false)
}

#[cfg(target_os = "macos")]
fn connect_one(address: &str) -> bool {
    let address_owned = address.to_string();
    let opened = on_main({
        let address = address_owned.clone();
        move || paired_here(&address) && connect_here(&address)
    });
    if opened {
        return true;
    }
    let still_paired = on_main({
        let address = address_owned.clone();
        move || paired_here(&address) || connected_here(&address)
    });
    if still_paired {
        return on_main(move || connected_here(&address_owned));
    }
    false
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
fn pair_here(address: &str) -> bool {
    use std::sync::mpsc;

    let (tx, rx) = mpsc::channel();
    {
        let Ok(mut slot) = PAIR_SEND.lock() else {
            return false;
        };
        *slot = Some(tx);
    }
    let started = on_main({
        let address = address.to_string();
        move || begin_pair(&address)
    });
    if !started {
        if let Ok(mut slot) = PAIR_SEND.lock() {
            *slot = None;
        }
        return false;
    }
    let paired = match rx.recv_timeout(std::time::Duration::from_secs(30)) {
        Ok(paired) => paired,
        Err(_) => {
            if let Ok(mut slot) = PAIR_SEND.lock() {
                *slot = None;
            }
            false
        }
    };
    let connected = on_main({
        let address = address.to_string();
        move || connect_here(&address)
    });
    end_pair();
    paired && connected
}

#[cfg(target_os = "macos")]
fn begin_pair(address: &str) -> bool {
    use objc::runtime::Object;
    use objc::{class, msg_send, sel, sel_impl};

    let Some(class) = pair_class() else {
        return false;
    };
    with_device(address, |device| unsafe {
        let pair: *mut Object = msg_send![class!(IOBluetoothDevicePair), pairWithDevice: device];
        if pair.is_null() {
            return false;
        }
        let delegate: *mut Object = msg_send![class, new];
        if delegate.is_null() {
            return false;
        }
        let _: () = msg_send![pair, retain];
        let _: () = msg_send![pair, setDelegate: delegate];
        let result: i32 = msg_send![pair, start];
        if result != 0 {
            let _: () = msg_send![pair, setDelegate: std::ptr::null::<Object>()];
            let _: () = msg_send![pair, release];
            let _: () = msg_send![delegate, release];
            return false;
        }
        if let Ok(mut hold) = PAIR_HOLD.lock() {
            *hold = pair as usize;
        }
        if let Ok(mut hold) = DELEGATE_HOLD.lock() {
            *hold = delegate as usize;
        }
        true
    })
    .unwrap_or(false)
}

#[cfg(target_os = "macos")]
fn end_pair() {
    use objc::runtime::Object;
    use objc::{msg_send, sel, sel_impl};

    on_main(|| unsafe {
        let pair = PAIR_HOLD.lock().map(|mut hold| {
            let pair = *hold;
            *hold = 0;
            pair
        });
        let delegate = DELEGATE_HOLD.lock().map(|mut hold| {
            let delegate = *hold;
            *hold = 0;
            delegate
        });
        if let Ok(pair) = pair {
            if pair != 0 {
                let pair = pair as *mut Object;
                let _: () = msg_send![pair, setDelegate: std::ptr::null::<Object>()];
                let _: () = msg_send![pair, release];
            }
        }
        if let Ok(delegate) = delegate {
            if delegate != 0 {
                let _: () = msg_send![delegate as *mut Object, release];
            }
        }
    });
}

#[cfg(target_os = "macos")]
fn pair_class() -> Option<*const objc::runtime::Class> {
    use objc::declare::ClassDecl;
    use objc::runtime::{Object, Sel};
    use objc::{class, sel, sel_impl};
    use std::sync::OnceLock;

    static CLASS: OnceLock<usize> = OnceLock::new();
    let bits = CLASS.get_or_init(|| {
        let Some(mut decl) = ClassDecl::new("HopDevicePair", class!(NSObject)) else {
            return 0;
        };
        unsafe {
            decl.add_method(
                sel!(devicePairingFinished:error:),
                pairing_finished as extern "C" fn(&Object, Sel, *mut Object, i32),
            );
            decl.add_method(
                sel!(devicePairingUserConfirmationRequest:numericValue:),
                pairing_confirm as extern "C" fn(&Object, Sel, *mut Object, u32),
            );
        }
        decl.register() as *const objc::runtime::Class as usize
    });
    let class = *bits as *const objc::runtime::Class;
    if class.is_null() { None } else { Some(class) }
}

#[cfg(target_os = "macos")]
extern "C" fn pairing_finished(
    _this: &objc::runtime::Object,
    _: objc::runtime::Sel,
    _sender: *mut objc::runtime::Object,
    error: i32,
) {
    if let Ok(mut slot) = PAIR_SEND.lock() {
        if let Some(tx) = slot.take() {
            let _ = tx.send(error == 0);
        }
    }
}

#[cfg(target_os = "macos")]
extern "C" fn pairing_confirm(
    _this: &objc::runtime::Object,
    _: objc::runtime::Sel,
    sender: *mut objc::runtime::Object,
    _numeric: u32,
) {
    use objc::{msg_send, sel, sel_impl};
    unsafe {
        let _: () = msg_send![sender, replyUserConfirmation: true];
    }
}

#[cfg(target_os = "macos")]
fn on_main<R: Send + Default + 'static>(work: impl FnOnce() -> R + Send + 'static) -> R {
    use block::ConcreteBlock;

    unsafe extern "C" {
        fn pthread_main_np() -> i32;
        static _dispatch_main_q: std::ffi::c_void;
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
            &raw const _dispatch_main_q as *mut std::ffi::c_void,
            &*block as *const block::Block<(), ()> as *const std::ffi::c_void,
        );
    }
    rx.recv().unwrap_or_else(|_| {
        eprintln!("hop: Bluetooth work on the main thread did not finish");
        R::default()
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
        let mut device: *mut Object = std::ptr::null_mut();
        if let Ok(text) = std::ffi::CString::new(want.clone()) {
            let name: *mut Object =
                msg_send![class!(NSString), stringWithUTF8String: text.as_ptr()];
            if !name.is_null() {
                device = msg_send![device_class, deviceWithAddressString: name];
            }
        }
        let list: *mut Object = if !device.is_null() {
            std::ptr::null_mut()
        } else {
            msg_send![device_class, pairedDevices]
        };
        if device.is_null() && !list.is_null() {
            let count: usize = msg_send![list, count];
            for index in 0..count {
                let candidate: *mut Object = msg_send![list, objectAtIndex: index];
                if candidate.is_null() {
                    continue;
                }
                let Some(have) = ns_string(msg_send![candidate, addressString]) else {
                    continue;
                };
                if crate::wire::canon(&have) == want {
                    device = candidate;
                    break;
                }
            }
        }
        let found = if device.is_null() {
            None
        } else {
            Some(f(device))
        };
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
