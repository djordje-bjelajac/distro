use messaging::domain::ConversationId;
use shared_types::PeerId;

use crate::tui::PeerLabels;

/// Everything the interface remembers between frames.
///
/// # Why this is a type with tests rather than a handful of locals in a loop
///
/// A terminal interface is a state machine — a mode, a selection, a text
/// buffer, an overlay — and the bugs it has are the transitions: a keystroke
/// swallowed by an overlay, a selection that outlives the row it pointed at, a
/// buffer that survives a cancel and reappears on the next send. None of that
/// needs a terminal to reproduce, so none of it is in the render path.
///
/// The state deliberately holds **no domain data**. Conversations, messages,
/// roster rows and trust are read fresh from the query ports on every frame:
/// they are in-memory reads, and a cached copy is a second thing that can
/// disagree with the conversation the user is looking at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UiState {
    mode: Mode,
    overlay: Overlay,
    /// Index into the conversation list the frame just built.
    selected: usize,
    input: String,
    quitting: bool,
}

impl Default for UiState {
    fn default() -> Self {
        Self::new()
    }
}

impl UiState {
    /// Longest text accepted into the input line.
    ///
    /// The domain caps a message body at 16 KiB and a pasted join ticket at
    /// 4 KiB, and this is above both — the buffer is refused here only to stop
    /// a terminal paste of a whole file from being held in memory a keystroke
    /// at a time. The real limits are the domain's and the codec's, and they
    /// still apply on submit.
    pub const MAX_INPUT_BYTES: usize = 32 * 1024;

    pub fn new() -> Self {
        Self {
            mode: Mode::Browsing,
            overlay: Overlay::None,
            selected: 0,
            input: String::new(),
            quitting: false,
        }
    }

    pub const fn mode(&self) -> Mode {
        self.mode
    }

    pub const fn overlay(&self) -> &Overlay {
        &self.overlay
    }

    pub const fn selected(&self) -> usize {
        self.selected
    }

    pub fn input(&self) -> &str {
        &self.input
    }

    pub const fn is_quitting(&self) -> bool {
        self.quitting
    }

    /// Whether keystrokes are being typed into the input line rather than
    /// interpreted as commands.
    pub const fn is_typing(&self) -> bool {
        matches!(self.mode, Mode::Composing | Mode::RedeemingTicket)
    }

    pub const fn quit(&mut self) {
        self.quitting = true;
    }

    // ---------------------------------------------------------- selection

    /// Moves the selection down, wrapping. `count` is the number of rows the
    /// frame actually has, so a list that shrank cannot leave the selection
    /// pointing past its end.
    pub const fn select_next(&mut self, count: usize) {
        if count == 0 {
            self.selected = 0;
            return;
        }
        self.selected = (self.selected + 1) % count;
    }

    /// Moves the selection up, wrapping.
    pub const fn select_previous(&mut self, count: usize) {
        if count == 0 {
            self.selected = 0;
            return;
        }
        self.selected = if self.selected == 0 {
            count - 1
        } else {
            self.selected - 1
        };
    }

    /// Clamps the selection to a list of `count` rows.
    ///
    /// Called once per frame: peers appear and disappear while a user is
    /// looking at the screen, and a selection that survived the row it pointed
    /// at would send the next message to whoever took its place.
    pub const fn clamp_selection(&mut self, count: usize) {
        if count == 0 {
            self.selected = 0;
        } else if self.selected >= count {
            self.selected = count - 1;
        }
    }

    // --------------------------------------------------------------- input

    /// Starts composing a message.
    pub fn compose(&mut self) {
        self.overlay = Overlay::None;
        self.mode = Mode::Composing;
        self.input.clear();
    }

    /// Starts pasting a join ticket (D1's third rung).
    pub fn redeem_ticket(&mut self) {
        self.overlay = Overlay::None;
        self.mode = Mode::RedeemingTicket;
        self.input.clear();
    }

    /// Abandons whatever is being typed.
    ///
    /// The buffer is cleared rather than kept: a half-typed message that
    /// reappeared on the next compose would eventually be sent by accident,
    /// and on this interface the next compose may be to a different peer.
    pub fn cancel(&mut self) {
        self.mode = Mode::Browsing;
        self.input.clear();
    }

    pub fn insert(&mut self, character: char) {
        if self.input.len() + character.len_utf8() > Self::MAX_INPUT_BYTES {
            return;
        }
        self.input.push(character);
    }

    pub fn delete(&mut self) {
        self.input.pop();
    }

    /// Takes what was typed and returns to browsing.
    ///
    /// Returns `None` for a buffer that is empty or only whitespace — an empty
    /// message is not a message, and the domain would refuse it anyway
    /// (`MessageBody::Empty`). Refusing it here keeps a stray `Enter` from
    /// becoming a notice.
    pub fn submit(&mut self) -> Option<String> {
        let taken = std::mem::take(&mut self.input);
        self.mode = Mode::Browsing;

        let trimmed = taken.trim();
        (!trimmed.is_empty()).then(|| trimmed.to_owned())
    }

    // ------------------------------------------------------------- overlay

    /// Shows an overlay, leaving any typing alone.
    pub fn show(&mut self, overlay: Overlay) {
        self.overlay = overlay;
    }

    /// Closes whatever overlay is open. Returns whether one was.
    pub fn close_overlay(&mut self) -> bool {
        let was_open = !matches!(self.overlay, Overlay::None);
        self.overlay = Overlay::None;
        was_open
    }

    /// Opens `overlay`, or closes it if the same kind is already open.
    pub fn toggle(&mut self, overlay: Overlay) {
        if std::mem::discriminant(&self.overlay) == std::mem::discriminant(&overlay) {
            self.overlay = Overlay::None;
        } else {
            self.overlay = overlay;
        }
    }
}

/// What keystrokes currently mean.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Keys are commands.
    Browsing,
    /// Keys are text for a message.
    Composing,
    /// Keys are text for a pasted join ticket.
    RedeemingTicket,
}

/// What is drawn over the panes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Overlay {
    None,
    /// The keys, and the two disclosures S7 and S8 require be tellable.
    Help,
    /// The full fingerprints of this peer and the selected one, for the
    /// out-of-band comparison that verifies a peer (AC6).
    Fingerprints,
    /// A freshly minted join ticket, for copying (D1).
    Ticket(String),
    /// The local diagnostic counters (AC6, AC14, AC15).
    Diagnostics,
    /// About to forget every cached peer, carrying how many are at stake.
    ///
    /// The count is in the overlay rather than fetched at draw time because a
    /// confirmation has to be about the state the user was looking at when
    /// they asked. A roster that gained a peer between the question and the
    /// answer must not silently change what the question meant.
    ConfirmForgetPeers {
        peers: usize,
    },
    /// About to clear the conversation history, carrying how much is at stake.
    ConfirmClearHistory {
        messages: usize,
    },
}

impl Overlay {
    /// Whether this overlay is asking a question that destroys something if
    /// answered yes.
    ///
    /// The key map branches on this rather than on the individual variants, so
    /// a third destructive confirmation added later cannot accidentally be
    /// left out of the branch that makes ordinary keys stop working.
    pub const fn is_confirmation(&self) -> bool {
        matches!(
            self,
            Self::ConfirmForgetPeers { .. } | Self::ConfirmClearHistory { .. }
        )
    }
}

/// A conversation the interface can show: the broadcast channel, or a 1:1 with
/// one peer.
///
/// Every known peer gets an entry whether or not anything has been said, so a
/// user can start a conversation with a peer they have only just discovered.
/// `MessagingQueryPort::conversations` lists only those with recorded history,
/// which is the right answer to a different question.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConversationEntry {
    pub id: ConversationId,
    pub label: String,
}

impl ConversationEntry {
    /// The broadcast channel first, then one entry per known peer in roster
    /// order — a stable list, so a selection means the same thing between
    /// frames.
    pub fn list(peers: &[PeerId], labels: PeerLabels) -> Vec<Self> {
        let mut entries = vec![Self {
            id: ConversationId::Broadcast,
            label: "broadcast".to_owned(),
        }];

        entries.extend(peers.iter().map(|peer| Self {
            id: ConversationId::Direct(*peer),
            label: format!("direct {}", labels.label(*peer)),
        }));

        entries
    }

    /// The peer this conversation is with, or `None` for the broadcast
    /// channel.
    pub const fn counterpart(&self) -> Option<PeerId> {
        self.id.counterpart()
    }
}
