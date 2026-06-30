use super::*;

pub(super) struct SessionLimitState {
    max: Option<u32>,
    active: AtomicU32,
}

pub(super) struct SessionLimitPermit {
    state: Arc<SessionLimitState>,
    armed: bool,
}

impl SessionLimitState {
    pub(super) fn new(max: Option<u32>) -> Self {
        Self {
            max,
            active: AtomicU32::new(0),
        }
    }

    pub(super) fn try_acquire(self: &Arc<Self>) -> Result<SessionLimitPermit, HarnessError> {
        let Some(max) = self.max else {
            return Ok(SessionLimitPermit {
                state: Arc::clone(self),
                armed: false,
            });
        };

        loop {
            let active = self.active.load(Ordering::Acquire);
            if active >= max {
                return Err(HarnessError::PermissionDenied(format!(
                    "tenant session limit exceeded: active={active}, max={max}"
                )));
            }
            if self
                .active
                .compare_exchange(active, active + 1, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                return Ok(SessionLimitPermit {
                    state: Arc::clone(self),
                    armed: true,
                });
            }
        }
    }

    pub(super) fn release(&self) {
        if self.max.is_some() {
            let _ = self
                .active
                .fetch_update(Ordering::AcqRel, Ordering::Acquire, |active| {
                    active.checked_sub(1)
                });
        }
    }
}

impl SessionLimitPermit {
    pub(super) fn disarm(mut self) {
        self.armed = false;
    }
}

impl Drop for SessionLimitPermit {
    fn drop(&mut self) {
        if self.armed {
            self.state.release();
        }
    }
}
