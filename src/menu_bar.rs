//! Menu bar presence on macOS. Closing the window leaves Hop running.

use std::ffi::CString;
use std::sync::Mutex;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MenuCommand {
    Open,
    SendAll,
    SendOne(String),
    UseAll,
    UseOne(String),
    Allow(String),
    Quit,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MenuDevice {
    pub address: String,
    pub name: String,
    pub connected: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MenuAllow {
    pub id: String,
    pub name: String,
}

const OPEN: isize = 1;
const SEND_ALL: isize = 2;
const QUIT: isize = 3;
const ABOUT: isize = 4;
const USE_ALL: isize = 5;
const DEVICE_TAG: isize = 100;
const ALLOW_TAG: isize = 1000;

static PENDING: Mutex<Vec<MenuCommand>> = Mutex::new(Vec::new());
static ALLOWS: Mutex<Vec<MenuAllow>> = Mutex::new(Vec::new());

pub fn poll() -> Vec<MenuCommand> {
    PENDING
        .lock()
        .map(|mut pending| std::mem::take(&mut *pending))
        .unwrap_or_default()
}

pub fn install() {
    #[cfg(target_os = "macos")]
    install_mac();
}

pub fn set_title(title: &str) {
    #[cfg(target_os = "macos")]
    set_title_mac(title);
    #[cfg(not(target_os = "macos"))]
    let _ = title;
}

pub fn set_tooltip(text: &str) {
    #[cfg(target_os = "macos")]
    set_tooltip_mac(text);
    #[cfg(not(target_os = "macos"))]
    let _ = text;
}

pub fn set_devices(devices: Vec<MenuDevice>, allows: Vec<MenuAllow>) {
    #[cfg(target_os = "macos")]
    set_devices_mac(devices, allows);
    #[cfg(not(target_os = "macos"))]
    {
        let _ = devices;
        let _ = allows;
    }
}

pub fn notify_stayed(body: &str) {
    #[cfg(target_os = "macos")]
    notify_stayed_mac(body);
    #[cfg(not(target_os = "macos"))]
    let _ = body;
}

#[cfg(target_os = "macos")]
fn install_mac() {
    use objc::declare::ClassDecl;
    use objc::runtime::{Object, Sel};
    use objc::{class, msg_send, sel, sel_impl};

    unsafe {
        let app: *mut Object = msg_send![class!(NSApplication), sharedApplication];
        // NSApplicationActivationPolicyAccessory: menu bar only, no Dock icon.
        let _: bool = msg_send![app, setActivationPolicy: 1isize];

        let target_class = menu_target_class();
        let target: *mut Object = msg_send![target_class, new];

        let menu = status_menu(target);
        let bar: *mut Object = msg_send![class!(NSStatusBar), systemStatusBar];
        let item: *mut Object = msg_send![bar, statusItemWithLength: -1.0f64];
        if item.is_null() {
            eprintln!("hop: could not create the menu bar item");
            return;
        }
        let _: *mut Object = msg_send![item, retain];
        let _: () = msg_send![item, setMenu: menu];
        let button: *mut Object = msg_send![item, button];
        if let Ok(mut slot) = STATUS.lock() {
            *slot = StatusSlot {
                button: button as usize,
                item: item as usize,
                menu: menu as usize,
                target: target as usize,
            };
        }
        steady_menu_font(button);
        install_images(app, button);
        install_app_menu(app, target);
    }

    fn menu_target_class() -> &'static objc::runtime::Class {
        use objc::runtime::Class;
        use std::sync::OnceLock;
        static CLASS: OnceLock<&'static Class> = OnceLock::new();
        CLASS.get_or_init(|| {
            let existing = Class::get("HopMenuTarget");
            if let Some(existing) = existing {
                return existing;
            }
            let mut decl =
                ClassDecl::new("HopMenuTarget", class!(NSObject)).expect("declare HopMenuTarget");
            unsafe {
                decl.add_method(
                    sel!(hopMenu:),
                    menu_action as extern "C" fn(&Object, Sel, *mut Object),
                );
            }
            decl.register()
        })
    }

    extern "C" fn menu_action(_this: &objc::runtime::Object, _: Sel, sender: *mut Object) {
        let tag: isize = unsafe { msg_send![sender, tag] };
        if tag == ABOUT {
            show_about();
            return;
        }
        let command = match tag {
            OPEN => MenuCommand::Open,
            SEND_ALL => MenuCommand::SendAll,
            USE_ALL => MenuCommand::UseAll,
            QUIT => MenuCommand::Quit,
            tag if (DEVICE_TAG..ALLOW_TAG).contains(&tag) => {
                let index = (tag - DEVICE_TAG) as usize;
                let Ok(rows) = ROWS.lock() else {
                    return;
                };
                let Some(device) = rows.get(index) else {
                    return;
                };
                if device.connected {
                    MenuCommand::SendOne(device.address.clone())
                } else {
                    MenuCommand::UseOne(device.address.clone())
                }
            }
            tag if tag >= ALLOW_TAG => {
                let index = (tag - ALLOW_TAG) as usize;
                let Ok(allows) = ALLOWS.lock() else {
                    return;
                };
                let Some(allow) = allows.get(index) else {
                    return;
                };
                MenuCommand::Allow(allow.id.clone())
            }
            _ => return,
        };
        if let Ok(mut pending) = PENDING.lock() {
            pending.push(command);
        }
    }

    fn status_menu(target: *mut Object) -> *mut Object {
        unsafe {
            let menu: *mut Object = msg_send![class!(NSMenu), new];
            add_item(menu, target, "About Hop", "", ABOUT);
            add_separator(menu);
            add_item(menu, target, "Devices…", ",", OPEN);
            add_item(menu, target, "Use here", "", USE_ALL);
            add_item(menu, target, "Send to the other Mac", "", SEND_ALL);
            add_separator(menu);
            add_item(menu, target, "Quit Hop", "q", QUIT);
            menu
        }
    }

    fn install_app_menu(app: *mut Object, target: *mut Object) {
        unsafe {
            let main: *mut Object = msg_send![class!(NSMenu), new];
            let app_item: *mut Object = msg_send![class!(NSMenuItem), new];
            let _: () = msg_send![main, addItem: app_item];
            let submenu: *mut Object = msg_send![class!(NSMenu), new];
            let _: () = msg_send![submenu, setTitle: ns_string("Hop")];
            add_item(submenu, target, "About Hop", "", ABOUT);
            add_separator(submenu);
            add_item(submenu, target, "Devices…", ",", OPEN);
            add_item(submenu, target, "Use here", "", USE_ALL);
            add_item(submenu, target, "Send to the other Mac", "", SEND_ALL);
            add_separator(submenu);
            add_item(submenu, target, "Quit Hop", "q", QUIT);
            let _: () = msg_send![app_item, setSubmenu: submenu];
            let _: () = msg_send![app, setMainMenu: main];
        }
    }
}

#[cfg(target_os = "macos")]
struct StatusSlot {
    button: usize,
    item: usize,
    menu: usize,
    target: usize,
}

#[cfg(target_os = "macos")]
static STATUS: Mutex<StatusSlot> = Mutex::new(StatusSlot {
    button: 0,
    item: 0,
    menu: 0,
    target: 0,
});

#[cfg(target_os = "macos")]
static ROWS: Mutex<Vec<MenuDevice>> = Mutex::new(Vec::new());

#[cfg(target_os = "macos")]
fn steady_menu_font(button: *mut objc::runtime::Object) {
    use objc::{class, msg_send, sel, sel_impl};

    if button.is_null() {
        return;
    }
    unsafe {
        let current: *mut objc::runtime::Object = msg_send![button, font];
        let size: f64 = if current.is_null() {
            0.0
        } else {
            msg_send![current, pointSize]
        };
        let font: *mut objc::runtime::Object =
            msg_send![class!(NSFont), monospacedDigitSystemFontOfSize: size weight: 0.0f64];
        if !font.is_null() {
            let _: () = msg_send![button, setFont: font];
        }
    }
}

#[cfg(target_os = "macos")]
fn set_title_mac(title: &str) {
    use objc::runtime::Object;
    use objc::{msg_send, sel, sel_impl};

    let Ok(slot) = STATUS.lock() else {
        return;
    };
    let button = slot.button as *mut Object;
    let _kept = slot.item;
    if button.is_null() {
        return;
    }
    unsafe {
        let _: () = msg_send![button, setTitle: ns_string(title)];
    }
}

#[cfg(target_os = "macos")]
fn set_tooltip_mac(text: &str) {
    use objc::runtime::Object;
    use objc::{msg_send, sel, sel_impl};

    let Ok(slot) = STATUS.lock() else {
        return;
    };
    let button = slot.button as *mut Object;
    if button.is_null() {
        return;
    }
    unsafe {
        let _: () = msg_send![button, setToolTip: ns_string(text)];
    }
}

#[cfg(target_os = "macos")]
fn add_separator(menu: *mut objc::runtime::Object) {
    use objc::runtime::Object;
    use objc::{class, msg_send, sel, sel_impl};

    unsafe {
        let separator: *mut Object = msg_send![class!(NSMenuItem), separatorItem];
        let _: () = msg_send![menu, addItem: separator];
    }
}

#[cfg(target_os = "macos")]
fn add_item(
    menu: *mut objc::runtime::Object,
    target: *mut objc::runtime::Object,
    title: &str,
    key: &str,
    tag: isize,
) -> *mut objc::runtime::Object {
    use objc::runtime::Object;
    use objc::{class, msg_send, sel, sel_impl};

    unsafe {
        let item: *mut Object = msg_send![class!(NSMenuItem), alloc];
        let item: *mut Object = msg_send![item, initWithTitle: ns_string(title) action: sel!(hopMenu:) keyEquivalent: ns_string(key)];
        let _: () = msg_send![item, setTarget: target];
        let _: () = msg_send![item, setTag: tag];
        if !key.is_empty() {
            // NSEventModifierFlagCommand
            let _: () = msg_send![item, setKeyEquivalentModifierMask: 1usize << 20];
        }
        let _: () = msg_send![menu, addItem: item];
        item
    }
}

#[cfg(target_os = "macos")]
fn set_devices_mac(devices: Vec<MenuDevice>, allows: Vec<MenuAllow>) {
    use objc::runtime::Object;
    use objc::{msg_send, sel, sel_impl};

    if let Ok(mut rows) = ROWS.lock() {
        *rows = devices.clone();
    }
    if let Ok(mut saved) = ALLOWS.lock() {
        *saved = allows.clone();
    }
    let Ok(slot) = STATUS.lock() else {
        return;
    };
    let menu = slot.menu as *mut Object;
    let target = slot.target as *mut Object;
    if menu.is_null() || target.is_null() {
        return;
    }
    let any_connected = devices.iter().any(|device| device.connected);
    let any_away = devices.iter().any(|device| !device.connected);
    unsafe {
        let _: () = msg_send![menu, removeAllItems];
        add_item(menu, target, "About Hop", "", ABOUT);
        add_separator(menu);
        for (index, allow) in allows.iter().enumerate() {
            add_item(
                menu,
                target,
                &format!("Allow {}", allow.name),
                "",
                ALLOW_TAG + index as isize,
            );
        }
        if !allows.is_empty() {
            add_separator(menu);
        }
        add_item(menu, target, "Devices…", ",", OPEN);
        add_separator(menu);
        for (index, device) in devices.iter().enumerate() {
            let item = add_item(menu, target, &device.name, "", DEVICE_TAG + index as isize);
            let state: isize = if device.connected { 1 } else { 0 };
            let _: () = msg_send![item, setState: state];
        }
        if !devices.is_empty() {
            add_separator(menu);
        }
        let use_here = add_item(menu, target, "Use here", "", USE_ALL);
        let _: () = msg_send![use_here, setEnabled: any_away];
        let send = add_item(menu, target, "Send to the other Mac", "", SEND_ALL);
        let _: () = msg_send![send, setEnabled: any_connected];
        add_separator(menu);
        add_item(menu, target, "Quit Hop", "q", QUIT);
    }
}

#[cfg(target_os = "macos")]
fn notify_stayed_mac(body: &str) {
    use objc::runtime::Object;
    use objc::{class, msg_send, sel, sel_impl};

    unsafe {
        let note: *mut Object = msg_send![class!(NSUserNotification), alloc];
        let note: *mut Object = msg_send![note, init];
        if note.is_null() {
            return;
        }
        let _: () = msg_send![note, setTitle: ns_string("Hop")];
        let _: () = msg_send![note, setInformativeText: ns_string(body)];
        let _: () = msg_send![note, setIdentifier: ns_string("hop-send-stayed")];
        let center: *mut Object = msg_send![
            class!(NSUserNotificationCenter),
            defaultUserNotificationCenter
        ];
        if center.is_null() {
            return;
        }
        let _: () = msg_send![center, deliverNotification: note];
    }
}

#[cfg(target_os = "macos")]
#[repr(C)]
struct NsSize {
    width: f64,
    height: f64,
}

#[cfg(target_os = "macos")]
fn install_images(app: *mut objc::runtime::Object, button: *mut objc::runtime::Object) {
    use objc::runtime::YES;
    use objc::{msg_send, sel, sel_impl};

    unsafe {
        let mark = ns_image(include_bytes!("../assets/MenuBarTemplate.png"));
        if !button.is_null() && !mark.is_null() {
            let _: () = msg_send![mark, setTemplate: YES];
            let _: () = msg_send![mark, setSize: NsSize { width: 18.0, height: 18.0 }];
            let _: () = msg_send![button, setImage: mark];
            // NSImageLeft: the title stays beside the mark.
            let _: () = msg_send![button, setImagePosition: 2isize];
        }
        let icon = ns_image(include_bytes!("../assets/AppIcon.png"));
        if !app.is_null() && !icon.is_null() {
            let _: () = msg_send![app, setApplicationIconImage: icon];
        }
    }
}

#[cfg(target_os = "macos")]
fn show_about() {
    use objc::runtime::{Object, YES};
    use objc::{class, msg_send, sel, sel_impl};

    unsafe {
        let app: *mut Object = msg_send![class!(NSApplication), sharedApplication];
        let _: () = msg_send![app, activateIgnoringOtherApps: YES];
        let _: *mut Object =
            msg_send![app, orderFrontStandardAboutPanel: std::ptr::null::<Object>()];
    }
}

#[cfg(target_os = "macos")]
fn ns_image(bytes: &[u8]) -> *mut objc::runtime::Object {
    use objc::runtime::Object;
    use objc::{class, msg_send, sel, sel_impl};

    unsafe {
        let data: *mut Object =
            msg_send![class!(NSData), dataWithBytes: bytes.as_ptr() length: bytes.len()];
        if data.is_null() {
            return std::ptr::null_mut();
        }
        let image: *mut Object = msg_send![class!(NSImage), alloc];
        msg_send![image, initWithData: data]
    }
}

#[cfg(target_os = "macos")]
fn ns_string(text: &str) -> *mut objc::runtime::Object {
    use objc::{class, msg_send, sel, sel_impl};

    let c = CString::new(text).unwrap_or_else(|_| CString::new("Hop").expect("fallback"));
    unsafe { msg_send![class!(NSString), stringWithUTF8String: c.as_ptr()] }
}
