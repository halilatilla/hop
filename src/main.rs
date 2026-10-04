mod bluetooth;
mod choice;
mod handoff;
mod menu_bar;

use std::collections::HashSet;
use std::time::Duration;

use gpui::{
    App, ClickEvent, Context, Entity, FocusHandle, FontWeight, Global, IntoElement, Render,
    TitlebarOptions, Window, WindowBounds, WindowOptions, div, point, prelude::*, px, rgb, size,
};
use gpui_platform::application;

use bluetooth::PairedDevice;
use menu_bar::MenuCommand;

const INK: u32 = 0x171512;
const PAPER: u32 = 0xf3eee6;
const CARD: u32 = 0xfffbf6;
const MUTED: u32 = 0xa89b8c;
const MUTED_ON_INK: u32 = 0xc4b8aa;
const AMBER: u32 = 0xe39a4b;
const AMBER_INK: u32 = 0x1a140e;

struct HopKeepAlive(#[allow(dead_code)] Entity<Hop>);

impl Global for HopKeepAlive {}

fn main() {
    application().run(|cx: &mut App| {
        menu_bar::install();
        let hop = cx.new(|cx| Hop::new(cx));
        cx.set_global(HopKeepAlive(hop.clone()));
        hop.update(cx, |hop, cx| hop.start(cx));
        // The menu bar is the app. Other platforms have no status item.
        #[cfg(not(target_os = "macos"))]
        open_window(cx, &hop);
    });
}

struct Hop {
    window: Option<gpui::WindowHandle<Hop>>,
    focus: FocusHandle,
    devices: Vec<DeviceView>,
    list_error: Option<String>,
    chosen: HashSet<String>,
    peer: handoff::Peer,
    status_label: String,
    ticks: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct DeviceView {
    address: String,
    name: String,
    detail: String,
    chosen: bool,
    connected: bool,
}

impl Hop {
    fn new(cx: &mut Context<Self>) -> Self {
        Self {
            window: None,
            focus: cx.focus_handle(),
            devices: Vec::new(),
            list_error: None,
            chosen: choice::load(&choice::choice_path()).unwrap_or_else(|err| {
                eprintln!("hop: using no chosen devices ({err})");
                HashSet::new()
            }),
            peer: handoff::observe_peer(),
            status_label: String::new(),
            ticks: 0,
        }
    }

    fn start(&mut self, cx: &mut Context<Self>) {
        self.refresh(cx);
        self.publish_status();
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(250))
                    .await;
                if this.update(cx, |hop, cx| hop.on_tick(cx)).is_err() {
                    break;
                }
            }
        })
        .detach();
    }

    fn on_tick(&mut self, cx: &mut Context<Self>) {
        self.ticks = self.ticks.wrapping_add(1);
        let peer = handoff::observe_peer();
        if peer != self.peer {
            self.peer = peer;
            self.publish_status();
            cx.notify();
        }
        if self.ticks.is_multiple_of(8) {
            self.refresh(cx);
        }
        let hop = cx.entity();
        for command in menu_bar::poll() {
            match command {
                MenuCommand::Open => {
                    let hop = hop.clone();
                    cx.defer(move |cx| open_window(cx, &hop));
                }
                MenuCommand::Send => self.send(),
                MenuCommand::Quit => cx.quit(),
            }
        }
    }

    fn refresh(&mut self, cx: &mut Context<Self>) {
        let (devices, list_error) = match bluetooth::paired_devices() {
            Ok(list) => (views(list, &self.chosen), None),
            Err(err) => (Vec::new(), Some(err)),
        };
        if devices != self.devices || list_error != self.list_error {
            self.devices = devices;
            self.list_error = list_error;
            self.publish_status();
            cx.notify();
        }
    }

    fn send(&mut self) {
        let connected = self.connected_chosen();
        if let handoff::Effect::HandOff(addresses) = handoff::plan(self.peer, &connected) {
            // Plan allowed the move. Delivery still disconnects nothing.
            let _left_connected = handoff::deliver(&addresses);
        }
    }

    fn connected_chosen(&self) -> Vec<String> {
        self.devices
            .iter()
            .filter(|device| device.chosen && device.connected)
            .map(|device| device.address.clone())
            .collect()
    }

    fn chosen_count(&self) -> usize {
        self.devices.iter().filter(|device| device.chosen).count()
    }

    fn publish_status(&mut self) {
        let label = handoff::status_label(self.chosen_count(), self.peer);
        if self.status_label == label {
            return;
        }
        self.status_label = label.clone();
        menu_bar::set_title(&label);
    }

    fn toggle(&mut self, address: &str, cx: &mut Context<Self>) {
        if !self.chosen.remove(address) {
            self.chosen.insert(address.to_string());
        }
        let chosen = self.chosen.contains(address);
        for device in &mut self.devices {
            if device.address == address {
                device.chosen = chosen;
            }
        }
        if let Err(err) = choice::save(&choice::choice_path(), &self.chosen) {
            eprintln!("hop: could not save the chosen devices ({err})");
        }
        self.publish_status();
        cx.notify();
    }
}

fn views(list: Vec<PairedDevice>, chosen: &HashSet<String>) -> Vec<DeviceView> {
    list.into_iter()
        .map(|device| {
            let connection = if device.connected {
                "Connected"
            } else {
                "Not connected"
            };
            DeviceView {
                chosen: chosen.contains(&device.address),
                connected: device.connected,
                detail: format!("{} · {connection}", device.kind),
                address: device.address,
                name: device.name,
            }
        })
        .collect()
}

fn open_window(cx: &mut App, hop: &Entity<Hop>) {
    if let Some(existing) = hop.read(cx).window {
        existing
            .update(cx, |_, window, cx| {
                window.activate_window();
                cx.activate(true);
            })
            .ok();
        return;
    }
    let options = window_options(cx);
    let focus = hop.read(cx).focus.clone();
    let owner = hop.clone();
    let opened = cx.open_window(options, move |window, cx| {
        let weak = owner.downgrade();
        window.on_window_should_close(cx, move |_, cx| {
            weak.update(cx, |hop, _| hop.window = None).ok();
            true
        });
        window.focus(&focus, cx);
        owner
    });
    match opened {
        Ok(handle) => {
            hop.update(cx, |hop, _| hop.window = Some(handle));
            cx.activate(true);
        }
        Err(err) => eprintln!("hop: could not open the window ({err:#})"),
    }
}

fn window_options(cx: &App) -> WindowOptions {
    WindowOptions {
        window_bounds: Some(WindowBounds::centered(size(px(440.0), px(640.0)), cx)),
        titlebar: Some(TitlebarOptions {
            title: Some("Hop".into()),
            appears_transparent: true,
            traffic_light_position: Some(point(px(20.0), px(18.0))),
        }),
        focus: true,
        is_resizable: false,
        app_id: Some("hop".into()),
        ..Default::default()
    }
}

impl Render for Hop {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let chosen_count = self.chosen_count();
        let peer = self.peer;
        let mut list = div()
            .id("device-list")
            .flex()
            .flex_col()
            .flex_1()
            .min_h(px(0.0))
            .gap(px(8.0))
            .overflow_y_scroll();
        if let Some(error) = &self.list_error {
            list = list.child(note(error.clone()));
        } else if self.devices.is_empty() {
            list = list.child(note(
                "No paired Bluetooth devices yet. Pair them in System Settings on both Macs.",
            ));
        } else {
            for device in &self.devices {
                let address = device.address.clone();
                list = list.child(device_row(
                    device,
                    cx.listener(move |this, _: &ClickEvent, _, cx| this.toggle(&address, cx)),
                ));
            }
        }

        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(rgb(PAPER))
            .text_color(rgb(INK))
            .pt(px(56.0))
            .px(px(24.0))
            .pb(px(20.0))
            .gap(px(14.0))
            .child(div().text_size(px(28.0)).child("Hop"))
            .child(
                div()
                    .text_size(px(15.0))
                    .child("Choose which Bluetooth devices to send."),
            )
            .child(
                div()
                    .text_size(px(13.0))
                    .text_color(rgb(MUTED))
                    .child("A mouse, a keyboard, headphones, or anything else already paired. Both Macs use the same Apple Account, the same network, and those same devices."),
            )
            .child(list)
            .child(
                div()
                    .id("send-devices")
                    .h(px(44.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(10.0))
                    .bg(rgb(INK))
                    .text_color(rgb(PAPER))
                    .font_weight(FontWeight::BOLD)
                    .text_size(px(16.0))
                    .cursor_pointer()
                    .child("Send to the other Mac")
                    .on_click(cx.listener(|this, _: &ClickEvent, _, _cx| this.send())),
            )
            .child(
                div()
                    .text_size(px(13.0))
                    .text_color(rgb(MUTED))
                    .child(summary(chosen_count, peer)),
            )
    }
}

fn summary(chosen: usize, peer: handoff::Peer) -> String {
    match (chosen, peer) {
        (0, handoff::Peer::Missing) => "Nothing chosen. The other Mac is not running Hop, so Send leaves everything connected here.".into(),
        (_, handoff::Peer::Missing) => format!(
            "{chosen} chosen. The other Mac is not running Hop, so Send leaves them connected here."
        ),
        (0, handoff::Peer::Ready) => "Nothing chosen.".into(),
        (count, handoff::Peer::Ready) => {
            format!("{count} chosen. Send moves the ones that are connected.")
        }
    }
}

fn note(text: impl Into<String>) -> impl IntoElement {
    div()
        .text_size(px(14.0))
        .text_color(rgb(MUTED))
        .child(text.into())
}

fn device_row(
    device: &DeviceView,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut gpui::App) + 'static,
) -> impl IntoElement {
    let (name_color, detail_color, row_bg) = if device.chosen {
        (rgb(PAPER), rgb(MUTED_ON_INK), rgb(INK))
    } else {
        (rgb(INK), rgb(MUTED), rgb(CARD))
    };
    let mark_bg = if device.chosen {
        rgb(AMBER)
    } else {
        rgb(0xe7e0d6)
    };
    let mark_ink = if device.chosen {
        rgb(AMBER_INK)
    } else {
        rgb(MUTED)
    };
    div()
        .id(format!("device-{}", device.address))
        .flex()
        .flex_row()
        .items_center()
        .gap(px(12.0))
        .px(px(12.0))
        .py(px(10.0))
        .rounded(px(10.0))
        .bg(row_bg)
        .cursor_pointer()
        .on_click(on_click)
        .child(
            div()
                .flex()
                .items_center()
                .justify_center()
                .w(px(22.0))
                .h(px(22.0))
                .rounded(px(6.0))
                .bg(mark_bg)
                .text_color(mark_ink)
                .text_size(px(13.0))
                .font_weight(FontWeight::BOLD)
                .child(if device.chosen { "✓" } else { "" }),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(2.0))
                .child(
                    div()
                        .text_size(px(15.0))
                        .text_color(name_color)
                        .child(device.name.clone()),
                )
                .child(
                    div()
                        .text_size(px(13.0))
                        .text_color(detail_color)
                        .child(device.detail.clone()),
                ),
        )
}
