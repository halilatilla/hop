mod bluetooth;
mod choice;
mod handoff;
mod link;
mod menu_bar;
mod wire;

use std::collections::HashSet;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

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
        #[cfg(not(target_os = "macos"))]
        open_window(cx, &hop);
    });
}

enum Phase {
    Idle,
    Sending { started: Instant },
}

struct Hop {
    window: Option<gpui::WindowHandle<Hop>>,
    focus: FocusHandle,
    devices: Vec<DeviceView>,
    list_error: Option<String>,
    choice: choice::Choice,
    peer: handoff::Peer,
    seen: link::Seen,
    allows: Vec<link::Nearby>,
    notice: Option<String>,
    phase: Phase,
    flight: Option<JoinHandle<handoff::Outcome>>,
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
            choice: choice::load(&choice::choice_path()).unwrap_or_else(|err| {
                eprintln!("hop: using an empty shared list ({err})");
                choice::Choice::default()
            }),
            peer: handoff::Peer::Missing,
            seen: link::Seen::None,
            allows: Vec::new(),
            notice: None,
            phase: Phase::Idle,
            flight: None,
            status_label: String::new(),
            ticks: 0,
        }
    }

    fn start(&mut self, cx: &mut Context<Self>) {
        link::start();
        self.refresh(cx);
        self.note_peer();
        self.publish_status();
        self.sync_menu();
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
        self.absorb_peer(cx);
        link::set_shared(self.choice.addresses.clone());
        link::set_paired(
            self.devices
                .iter()
                .map(|device| device.address.clone())
                .collect(),
        );
        self.drive_send();
        let seen = link::seen();
        let allows = link::pending();
        let became_ready =
            matches!(seen, link::Seen::Ready) && !matches!(self.seen, link::Seen::Ready);
        if became_ready
            || (matches!(seen, link::Seen::Ready)
                && !self.choice.quiet.is_empty()
                && self.ticks.is_multiple_of(40))
        {
            self.push_list();
        }
        if seen != self.seen || allows != self.allows {
            if matches!(self.phase, Phase::Idle) {
                self.notice = None;
            }
            self.seen = seen;
            self.allows = allows;
            self.peer = link::peer();
            self.publish_status();
            self.sync_menu();
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
                MenuCommand::SendAll => self.send_all(),
                MenuCommand::SendOne(address) => self.send_one(&address),
                MenuCommand::UseAll => self.use_here(),
                MenuCommand::UseOne(address) => self.use_one(&address),
                MenuCommand::Allow(id) => {
                    link::allow(&id);
                    self.note_peer();
                    self.publish_status();
                    self.sync_menu();
                    cx.notify();
                }
                MenuCommand::Quit => cx.quit(),
            }
        }
    }

    fn refresh(&mut self, cx: &mut Context<Self>) {
        let (devices, list_error) = match bluetooth::paired_devices() {
            Ok(list) => (views(list, &self.choice.addresses), None),
            Err(err) => (Vec::new(), Some(err)),
        };
        if devices != self.devices || list_error != self.list_error {
            self.devices = devices;
            self.list_error = list_error;
            if self.remember_names() {
                self.persist();
                if matches!(self.seen, link::Seen::Ready) {
                    self.push_list();
                }
            }
            self.publish_status();
            self.sync_menu();
            cx.notify();
        }
    }

    fn remember_names(&mut self) -> bool {
        let mut changed = false;
        for device in &self.devices {
            if !self.choice.addresses.contains(&device.address) || device.name.is_empty() {
                continue;
            }
            if self.choice.names.get(&device.address) != Some(&device.name) {
                self.choice
                    .names
                    .insert(device.address.clone(), device.name.clone());
                changed = true;
            }
        }
        changed
    }

    fn send_all(&mut self) {
        self.send_addresses(&self.connected_chosen());
    }

    fn send_one(&mut self, address: &str) {
        let address = wire::canon(address);
        let on_this_mac = self.devices.iter().any(|device| {
            device.address == address
                && self.choice.addresses.contains(&address)
                && device.connected
        });
        if !on_this_mac {
            return;
        }
        self.send_addresses(std::slice::from_ref(&address));
    }

    fn use_here(&mut self) {
        if self.choice.addresses.is_empty() {
            self.report(handoff::Outcome::Stayed(handoff::StayReason::NothingShared));
            return;
        }
        let away = self.away();
        if away.is_empty() {
            self.report(handoff::Outcome::Stayed(handoff::StayReason::AlreadyHere));
            return;
        }
        self.claim_addresses(&away);
    }

    fn use_one(&mut self, address: &str) {
        let address = wire::canon(address);
        if !self.choice.addresses.contains(&address) {
            return;
        }
        let here = self
            .devices
            .iter()
            .any(|device| device.address == address && device.connected);
        if here {
            return;
        }
        self.claim_addresses(std::slice::from_ref(&address));
    }

    fn away(&self) -> Vec<String> {
        let here: HashSet<String> = self
            .devices
            .iter()
            .filter(|device| device.connected)
            .map(|device| device.address.clone())
            .collect();
        let mut away: Vec<_> = self
            .choice
            .addresses
            .iter()
            .filter(|address| !here.contains(*address))
            .cloned()
            .collect();
        away.sort();
        away
    }

    fn send_addresses(&mut self, targets: &[String]) {
        if !matches!(self.phase, Phase::Idle) {
            return;
        }
        self.notice = None;
        if targets.is_empty() {
            self.report(handoff::Outcome::Stayed(handoff::StayReason::NothingHere));
            return;
        }
        let Some(target) = link::target() else {
            let reason = match self.seen {
                link::Seen::Nearby => handoff::StayReason::NeedsAllow,
                link::Seen::Crowd => handoff::StayReason::Crowd,
                _ => handoff::StayReason::PeerUnreachable,
            };
            self.report(handoff::Outcome::Stayed(reason));
            return;
        };
        let addresses = targets.to_vec();
        self.flight = Some(std::thread::spawn(move || {
            link::handover(target, addresses)
        }));
        self.phase = Phase::Sending {
            started: Instant::now(),
        };
    }

    fn claim_addresses(&mut self, targets: &[String]) {
        if !matches!(self.phase, Phase::Idle) {
            return;
        }
        self.notice = None;
        if targets.is_empty() {
            self.report(handoff::Outcome::Stayed(handoff::StayReason::NothingThere));
            return;
        }
        let Some(target) = link::target() else {
            let reason = match self.seen {
                link::Seen::Nearby => handoff::StayReason::NeedsAllow,
                link::Seen::Crowd => handoff::StayReason::Crowd,
                _ => handoff::StayReason::PeerUnreachable,
            };
            self.report(handoff::Outcome::Stayed(reason));
            return;
        };
        let addresses = targets.to_vec();
        self.flight = Some(std::thread::spawn(move || link::claim(target, addresses)));
        self.phase = Phase::Sending {
            started: Instant::now(),
        };
    }

    fn drive_send(&mut self) {
        let Phase::Sending { started } = self.phase else {
            return;
        };
        let timed_out = started.elapsed() > Duration::from_secs(90);
        let finished = self
            .flight
            .as_ref()
            .is_some_and(|flight| flight.is_finished());
        if !timed_out && !finished {
            return;
        }
        let outcome = self.flight.take().and_then(|flight| {
            if flight.is_finished() {
                flight.join().ok()
            } else {
                None
            }
        });
        self.phase = Phase::Idle;
        self.report(outcome.unwrap_or(handoff::Outcome::Stayed(
            handoff::StayReason::PeerUnreachable,
        )));
    }

    fn note_peer(&mut self) {
        self.seen = link::seen();
        self.allows = link::pending();
        self.peer = link::peer();
    }

    fn report(&mut self, outcome: handoff::Outcome) {
        let notice = match &outcome {
            handoff::Outcome::Stayed(reason) => handoff::stayed_notice(*reason).map(str::to_string),
            handoff::Outcome::Reconnected(_) => Some(handoff::reconnected_notice().to_string()),
            handoff::Outcome::Moved(_) => None,
        };
        let Some(notice) = notice else {
            return;
        };
        self.notice = Some(notice.clone());
        menu_bar::set_tooltip(&notice);
        if self.window.is_none() {
            menu_bar::notify_stayed(&notice);
        }
    }

    fn connected_chosen(&self) -> Vec<String> {
        self.devices
            .iter()
            .filter(|device| self.choice.addresses.contains(&device.address) && device.connected)
            .map(|device| device.address.clone())
            .collect()
    }

    fn chosen_count(&self) -> usize {
        self.choice.addresses.len()
    }

    fn publish_status(&mut self) {
        let label = handoff::status_label(self.chosen_count(), self.peer);
        menu_bar::set_tooltip(&self.status_tooltip());
        if self.status_label == label {
            return;
        }
        self.status_label = label.clone();
        menu_bar::set_title(&label);
    }

    fn status_tooltip(&self) -> String {
        if let Some(notice) = &self.notice {
            return notice.clone();
        }
        match self.seen {
            link::Seen::Ready => handoff::tooltip(handoff::Peer::Ready),
            link::Seen::Crowd => "More than one other Mac is running Hop.".to_string(),
            link::Seen::Nearby => {
                let name = self
                    .allows
                    .first()
                    .map(|allow| allow.name.as_str())
                    .unwrap_or("the other Mac");
                format!("Allow {name} in this menu. Then allow this Mac on that Mac.")
            }
            link::Seen::None => handoff::tooltip(handoff::Peer::Missing),
        }
    }

    fn absorb_peer(&mut self, cx: &mut Context<Self>) {
        let incoming = link::take_incoming();
        let acked = link::take_acked();
        if incoming.add.is_empty() && incoming.remove.is_empty() && acked.is_empty() {
            return;
        }
        choice::absorb(&mut self.choice, &incoming.add, &incoming.remove);
        for address in acked {
            self.choice.quiet.remove(&wire::canon(&address));
        }
        self.persist();
        for device in &mut self.devices {
            device.chosen = self.choice.addresses.contains(&device.address);
        }
        self.publish_status();
        self.sync_menu();
        cx.notify();
    }

    fn push_list(&self) {
        let mut add: Vec<_> = self.choice.addresses.iter().cloned().collect();
        add.sort();
        let names = add
            .iter()
            .map(|address| self.choice.names.get(address).cloned().unwrap_or_default())
            .collect();
        let mut remove: Vec<_> = self.choice.quiet.iter().cloned().collect();
        remove.sort();
        link::announce(add, names, remove);
    }

    fn share_now(&self, add: Vec<String>, remove: Vec<String>) {
        let names = add
            .iter()
            .map(|address| self.choice.names.get(address).cloned().unwrap_or_default())
            .collect();
        link::announce(add, names, remove);
    }

    fn persist(&self) {
        if let Err(err) = choice::save(&choice::choice_path(), &self.choice) {
            eprintln!("hop: could not save the shared list ({err})");
        }
    }

    fn sync_menu(&self) {
        let rows = self.menu_rows();
        let allows = self
            .allows
            .iter()
            .map(|allow| menu_bar::MenuAllow {
                id: allow.id.clone(),
                name: allow.name.clone(),
            })
            .collect();
        menu_bar::set_devices(rows, allows);
    }

    fn menu_rows(&self) -> Vec<menu_bar::MenuDevice> {
        let mut rows = Vec::new();
        let mut seen = HashSet::new();
        for device in &self.devices {
            if !self.choice.addresses.contains(&device.address) {
                continue;
            }
            seen.insert(device.address.clone());
            rows.push(menu_bar::MenuDevice {
                address: device.address.clone(),
                name: device.name.clone(),
                connected: device.connected,
            });
        }
        let mut extra: Vec<_> = self
            .choice
            .addresses
            .iter()
            .filter(|address| !seen.contains(*address))
            .cloned()
            .collect();
        extra.sort();
        for address in extra {
            let name = self
                .choice
                .names
                .get(&address)
                .cloned()
                .filter(|name| !name.is_empty())
                .unwrap_or_else(|| "Shared device".to_string());
            rows.push(menu_bar::MenuDevice {
                address,
                name,
                connected: false,
            });
        }
        rows
    }

    fn listed(&self) -> Vec<DeviceView> {
        let mut rows = self.devices.clone();
        let known: HashSet<_> = rows.iter().map(|device| device.address.clone()).collect();
        let mut extra: Vec<_> = self
            .choice
            .addresses
            .iter()
            .filter(|address| !known.contains(*address))
            .cloned()
            .collect();
        extra.sort();
        for address in extra {
            let name = self
                .choice
                .names
                .get(&address)
                .cloned()
                .filter(|name| !name.is_empty())
                .unwrap_or_else(|| "Shared device".to_string());
            rows.push(DeviceView {
                address,
                name,
                detail: "Shared · On the other Mac".to_string(),
                chosen: true,
                connected: false,
            });
        }
        rows
    }

    fn toggle(&mut self, address: &str, cx: &mut Context<Self>) {
        let address = wire::canon(address);
        if address.is_empty() {
            return;
        }
        if !self.choice.addresses.remove(&address) {
            self.choice.addresses.insert(address.clone());
            self.choice.quiet.remove(&address);
            if let Some(device) = self.devices.iter().find(|device| device.address == address) {
                self.choice
                    .names
                    .insert(address.clone(), device.name.clone());
            }
            self.share_now(vec![address.clone()], Vec::new());
        } else {
            self.choice.names.remove(&address);
            self.choice.quiet.insert(address.clone());
            self.share_now(Vec::new(), vec![address]);
        }
        for device in &mut self.devices {
            device.chosen = self.choice.addresses.contains(&device.address);
        }
        self.persist();
        self.publish_status();
        self.sync_menu();
        cx.notify();
    }
}

fn views(list: Vec<PairedDevice>, chosen: &HashSet<String>) -> Vec<DeviceView> {
    list.into_iter()
        .filter_map(|device| {
            let address = wire::canon(&device.address);
            if address.is_empty() {
                return None;
            }
            let connection = if device.connected {
                "Connected"
            } else {
                "Not connected"
            };
            Some(DeviceView {
                chosen: chosen.contains(&address),
                connected: device.connected,
                detail: format!("{} · {connection}", device.kind),
                address,
                name: device.name,
            })
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
        let footer = self
            .notice
            .clone()
            .unwrap_or_else(|| summary(chosen_count, peer, self.seen));
        let listed = self.listed();
        let list_error = self.list_error.clone();
        let mut list = div()
            .id("device-list")
            .flex()
            .flex_col()
            .flex_1()
            .min_h(px(0.0))
            .gap(px(8.0))
            .overflow_y_scroll();
        if let Some(error) = list_error {
            list = list.child(note(error));
        } else if listed.is_empty() {
            list = list.child(note(
                "No paired Bluetooth devices yet. Pair them in System Settings on both Macs.",
            ));
        } else {
            for device in &listed {
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
                    .child("Check a device to share it. Either Mac can take it."),
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
                    .id("use-here")
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
                    .child("Use here")
                    .on_click(cx.listener(|this, _: &ClickEvent, _, _cx| this.use_here())),
            )
            .child(
                div()
                    .id("send-devices")
                    .h(px(44.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(10.0))
                    .bg(rgb(CARD))
                    .text_color(rgb(INK))
                    .text_size(px(16.0))
                    .cursor_pointer()
                    .child("Send to the other Mac")
                    .on_click(cx.listener(|this, _: &ClickEvent, _, _cx| this.send_all())),
            )
            .child(
                div()
                    .text_size(px(13.0))
                    .text_color(rgb(MUTED))
                    .child(footer),
            )
    }
}

fn summary(chosen: usize, peer: handoff::Peer, seen: link::Seen) -> String {
    if seen == link::Seen::Nearby {
        return "The other Mac is nearby. Allow it in the menu, on both Macs, before Send.".into();
    }
    if seen == link::Seen::Crowd {
        return "More than one other Mac is running Hop.".into();
    }
    match (chosen, peer) {
        (0, handoff::Peer::Missing) => {
            "Nothing is shared. The other Mac is not running Hop.".into()
        }
        (_, handoff::Peer::Missing) => format!(
            "{chosen} shared. The other Mac is not running Hop, so the devices stay where they are."
        ),
        (0, handoff::Peer::Ready) => {
            "Nothing is shared. Check a device, then either Mac can take it.".into()
        }
        (count, handoff::Peer::Ready) => {
            format!(
                "{count} shared. Use here brings them to this Mac. Send moves the ones connected here."
            )
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
