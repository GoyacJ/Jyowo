use std::sync::Arc;

use tokio::sync::watch;

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum InterruptCause {
    User,
    Parent,
    System { reason: String },
    Timeout,
    Budget,
}

#[derive(Clone)]
pub struct CancellationToken {
    inner: Arc<CancellationState>,
}

struct CancellationState {
    cause: watch::Sender<Option<InterruptCause>>,
}

impl CancellationToken {
    #[must_use]
    pub fn new() -> Self {
        let (cause, _) = watch::channel(None);
        Self {
            inner: Arc::new(CancellationState { cause }),
        }
    }

    pub fn cancel(&self, cause: InterruptCause) {
        let mut cause = Some(cause);
        self.inner.cause.send_if_modified(|current| {
            if current.is_none() {
                *current = cause.take();
                true
            } else {
                false
            }
        });
    }

    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.inner.cause.borrow().is_some()
    }

    pub async fn cause(&self) -> Option<InterruptCause> {
        self.inner.cause.borrow().clone()
    }

    pub async fn cancelled(&self) -> InterruptCause {
        let mut cause = self.inner.cause.subscribe();
        loop {
            if let Some(interrupt) = cause.borrow_and_update().clone() {
                return interrupt;
            }

            let _ = cause.changed().await;
        }
    }
}

impl Default for CancellationToken {
    fn default() -> Self {
        Self::new()
    }
}
