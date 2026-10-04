//! Whether chosen devices may leave this Mac.
//!
//! A move checks the other Mac before any device disconnects. If that check
//! fails, nothing changes. If the devices do disconnect and the other Mac does
//! not take them, they connect again here.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Peer {
    Missing,
    /// Produced once a link can prove the other Mac is running Hop.
    #[allow(dead_code)]
    Ready,
}

/// Result of asking the other Mac whether it will accept a move.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Preflight {
    Unreachable,
    Accepted,
}

/// What the other Mac did after the devices disconnected here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PeerReply {
    /// The other Mac connected the devices. Not produced until a link exists.
    #[allow(dead_code)]
    TookThem,
    Failed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BluetoothOp {
    Disconnect(Vec<String>),
    Connect(Vec<String>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StayReason {
    /// Send while this Mac is not holding the devices. A repeat does not fetch them back.
    NothingHere,
    /// The other Mac did not accept the move. Nothing disconnected.
    PeerUnreachable,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    Stayed(StayReason),
    Moved(Vec<String>),
    /// The other Mac did not take them, so they connect again on this Mac.
    Reconnected(Vec<String>),
}

/// This build has no link to the other Mac, so a ping cannot succeed.
pub fn observe_peer() -> Peer {
    Peer::Missing
}

pub fn preflight_now() -> Preflight {
    Preflight::Unreachable
}

/// Plans the move. `connected` is the devices this Mac is holding.
/// An unreachable peer, or an empty list, produces no Bluetooth operations.
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

/// Applies a plan. Any operation is refused until Hop can ask the other Mac to
/// connect the devices, so a disconnect never happens on its own.
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
            "The other Mac is not running Hop. Send leaves devices on this Mac.".to_string()
        }
        Peer::Ready => "The other Mac can take the devices that are connected here.".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        BluetoothOp, Outcome, Peer, PeerReply, Preflight, StayReason, execute, run, status_label,
        tooltip,
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
    fn the_menu_bar_shows_the_peer_and_the_count() {
        assert_eq!(status_label(0, Peer::Missing), "no Mac");
        assert_eq!(status_label(2, Peer::Missing), "2 · no Mac");
        assert_eq!(status_label(0, Peer::Ready), "Hop");
        assert_eq!(status_label(1, Peer::Ready), "1");
        assert_eq!(
            tooltip(Peer::Missing),
            "The other Mac is not running Hop. Send leaves devices on this Mac."
        );
    }
}
