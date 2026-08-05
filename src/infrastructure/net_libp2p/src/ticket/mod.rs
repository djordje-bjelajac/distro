//! The string form of a [`JoinTicket`](membership::domain::JoinTicket) — D1's
//! third bootstrap rung made copy-pasteable.
//!
//! The domain deliberately holds no encoding: a ticket exists only once its
//! parts are individually valid, and "how it is written down" is a transport
//! concern (canvas §2.2). This module is that concern, and nothing more — it
//! never decides whether a ticket may be redeemed.

mod join_ticket_codec;
mod join_ticket_codec_error;
#[cfg(test)]
mod join_ticket_codec_test;

pub use join_ticket_codec::JoinTicketCodec;
pub use join_ticket_codec_error::JoinTicketCodecError;
