//! Menu bar presence on macOS. Closing the window leaves Hop running.

use std::ffi::CString;
use std::sync::Mutex;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuCommand {
    Open,
    Send,
    Quit,
}

static PENDING: Mutex<Vec<MenuCommand>> = Mutex::new(Vec::new());

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
            };
        }
        steady_menu_font(button);
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
        let command = match tag {
            1 => MenuCommand::Open,
            2 => MenuCommand::Send,
            3 => MenuCommand::Quit,
            _ => return,
        };
        if let Ok(mut pending) = PENDING.lock() {
            pending.push(command);
        }
    }

    fn status_menu(target: *mut Object) -> *mut Object {
        unsafe {
            let menu: *mut Object = msg_send![class!(NSMenu), new];
            add_item(menu, target, "Devices…", ",", 1);
            add_item(menu, target, "Send to the other Mac", "", 2);
            add_separator(menu);
            add_item(menu, target, "Quit Hop", "q", 3);
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
            add_item(submenu, target, "Devices…", ",", 1);
            add_item(submenu, target, "Send to the other Mac", "", 2);
            add_separator(submenu);
            add_item(submenu, target, "Quit Hop", "q", 3);
            let _: () = msg_send![app_item, setSubmenu: submenu];
            let _: () = msg_send![app, setMainMenu: main];
        }
    }

    fn add_separator(menu: *mut Object) {
        unsafe {
            let separator: *mut Object = msg_send![class!(NSMenuItem), separatorItem];
            let _: () = msg_send![menu, addItem: separator];
        }
    }

    fn add_item(menu: *mut Object, target: *mut Object, title: &str, key: &str, tag: isize) {
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
        }
    }
}

#[cfg(target_os = "macos")]
struct StatusSlot {
    button: usize,
    item: usize,
}

#[cfg(target_os = "macos")]
static STATUS: Mutex<StatusSlot> = Mutex::new(StatusSlot { button: 0, item: 0 });

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
fn ns_string(text: &str) -> *mut objc::runtime::Object {
    use objc::{class, msg_send, sel, sel_impl};

    let c = CString::new(text).unwrap_or_else(|_| CString::new("Hop").expect("fallback"));
    unsafe { msg_send![class!(NSString), stringWithUTF8String: c.as_ptr()] }
}
