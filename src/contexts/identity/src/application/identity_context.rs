use std::sync::Arc;

use crate::application::LocalIdentityState;
use crate::application::commands::IdentityCommandService;
use crate::application::queries::IdentityQueryService;
use crate::ports::{IdentityKeyStorePort, TrustRecordStorePort};

/// The assembled `identity` context: its inbound command and query ports,
/// wired over the outbound ports a composition root supplies.
///
/// # Why the two sides are built together
///
/// CQRS separates the command and query *paths*, not the state they describe.
/// [`IdentityCommandService`] and [`IdentityQueryService`] must therefore see
/// one [`LocalIdentityState`], or a rename would land in a peer the query side
/// cannot see — a defect that would surface only at runtime, in the UI, as a
/// name that refuses to change. Constructing both here makes that mistake
/// unrepresentable at the root: there is no way to hand them different state.
///
/// # What OP-12 wires
///
/// The root supplies `infra-store-fs`'s implementations of
/// [`IdentityKeyStorePort`] (D5, the persistent keypair) and
/// [`TrustRecordStorePort`] (verification and the block list), then drives the
/// context through [`commands`](Self::commands) and
/// [`queries`](Self::queries) as `&dyn IdentityCommandPort` /
/// `&dyn IdentityQueryPort`. `IdentityQueryPort::blocked_peers` is the seam
/// invariant 11 hangs on: the root passes that list to `messaging`'s own
/// `AuthorPolicyPort`, so neither context imports the other.
///
/// Nothing here starts a task, opens a file, or reads a clock: the context is
/// inert until a command arrives.
pub struct IdentityContext {
    commands: IdentityCommandService,
    queries: IdentityQueryService,
}

impl IdentityContext {
    /// Assembles both sides over the given outbound ports.
    pub fn new(
        key_store: Arc<dyn IdentityKeyStorePort + Send + Sync>,
        trust_records: Arc<dyn TrustRecordStorePort + Send + Sync>,
    ) -> Self {
        let state = Arc::new(LocalIdentityState::uninitialized());

        Self {
            commands: IdentityCommandService::new(
                Arc::clone(&state),
                key_store,
                Arc::clone(&trust_records),
            ),
            queries: IdentityQueryService::new(state, trust_records),
        }
    }

    /// The inbound command port: everything that changes identity or trust.
    pub const fn commands(&self) -> &IdentityCommandService {
        &self.commands
    }

    /// The inbound query port: everything that only reads.
    pub const fn queries(&self) -> &IdentityQueryService {
        &self.queries
    }

    /// Splits the context so a root can hand each side to a different owner —
    /// a UI task and a network task, say — while both keep the shared state
    /// this constructor established.
    pub fn into_parts(self) -> (IdentityCommandService, IdentityQueryService) {
        (self.commands, self.queries)
    }
}
