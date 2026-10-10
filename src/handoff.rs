//! Whether chosen devices may leave this Mac.

#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

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
    CouldNotReach,
    NoAnswer,
    BadReply,
    GaveUp,
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
        Some(_) => AskResult::Stay(StayReason::BadReply),
        None => AskResult::Stay(StayReason::NoAnswer),
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
        Some(_) => WantResult::Stay(StayReason::BadReply),
        None => WantResult::Stay(StayReason::NoAnswer),
    }
}

pub fn stay_after_read(kind: std::io::ErrorKind) -> StayReason {
    match kind {
        std::io::ErrorKind::TimedOut => StayReason::NoAnswer,
        std::io::ErrorKind::InvalidData => StayReason::BadReply,
        _ => StayReason::CouldNotReach,
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
            Some("Hop isn't running on the other Mac. The devices stayed connected here.")
        }
        StayReason::CouldNotReach => {
            Some("Couldn't reach the other Mac. The devices stayed connected here.")
        }
        StayReason::NoAnswer => {
            Some("The other Mac did not answer. The devices stayed connected here.")
        }
        StayReason::GaveUp => {
            Some("Stopped waiting for the other Mac. The devices stayed connected here.")
        }
        StayReason::BadReply => {
            Some("The other Mac's reply could not be used. The devices stayed connected here.")
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
            Some("Nothing can move yet. On the Mac that has the device, let the other Mac move it.")
        }
        StayReason::AlreadyHere => Some("Those shared devices are already on this Mac."),
        StayReason::Busy => Some("Hop is already moving a device."),
        StayReason::StayedThere => Some("The other Mac still has the devices."),
    }
}

pub fn reconnected_notice() -> &'static str {
    "Couldn't move the devices. They are connected here again."
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

pub fn tooltip(peer: Peer) -> String {
    match peer {
        Peer::Missing => {
            "Hop isn't running on the other Mac. Devices stay where they are.".to_string()
        }
        Peer::Ready => {
            "A checkmark means connected here. Move here brings a device to this Mac.".to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AskResult, BluetoothOp, DropResult, Outcome, Peer, PeerReply, Preflight, ReleaseResult,
        StayReason, WantResult, after_ask, after_drop, after_release, after_want, execute, run,
        stay_after_read, stayed_notice, tooltip,
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
        assert_eq!(after_ask(None), AskResult::Stay(StayReason::NoAnswer));
        assert_eq!(
            after_ask(Some("later")),
            AskResult::Stay(StayReason::BadReply)
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
        assert_eq!(after_want(None), WantResult::Stay(StayReason::NoAnswer));
        assert_eq!(
            after_want(Some("later")),
            WantResult::Stay(StayReason::BadReply)
        );
    }

    #[test]
    fn a_failed_release_reconnects() {
        assert_eq!(after_release(Some("took")), ReleaseResult::Moved);
        assert_eq!(after_release(Some("failed")), ReleaseResult::Reconnect);
        assert_eq!(after_release(None), ReleaseResult::Reconnect);
    }

    #[test]
    fn the_notice_says_why_the_devices_stayed() {
        assert_eq!(
            stayed_notice(StayReason::PeerUnreachable),
            Some("Hop isn't running on the other Mac. The devices stayed connected here.")
        );
        assert_eq!(
            stayed_notice(StayReason::CouldNotReach),
            Some("Couldn't reach the other Mac. The devices stayed connected here.")
        );
        assert_eq!(
            stayed_notice(stay_after_read(std::io::ErrorKind::TimedOut)),
            Some("The other Mac did not answer. The devices stayed connected here.")
        );
        assert_eq!(
            stayed_notice(StayReason::BadReply),
            Some("The other Mac's reply could not be used. The devices stayed connected here.")
        );
        assert_eq!(
            stayed_notice(StayReason::GaveUp),
            Some("Stopped waiting for the other Mac. The devices stayed connected here.")
        );
        assert_eq!(
            stay_after_read(std::io::ErrorKind::ConnectionReset),
            StayReason::CouldNotReach
        );
        assert_eq!(
            stay_after_read(std::io::ErrorKind::InvalidData),
            StayReason::BadReply
        );
    }

    #[test]
    fn the_tooltip_says_where_the_other_mac_is() {
        assert_eq!(
            tooltip(Peer::Missing),
            "Hop isn't running on the other Mac. Devices stay where they are."
        );
        assert_eq!(
            tooltip(Peer::Ready),
            "A checkmark means connected here. Move here brings a device to this Mac."
        );
    }
}
