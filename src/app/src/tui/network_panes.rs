use membership::domain::NetworkStatus;
use membership::ports::MembershipQueryPort;
use shared_types::PeerId;

use crate::composition::PeerTrust;
use crate::tui::{PeerLabels, RosterRow, roster_rows};

/// Everything on screen that describes the network, taken from **one** reading
/// of the roster (canvas D5, OP-7).
///
/// # What went wrong without it
///
/// The frame used to call `network_status()` for the status line and
/// `known_peers()` for the roster: two calls, two traversals, two clock
/// readings. Observed live on two instances at once, the result was `connected
/// (2 peers)` above a roster in which every row read `offline`.
///
/// Taking both from one snapshot does not by itself fix that — the
/// contradiction was semantic and would have survived any number of atomic
/// reads, which is why the substance of the fix is in `membership`'s
/// `PeerStanding` and in what [`RosterRow::presence_text`] renders. But the two
/// reads were the other half: they let the count describe a roster the rows no
/// longer were. This type removes them. There is one call, and both halves of
/// the screen are read off its result.
///
/// # Why a type rather than two locals
///
/// Because a caller cannot assemble one from two reads. The status is not a
/// field anyone sets here: it arrives already counted from the rows it belongs
/// to, through `NetworkView`, whose count is derived from the standings its own
/// rows carry. So `connected (n)` is the number of rows that draw as linked by
/// construction, and the only way to break that is to change `membership`.
pub struct NetworkPanes {
    status: NetworkStatus,
    roster: Vec<RosterRow>,
}

impl NetworkPanes {
    /// Reads the roster once and builds both halves from that reading.
    ///
    /// The only query call in the frame's network path. `known_peers` and
    /// `network_status` are deliberately not called: each is a second reading,
    /// and a second reading is what the two panes disagreed about.
    pub fn gather(
        queries: &dyn MembershipQueryPort,
        labels: PeerLabels,
        trust_of: impl Fn(PeerId) -> PeerTrust,
    ) -> Self {
        let (status, peers) = queries.network_view().into_parts();

        Self {
            status,
            roster: roster_rows(&peers, labels, trust_of),
        }
    }

    /// How connected this instance is, as of this snapshot — the count the
    /// roster below it was read off.
    pub const fn status(&self) -> NetworkStatus {
        self.status
    }

    /// The roster pane's rows, in the roster's own `PeerId` order.
    pub fn roster(&self) -> &[RosterRow] {
        &self.roster
    }

    /// The peers the conversation list offers a direct channel with.
    ///
    /// Derived from the rows rather than fetched, so the list and the roster
    /// name the same peers: a conversation for a peer that is not in the roster
    /// — or a roster entry with nowhere to write to it — is the same class of
    /// defect one pane down.
    pub fn peer_ids(&self) -> Vec<PeerId> {
        self.roster.iter().map(|row| row.peer).collect()
    }
}
