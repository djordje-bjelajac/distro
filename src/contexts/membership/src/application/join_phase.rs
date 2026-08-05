use crate::application::MembershipState;

/// A live bootstrap ladder, as the status line sees it.
///
/// Exists only so the `Joining` status cannot outlive the join that set it.
/// The ladder of D1 has many exits — a rung connects, every rung fails, or the
/// event publisher gives out mid-walk and the handler returns with `?` — and a
/// hand-written "clear the flag" at each of them is one `return` away from a
/// status line permanently stuck on `Joining`, which is precisely the
/// indistinguishable-from-a-hang failure AC3 forbids.
///
/// Not `Clone`, and borrowed rather than owned: one join, one guard.
pub(crate) struct JoinPhase<'a> {
    state: &'a MembershipState,
}

impl<'a> JoinPhase<'a> {
    pub(crate) const fn over(state: &'a MembershipState) -> Self {
        Self { state }
    }
}

impl Drop for JoinPhase<'_> {
    fn drop(&mut self) {
        self.state.end_join();
    }
}
