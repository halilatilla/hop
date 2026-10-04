//! Whether chosen devices may leave this Mac.
//!
//! The other Mac has to be awake and running Hop before anything disconnects.
//! Tests pass a peer in directly. This build has not discovered one yet.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Peer {
    Missing,
    Ready,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Effect {
    /// Leave every device connected here.
    Held,
    /// The other Mac can take these addresses.
    HandOff(Vec<String>),
}

/// This build has no link to the other Mac, so the peer stays missing.
pub fn observe_peer() -> Peer {
    Peer::Missing
}

pub fn plan(peer: Peer, connected: &[String]) -> Effect {
    if peer != Peer::Ready || connected.is_empty() {
        return Effect::Held;
    }
    Effect::HandOff(connected.to_vec())
}

/// Asks the other Mac to connect these devices.
/// The link is not in this build, so nothing on this Mac disconnects.
pub fn deliver(addresses: &[String]) -> usize {
    let _ = addresses;
    0
}

pub fn status_label(chosen: usize, peer: Peer) -> String {
    match peer {
        Peer::Missing if chosen == 0 => "no Mac".to_string(),
        Peer::Missing => format!("{chosen} · no Mac"),
        Peer::Ready if chosen == 0 => "Hop".to_string(),
        Peer::Ready => chosen.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::{Effect, Peer, deliver, plan, status_label};

    #[test]
    fn a_missing_mac_keeps_devices_here() {
        let chosen = ["aa-bb".to_string()];
        assert_eq!(plan(Peer::Missing, &chosen), Effect::Held);
        assert_eq!(plan(Peer::Missing, &[]), Effect::Held);
    }

    #[test]
    fn a_ready_mac_takes_only_a_non_empty_list() {
        assert_eq!(plan(Peer::Ready, &[]), Effect::Held);
        let chosen = ["aa-bb".to_string(), "cc-dd".to_string()];
        assert_eq!(plan(Peer::Ready, &chosen), Effect::HandOff(chosen.to_vec()));
    }

    #[test]
    fn delivery_does_not_disconnect_yet() {
        assert_eq!(deliver(&["aa-bb".to_string()]), 0);
        assert_eq!(deliver(&[]), 0);
    }

    #[test]
    fn the_menu_bar_shows_the_peer_and_the_count() {
        assert_eq!(status_label(0, Peer::Missing), "no Mac");
        assert_eq!(status_label(2, Peer::Missing), "2 · no Mac");
        assert_eq!(status_label(0, Peer::Ready), "Hop");
        assert_eq!(status_label(1, Peer::Ready), "1");
    }
}
