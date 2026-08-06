use std::sync::Arc;

use crate::application::ConversationRegistry;
use crate::application::commands::{ClearHistory, ClearHistoryHandler};
use crate::ports::{ClearHistoryPort, ClearedHistory, MessageLogError, MessageLogPort};

/// The maintenance half of this context's inbound surface: one
/// [`ClearHistoryPort`] implementation over the single handler behind it
/// (canvas `0013`, D6).
///
/// It holds a handler rather than reimplementing it, exactly as the other
/// three services do, so the use case keeps its own file and its own tests and
/// this type adds only the translation from the port's argument-free method to
/// the imperative command.
#[derive(Clone)]
pub struct ClearHistoryService {
    clear_history: ClearHistoryHandler,
}

impl ClearHistoryService {
    pub(crate) fn new(
        registry: Arc<ConversationRegistry>,
        log: Arc<dyn MessageLogPort + Send + Sync>,
    ) -> Self {
        Self {
            clear_history: ClearHistoryHandler::new(registry, log),
        }
    }
}

impl ClearHistoryPort for ClearHistoryService {
    fn clear_history(&self) -> Result<ClearedHistory, MessageLogError> {
        self.clear_history.handle(ClearHistory)
    }
}
