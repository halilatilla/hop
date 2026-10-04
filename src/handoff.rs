//! Whether chosen devices may leave this Mac.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Peer {
    Missing,
    #[allow(dead_code)]
    Ready,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Preflight {
    Unreachable,
    Accepted,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PeerReply {
    TookThem,
    Failed,
}

#[allow(dead_code)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BluetoothOp {
    Disconnect(Vec<String>),
    Connect(Vec<String>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StayReason {
    NothingHere,
    PeerUnreachable,
    NeedsAllow,
    Crowd,
    Refused,
    NotPaired,
    StillHere,
    NothingThere,
    NothingShared,
    AlreadyHere,
    Busy,
    StayedThere,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    Stayed(StayReason),
    #[allow(dead_code)]
    Moved(Vec<String>),
    Reconnected(Vec<String>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AskResult {
    Disconnect,
    Stay(StayReason),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReleaseResult {
    Moved,
    Reconnect,
}

pub fn after_ask(reply: Option<&str>) -> AskResult {
    match reply {
        Some("accept") => AskResult::Disconnect,
        Some("refuse") => AskResult::Stay(StayReason::Refused),
        Some("unpaired") => AskResult::Stay(StayReason::NotPaired),
        _ => AskResult::Stay(StayReason::PeerUnreachable),
    }
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DropResult {
    Release,
    Stay(StayReason),
}

#[allow(dead_code)]
pub fn after_drop(gone: bool) -> DropResult {
    if gone {
        DropResult::Release
    } else {
        DropResult::Stay(StayReason::StillHere)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WantResult {
    Connect,
    Local,
    Stay(StayReason),
}

pub fn after_want(reply: Option<&str>) -> WantResult {
    match reply {
        Some("released") => WantResult::Connect,
        Some("absent") => WantResult::Local,
        Some("refuse") => WantResult::Stay(StayReason::Refused),
        Some("busy") => WantResult::Stay(StayReason::Busy),
        Some("still") => WantResult::Stay(StayReason::StayedThere),
        _ => WantResult::Stay(StayReason::PeerUnreachable),
    }
}

pub fn after_release(reply: Option<&str>) -> ReleaseResult {
    match reply {
        Some("took") => ReleaseResult::Moved,
        _ => ReleaseResult::Reconnect,
    }
}

pub fn stayed_notice(reason: StayReason) -> Option<&'static str> {
    match reason {
        StayReason::NothingHere => Some("That device is not connected on this Mac."),
        StayReason::PeerUnreachable => {
            Some("The other Mac is not running Hop. Devices stayed on this Mac.")
        }
        StayReason::NeedsAllow => {
            Some("Allow the other Mac, on both Macs. Devices stayed on this Mac.")
        }
        StayReason::Crowd => {
            Some("More than one other Mac is running Hop. Devices stayed on this Mac.")
        }
        StayReason::Refused => {
            Some("The other Mac has not allowed this Mac. Devices stayed on this Mac.")
        }
        StayReason::NotPaired => Some(
            "That device is not paired on the other Mac. Pair it there once, in Bluetooth settings. It stayed on this Mac.",
        ),
        StayReason::StillHere => Some("The devices stayed on this Mac."),
        StayReason::NothingThere => Some("No shared device is on the other Mac."),
        StayReason::NothingShared => {
            Some("Nothing is on the shared list. On the Mac that has the device, share it.")
        }
        StayReason::AlreadyHere => Some("Those shared devices are already on this Mac."),
        StayReason::Busy => Some("Hop is already moving a device."),
        StayReason::StayedThere => Some("The other Mac still has the devices."),
    }
}

pub fn reconnected_notice() -> &'static str {
    "The other Mac did not take the devices. They are connected on this Mac again."
}

#[allow(dead_code)]
pub fn run(
    preflight: Preflight,
    connected: &[String],
    reply: PeerReply,
) -> (Outcome, Vec<BluetoothOp>) {
    if connected.is_empty() {
        return (Outcome::Stayed(StayReason::NothingHere), Vec::new());
    }
    if preflight != Preflight::Accepted {
        return (Outcome::Stayed(StayReason::PeerUnreachable), Vec::new());
    }
    let addresses = connected.to_vec();
    match reply {
        PeerReply::TookThem => (
            Outcome::Moved(addresses.clone()),
            vec![BluetoothOp::Disconnect(addresses)],
        ),
        PeerReply::Failed => (
            Outcome::Reconnected(addresses.clone()),
            vec![
                BluetoothOp::Disconnect(addresses.clone()),
                BluetoothOp::Connect(addresses),
            ],
        ),
    }
}

#[allow(dead_code)]
pub fn execute(ops: &[BluetoothOp]) -> bool {
    ops.is_empty()
}

pub fn status_label(chosen: usize, peer: Peer) -> String {
    match peer {
        Peer::Missing if chosen == 0 => "no Mac".to_string(),
        Peer::Missing => format!("{chosen} · no Mac"),
        Peer::Ready if chosen == 0 => "Hop".to_string(),
        Peer::Ready => chosen.to_string(),
    }
}

pub fn tooltip(peer: Peer) -> String {
    match peer {
        Peer::Missing => {
            "The other Mac is not running Hop. Shared devices stay where they are.".to_string()
        }
        Peer::Ready => {
            "A checkmark is on this Mac. Choose a device to connect it here.".to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AskResult, BluetoothOp, DropResult, Outcome, Peer, PeerReply, Preflight, ReleaseResult,
        StayReason, WantResult, after_ask, after_drop, after_release, after_want, execute, run,
        status_label, tooltip,
    };

    fn mouse() -> Vec<String> {
        vec!["aa-bb".to_string()]
    }

    #[test]
    fn an_unreachable_mac_disconnects_nothing() {
        let (outcome, ops) = run(Preflight::Unreachable, &mouse(), PeerReply::Failed);
        assert_eq!(outcome, Outcome::Stayed(StayReason::PeerUnreachable));
        assert!(ops.is_empty());
    }

    #[test]
    fn send_with_nothing_here_does_not_fetch_them_back() {
        let (outcome, ops) = run(Preflight::Accepted, &[], PeerReply::TookThem);
        assert_eq!(outcome, Outcome::Stayed(StayReason::NothingHere));
        assert!(ops.is_empty());
    }

    #[test]
    fn an_accepted_move_disconnects_then_the_other_mac_connects() {
        let (outcome, ops) = run(Preflight::Accepted, &mouse(), PeerReply::TookThem);
        assert_eq!(outcome, Outcome::Moved(mouse()));
        assert_eq!(ops, vec![BluetoothOp::Disconnect(mouse())]);
    }

    #[test]
    fn a_failed_take_reconnects_here() {
        let (outcome, ops) = run(Preflight::Accepted, &mouse(), PeerReply::Failed);
        assert_eq!(outcome, Outcome::Reconnected(mouse()));
        assert_eq!(
            ops,
            vec![
                BluetoothOp::Disconnect(mouse()),
                BluetoothOp::Connect(mouse())
            ]
        );
    }

    #[test]
    fn operations_are_refused_until_the_other_mac_can_connect() {
        assert!(execute(&[]));
        assert!(!execute(&[BluetoothOp::Disconnect(mouse())]));
        assert!(!execute(&[
            BluetoothOp::Disconnect(mouse()),
            BluetoothOp::Connect(mouse())
        ]));
    }

    #[test]
    fn a_stranger_reply_does_not_disconnect() {
        assert_eq!(
            after_ask(None),
            AskResult::Stay(StayReason::PeerUnreachable)
        );
        assert_eq!(
            after_ask(Some("refuse")),
            AskResult::Stay(StayReason::Refused)
        );
        assert_eq!(
            after_ask(Some("unpaired")),
            AskResult::Stay(StayReason::NotPaired)
        );
        assert_eq!(after_ask(Some("accept")), AskResult::Disconnect);
    }

    #[test]
    fn devices_that_are_still_connected_are_not_released() {
        assert_eq!(after_drop(false), DropResult::Stay(StayReason::StillHere));
        assert_eq!(after_drop(true), DropResult::Release);
    }

    #[test]
    fn asking_for_a_device_connects_only_after_the_other_mac_lets_go() {
        assert_eq!(after_want(Some("released")), WantResult::Connect);
        assert_eq!(after_want(Some("absent")), WantResult::Local);
        assert_eq!(
            after_want(Some("still")),
            WantResult::Stay(StayReason::StayedThere)
        );
        assert_eq!(after_want(Some("busy")), WantResult::Stay(StayReason::Busy));
        assert_eq!(
            after_want(None),
            WantResult::Stay(StayReason::PeerUnreachable)
        );
    }

    #[test]
    fn a_failed_release_reconnects() {
        assert_eq!(after_release(Some("took")), ReleaseResult::Moved);
        assert_eq!(after_release(Some("failed")), ReleaseResult::Reconnect);
        assert_eq!(after_release(None), ReleaseResult::Reconnect);
    }

    #[test]
    fn the_menu_bar_shows_the_peer_and_the_count() {
        assert_eq!(status_label(0, Peer::Missing), "no Mac");
        assert_eq!(status_label(2, Peer::Missing), "2 · no Mac");
        assert_eq!(status_label(0, Peer::Ready), "Hop");
        assert_eq!(status_label(1, Peer::Ready), "1");
        assert_eq!(
            tooltip(Peer::Missing),
            "The other Mac is not running Hop. Shared devices stay where they are."
        );
        assert_eq!(
            tooltip(Peer::Ready),
            "A checkmark is on this Mac. Choose a device to connect it here."
        );
    }
}
