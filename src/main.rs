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
    Subscription, TitlebarOptions, Window, WindowAppearance, WindowBounds, WindowOptions, div,
    point, prelude::*, px, rgb, size,
};
use gpui_platform::application;

use bluetooth::PairedDevice;
use menu_bar::MenuCommand;

#[derive(Clone, Copy)]
struct Palette {
    text: u32,
    paper: u32,
    card: u32,
    muted: u32,
    line: u32,
    stroke: u32,
    good: u32,
    good_wash: u32,
    wash: u32,
    wash_ink: u32,
    button_text: u32,
}

impl Palette {
    fn light() -> Self {
        Self {
            text: 0x171512,
            paper: 0xf3eee6,
            card: 0xfffbf6,
            muted: 0xa89b8c,
            line: 0xe4ddd4,
            stroke: 0xc4b8aa,
            good: 0x2f6f4e,
            good_wash: 0xe3f0e8,
            wash: 0xf6e6d4,
            wash_ink: 0x6b4a24,
            button_text: 0xf3eee6,
        }
    }

    fn dark() -> Self {
        Self {
            text: 0xf4efe8,
            paper: 0x1c1916,
            card: 0x28241f,
            muted: 0xb7aa9c,
            line: 0x3c362f,
            stroke: 0x6d645c,
            good: 0x8fbf9a,
            good_wash: 0x1e3328,
            wash: 0x3a2f22,
            wash_ink: 0xf0d3b0,
            button_text: 0x171512,
        }
    }
}

fn palette(window: &Window) -> Palette {
    match window.appearance() {
        WindowAppearance::Dark | WindowAppearance::VibrantDark => Palette::dark(),
        WindowAppearance::Light | WindowAppearance::VibrantLight => Palette::light(),
    }
}

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
    Sending {
        started: Instant,
        addresses: Vec<String>,
    },
}

enum RowAction {
    Connect,
    Connected,
    Share,
    Connecting,
    Wait,
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
    theme: Option<Subscription>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct DeviceView {
    address: String,
    name: String,
    kind: String,
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
            theme: None,
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
        self.drive_send(cx);
        let seen = link::seen();
        let allows = link::pending();
        let became_ready =
            matches!(seen, link::Seen::Ready) && !matches!(self.seen, link::Seen::Ready);
        if matches!(seen, link::Seen::Ready) && (became_ready || self.ticks.is_multiple_of(32)) {
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
                MenuCommand::UseOne(address) => self.use_one(&address, cx),
                MenuCommand::Remove(address) => self.unshare(&address, cx),
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

    fn use_one(&mut self, address: &str, cx: &mut Context<Self>) {
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
        self.claim_addresses(std::slice::from_ref(&address), cx);
    }

    fn claim_addresses(&mut self, targets: &[String], cx: &mut Context<Self>) {
        if !matches!(self.phase, Phase::Idle) {
            return;
        }
        self.notice = None;
        if targets.is_empty() {
            self.report(handoff::Outcome::Stayed(handoff::StayReason::NothingThere));
            cx.notify();
            return;
        }
        let Some(target) = link::target() else {
            let reason = match self.seen {
                link::Seen::Nearby => handoff::StayReason::NeedsAllow,
                link::Seen::Crowd => handoff::StayReason::Crowd,
                _ => handoff::StayReason::PeerUnreachable,
            };
            self.report(handoff::Outcome::Stayed(reason));
            cx.notify();
            return;
        };
        let addresses = targets.to_vec();
        let moving = addresses.clone();
        self.flight = Some(std::thread::spawn(move || link::claim(target, addresses)));
        self.phase = Phase::Sending {
            started: Instant::now(),
            addresses: moving,
        };
        self.publish_status();
        self.sync_menu();
        cx.notify();
    }

    fn moving(&self) -> &[String] {
        match &self.phase {
            Phase::Sending { addresses, .. } => addresses,
            Phase::Idle => &[],
        }
    }

    fn drive_send(&mut self, cx: &mut Context<Self>) {
        let started = match &self.phase {
            Phase::Sending { started, .. } => *started,
            Phase::Idle => return,
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
        self.publish_status();
        self.sync_menu();
        self.report(outcome.unwrap_or(handoff::Outcome::Stayed(
            handoff::StayReason::PeerUnreachable,
        )));
        cx.notify();
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

    fn chosen_count(&self) -> usize {
        self.choice.addresses.len()
    }

    fn publish_status(&mut self) {
        let label = if matches!(self.phase, Phase::Sending { .. }) {
            "…".to_string()
        } else {
            handoff::status_label(self.chosen_count(), self.peer)
        };
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
        menu_bar::set_devices(rows, allows, &self.menu_peer_line());
    }

    fn menu_peer_line(&self) -> String {
        match self.seen {
            link::Seen::Ready => link::other_mac(),
            link::Seen::Crowd => "More than one Mac".to_string(),
            link::Seen::Nearby | link::Seen::None => String::new(),
        }
    }

    fn menu_rows(&self) -> Vec<menu_bar::MenuDevice> {
        let moving: Vec<String> = self.moving().to_vec();
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
                busy: moving.iter().any(|address| address == &device.address),
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
                address: address.clone(),
                name,
                connected: false,
                busy: moving.iter().any(|moving| moving == &address),
            });
        }
        rows
    }

    fn shared_views(&self) -> Vec<DeviceView> {
        let mut addresses: Vec<_> = self.choice.addresses.iter().cloned().collect();
        addresses.sort();
        addresses
            .into_iter()
            .map(|address| {
                let local = self.devices.iter().find(|device| device.address == address);
                let connected = local.is_some_and(|device| device.connected);
                let kind = local.map(|device| device.kind.clone()).unwrap_or_default();
                let name = local
                    .map(|device| device.name.clone())
                    .filter(|name| !name.is_empty())
                    .or_else(|| self.choice.names.get(&address).cloned())
                    .filter(|name| !name.is_empty())
                    .unwrap_or_else(|| "Shared device".to_string());
                let place = if connected {
                    link::this_mac()
                } else {
                    link::other_mac()
                };
                DeviceView {
                    address,
                    name,
                    detail: place_line(&kind, &place),
                    kind,
                    chosen: true,
                    connected,
                }
            })
            .collect()
    }

    fn unshared_here(&self) -> Vec<DeviceView> {
        self.devices
            .iter()
            .filter(|device| device.connected && !self.choice.addresses.contains(&device.address))
            .cloned()
            .map(|mut device| {
                device.chosen = false;
                device.detail = place_line(&device.kind, &link::this_mac());
                device
            })
            .collect()
    }

    fn on_device(&mut self, address: &str, cx: &mut Context<Self>) {
        let address = wire::canon(address);
        if address.is_empty() {
            return;
        }
        let here = self
            .devices
            .iter()
            .any(|device| device.address == address && device.connected);
        let shared = self.choice.addresses.contains(&address);
        if shared && !here {
            self.use_one(&address, cx);
            return;
        }
        if here && !shared {
            self.toggle(&address, cx);
        }
    }

    fn unshare(&mut self, address: &str, cx: &mut Context<Self>) {
        let address = wire::canon(address);
        let here = self
            .devices
            .iter()
            .any(|device| device.address == address && device.connected);
        if here && self.choice.addresses.contains(&address) {
            self.toggle(&address, cx);
        }
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

    fn allow_one(&mut self, id: &str, cx: &mut Context<Self>) {
        link::allow(id);
        self.note_peer();
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
            Some(DeviceView {
                chosen: chosen.contains(&address),
                connected: device.connected,
                kind: device.kind.to_string(),
                detail: String::new(),
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
            weak.update(cx, |hop, _| {
                hop.window = None;
                hop.theme = None;
            })
            .ok();
            true
        });
        let watch = owner.clone();
        let theme = window.observe_window_appearance(move |_, cx| {
            watch.update(cx, |_, cx| cx.notify());
        });
        owner.update(cx, |hop, _| hop.theme = Some(theme));
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
        window_bounds: Some(WindowBounds::centered(size(px(400.0), px(520.0)), cx)),
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
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = palette(window);
        let moving: HashSet<String> = self.moving().iter().cloned().collect();
        let any_moving = !moving.is_empty();
        let shared = self.shared_views();
        let mine = self.unshared_here();
        let list_error = self.list_error.clone();
        let notice = self.notice.clone();
        let seen = self.seen;
        let allows = self.allows.clone();
        let mut list = div()
            .id("device-list")
            .flex()
            .flex_col()
            .flex_1()
            .min_h(px(0.0))
            .gap(px(8.0))
            .overflow_y_scroll();
        if let Some(error) = list_error {
            list = list.child(note(error, colors));
        } else {
            list = list.child(section_label("Shared", colors));
            if shared.is_empty() {
                list = list.child(note(
                    "Nothing shared yet. Share a device from this Mac.",
                    colors,
                ));
            } else {
                for device in &shared {
                    let action = if moving.contains(&device.address) {
                        RowAction::Connecting
                    } else if device.connected {
                        RowAction::Connected
                    } else if any_moving {
                        RowAction::Wait
                    } else {
                        RowAction::Connect
                    };
                    list = list.child(device_row(device, action, colors, cx));
                }
            }
            if !mine.is_empty() {
                list = list.child(section_label("On this Mac", colors));
                for device in &mine {
                    let action = if any_moving {
                        RowAction::Wait
                    } else {
                        RowAction::Share
                    };
                    list = list.child(device_row(device, action, colors, cx));
                }
            }
        }

        let mut column = div()
            .flex()
            .flex_col()
            .size_full()
            .bg(rgb(colors.paper))
            .text_color(rgb(colors.text))
            .pt(px(52.0))
            .px(px(20.0))
            .pb(px(16.0))
            .gap(px(16.0));
        if seen == link::Seen::Nearby && !allows.is_empty() {
            for allow in &allows {
                let id = allow.id.clone();
                column = column.child(allow_card(&allow.id, &allow.name, colors, {
                    cx.listener(move |this, _: &ClickEvent, _, cx| this.allow_one(&id, cx))
                }));
            }
        } else {
            column = column.child(peer_status(seen, colors));
        }
        column = column.child(list);
        if let Some(notice) = notice {
            column = column.child(
                div()
                    .px(px(12.0))
                    .py(px(8.0))
                    .rounded(px(8.0))
                    .bg(rgb(colors.wash))
                    .text_size(px(13.0))
                    .text_color(rgb(colors.wash_ink))
                    .child(notice),
            );
        }
        column
    }
}

fn device_row(
    device: &DeviceView,
    action: RowAction,
    colors: Palette,
    cx: &mut Context<Hop>,
) -> impl IntoElement {
    let on_click = {
        let address = device.address.clone();
        cx.listener(move |this, _: &ClickEvent, _, cx| this.on_device(&address, cx))
    };
    let on_remove = {
        let address = device.address.clone();
        cx.listener(move |this, _: &ClickEvent, _, cx| this.unshare(&address, cx))
    };
    let here = device.connected;
    let mut row = div()
        .id(format!("device-{}", device.address))
        .flex()
        .flex_row()
        .items_center()
        .gap(px(10.0))
        .px(px(12.0))
        .py(px(10.0))
        .rounded(px(10.0))
        .bg(rgb(colors.card))
        .child(presence_mark(here, colors.card, colors))
        .child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_w(px(0.0))
                .gap(px(2.0))
                .child(
                    div()
                        .text_size(px(15.0))
                        .text_color(rgb(colors.text))
                        .child(device.name.clone()),
                )
                .child(
                    div()
                        .text_size(px(13.0))
                        .text_color(rgb(colors.muted))
                        .child(device.detail.clone()),
                ),
        );
    row = match action {
        RowAction::Connecting => row.child(status_chip("Connecting…", false, colors)),
        RowAction::Connected => {
            row.child(status_chip("Connected", true, colors))
                .child(outline_button(
                    format!("remove-{}", device.address),
                    "Remove",
                    colors,
                    on_remove,
                ))
        }
        RowAction::Connect => row.child(action_button(
            format!("act-{}", device.address),
            "Connect",
            true,
            true,
            colors,
            on_click,
        )),
        RowAction::Share => row.child(action_button(
            format!("act-{}", device.address),
            "Share",
            false,
            true,
            colors,
            on_click,
        )),
        RowAction::Wait => row.child(action_button(
            format!("act-{}", device.address),
            if device.chosen { "Connect" } else { "Share" },
            device.chosen,
            false,
            colors,
            on_click,
        )),
    };
    row
}

fn place_line(kind: &str, place: &str) -> String {
    if kind.is_empty() {
        place.to_string()
    } else {
        format!("{kind} · {place}")
    }
}

fn peer_status(seen: link::Seen, colors: Palette) -> impl IntoElement {
    let (filled, title, detail) = match seen {
        link::Seen::Ready => (true, link::other_mac(), "Ready"),
        link::Seen::Crowd => (
            false,
            "More than one Mac".to_string(),
            "Hop uses one other Mac",
        ),
        link::Seen::Nearby => (
            false,
            "A Mac is nearby".to_string(),
            "Allow it here, then on that Mac",
        ),
        link::Seen::None => (
            false,
            "No other Mac".to_string(),
            "Devices stay where they are",
        ),
    };
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(10.0))
        .child(presence_mark(filled, colors.paper, colors))
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(1.0))
                .child(div().text_size(px(15.0)).child(title))
                .child(
                    div()
                        .text_size(px(13.0))
                        .text_color(rgb(colors.muted))
                        .child(detail.to_string()),
                ),
        )
}

fn allow_card(
    id: &str,
    name: &str,
    colors: Palette,
    on_allow: impl Fn(&ClickEvent, &mut Window, &mut gpui::App) + 'static,
) -> impl IntoElement {
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(10.0))
        .px(px(12.0))
        .py(px(10.0))
        .rounded(px(10.0))
        .bg(rgb(colors.card))
        .child(presence_mark(false, colors.card, colors))
        .child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .gap(px(1.0))
                .child(div().text_size(px(15.0)).child(name.to_string()))
                .child(
                    div()
                        .text_size(px(13.0))
                        .text_color(rgb(colors.muted))
                        .child("Allow it here, then on that Mac"),
                ),
        )
        .child(action_button(
            format!("allow-{id}"),
            "Allow",
            true,
            true,
            colors,
            on_allow,
        ))
}

fn section_label(text: &str, colors: Palette) -> impl IntoElement {
    div()
        .pt(px(4.0))
        .text_size(px(12.0))
        .font_weight(FontWeight::BOLD)
        .text_color(rgb(colors.muted))
        .child(text.to_string())
}

fn note(text: impl Into<String>, colors: Palette) -> impl IntoElement {
    div()
        .text_size(px(14.0))
        .text_color(rgb(colors.muted))
        .child(text.into())
}

fn presence_mark(filled: bool, hole: u32, colors: Palette) -> impl IntoElement {
    let (outer, inner, size) = if filled {
        (colors.good, colors.good, 10.0)
    } else {
        (colors.line, hole, 6.0)
    };
    div()
        .w(px(10.0))
        .h(px(10.0))
        .flex_shrink_0()
        .rounded(px(5.0))
        .bg(rgb(outer))
        .flex()
        .items_center()
        .justify_center()
        .child(
            div()
                .w(px(size))
                .h(px(size))
                .rounded(px(size / 2.0))
                .bg(rgb(inner)),
        )
}

fn status_chip(text: &str, here: bool, colors: Palette) -> impl IntoElement {
    div()
        .h(px(22.0))
        .px(px(8.0))
        .flex()
        .flex_shrink_0()
        .items_center()
        .rounded(px(11.0))
        .bg(rgb(if here { colors.good_wash } else { colors.line }))
        .text_size(px(12.0))
        .text_color(rgb(if here { colors.good } else { colors.muted }))
        .child(text.to_string())
}

fn action_button(
    id: String,
    label: &str,
    filled: bool,
    enabled: bool,
    colors: Palette,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut gpui::App) + 'static,
) -> impl IntoElement {
    let (bg, ink) = match (filled, enabled) {
        (true, true) => (colors.text, colors.button_text),
        (false, true) => (colors.line, colors.text),
        _ => (colors.line, colors.muted),
    };
    let button = div()
        .id(id)
        .h(px(30.0))
        .px(px(12.0))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(8.0))
        .bg(rgb(bg))
        .text_color(rgb(ink))
        .text_size(px(13.0))
        .font_weight(FontWeight::BOLD)
        .child(label.to_string());
    if enabled {
        button.cursor_pointer().on_click(on_click)
    } else {
        button
    }
}

fn outline_button(
    id: String,
    label: &str,
    colors: Palette,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut gpui::App) + 'static,
) -> impl IntoElement {
    div()
        .id(id)
        .p(px(1.0))
        .flex_shrink_0()
        .rounded(px(8.0))
        .bg(rgb(colors.stroke))
        .cursor_pointer()
        .on_click(on_click)
        .child(
            div()
                .h(px(28.0))
                .px(px(11.0))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(7.0))
                .bg(rgb(colors.card))
                .text_size(px(13.0))
                .font_weight(FontWeight::BOLD)
                .text_color(rgb(colors.text))
                .child(label.to_string()),
        )
}
