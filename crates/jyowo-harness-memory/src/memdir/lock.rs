use std::fs::{self, File, OpenOptions};
use std::path::Path;
use std::thread;
use std::time::{Duration, Instant};

use fs2::FileExt;
use harness_contracts::MemoryError;

use super::MemdirConcurrencyPolicy;

pub(crate) struct LockedFile {
    file: File,
}

impl LockedFile {
    pub(crate) fn acquire(
        path: &Path,
        policy: &MemdirConcurrencyPolicy,
    ) -> Result<Self, MemoryError> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(io_error)?;
        }

        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)
            .map_err(io_error)?;

        let started = Instant::now();
        let mut attempts = 0;

        loop {
            match file.try_lock_exclusive() {
                Ok(()) => return Ok(Self { file }),
                Err(error) if should_retry(started, attempts, policy) => {
                    attempts += 1;
                    sleep_jitter(policy);
                    if started.elapsed() >= policy.lock_timeout {
                        return Err(lock_error(error, attempts));
                    }
                }
                Err(error) => return Err(lock_error(error, attempts)),
            }
        }
    }
}

impl Drop for LockedFile {
    fn drop(&mut self) {
        let _ = fs2::FileExt::unlock(&self.file);
    }
}

fn should_retry(started: Instant, attempts: u32, policy: &MemdirConcurrencyPolicy) -> bool {
    attempts < policy.retry_max && started.elapsed() < policy.lock_timeout
}

fn sleep_jitter(policy: &MemdirConcurrencyPolicy) {
    let low = *policy.retry_jitter_ms.start();
    let high = *policy.retry_jitter_ms.end();
    let millis = if low >= high {
        low
    } else {
        fastrand::u64(low..=high)
    };
    thread::sleep(Duration::from_millis(millis));
}

fn lock_error(error: std::io::Error, retries: u32) -> MemoryError {
    let _ = error;
    MemoryError::ConcurrentWriteLockFailed { retries }
}

fn io_error(error: std::io::Error) -> MemoryError {
    MemoryError::Io(format!("memdir lock io error: {error}"))
}
