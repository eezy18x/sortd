//! Shared application state and background job tracking.

use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use serde::Serialize;

use crate::database::Database;
use crate::duplicates::DuplicateProgress;
use crate::error::{AppError, AppResult};
use crate::operations::execute::OperationProgress;
use crate::scanner::ScanProgress;

pub const EVENT_SCAN: &str = "sortd://scan-progress";
pub const EVENT_DUPLICATES: &str = "sortd://duplicates-progress";
pub const EVENT_OPERATION: &str = "sortd://operation-progress";
/// Emitted when indexed data changed; the UI refetches its queries.
pub const EVENT_INDEX_CHANGED: &str = "sortd://index-changed";

#[derive(Default)]
pub struct JobSlot<P> {
    pub cancel: Option<Arc<AtomicBool>>,
    pub last: Option<P>,
}

#[derive(Default)]
pub struct Jobs {
    pub scan: Mutex<JobSlot<ScanProgress>>,
    pub duplicates: Mutex<JobSlot<DuplicateProgress>>,
    pub operation: Mutex<JobSlot<OperationProgress>>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobStatus {
    pub scan_running: bool,
    pub duplicates_running: bool,
    pub operation_running: bool,
    pub scan: Option<ScanProgress>,
    pub duplicates: Option<DuplicateProgress>,
    pub operation: Option<OperationProgress>,
}

pub struct AppState {
    pub db: Arc<Database>,
    pub jobs: Arc<Jobs>,
}

fn lock<T>(m: &Mutex<T>) -> AppResult<std::sync::MutexGuard<'_, T>> {
    m.lock()
        .map_err(|_| AppError::Internal("job state lock poisoned".into()))
}

impl Jobs {
    pub fn status(&self) -> AppResult<JobStatus> {
        let s = lock(&self.scan)?;
        let d = lock(&self.duplicates)?;
        let o = lock(&self.operation)?;
        Ok(JobStatus {
            scan_running: s.cancel.is_some(),
            duplicates_running: d.cancel.is_some(),
            operation_running: o.cancel.is_some(),
            scan: s.last.clone(),
            duplicates: d.last.clone(),
            operation: o.last.clone(),
        })
    }

    /// Claims a job slot, failing when another job would conflict with it.
    /// Scans, duplicate detection and file operations are mutually exclusive
    /// so the index is never rewritten underneath a running operation.
    pub fn claim(&self, which: Job) -> AppResult<Arc<AtomicBool>> {
        let mut s = lock(&self.scan)?;
        let mut d = lock(&self.duplicates)?;
        let mut o = lock(&self.operation)?;
        if s.cancel.is_some() {
            return Err(AppError::Busy("a scan is in progress".into()));
        }
        if d.cancel.is_some() {
            return Err(AppError::Busy("duplicate detection is in progress".into()));
        }
        if o.cancel.is_some() {
            return Err(AppError::Busy("a file operation is in progress".into()));
        }
        let flag = Arc::new(AtomicBool::new(false));
        match which {
            Job::Scan => {
                s.cancel = Some(flag.clone());
                s.last = None;
            }
            Job::Duplicates => {
                d.cancel = Some(flag.clone());
                d.last = None;
            }
            Job::Operation => {
                o.cancel = Some(flag.clone());
                o.last = None;
            }
        }
        Ok(flag)
    }

    pub fn release(&self, which: Job) {
        match which {
            Job::Scan => {
                if let Ok(mut s) = self.scan.lock() {
                    s.cancel = None;
                }
            }
            Job::Duplicates => {
                if let Ok(mut s) = self.duplicates.lock() {
                    s.cancel = None;
                }
            }
            Job::Operation => {
                if let Ok(mut s) = self.operation.lock() {
                    s.cancel = None;
                }
            }
        }
    }

    pub fn cancel(&self, which: Job) -> bool {
        let flag = match which {
            Job::Scan => self.scan.lock().ok().and_then(|s| s.cancel.clone()),
            Job::Duplicates => self.duplicates.lock().ok().and_then(|s| s.cancel.clone()),
            Job::Operation => self.operation.lock().ok().and_then(|s| s.cancel.clone()),
        };
        match flag {
            Some(f) => {
                f.store(true, std::sync::atomic::Ordering::Relaxed);
                true
            }
            None => false,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum Job {
    Scan,
    Duplicates,
    Operation,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jobs_are_mutually_exclusive() {
        let jobs = Jobs::default();
        let flag = jobs.claim(Job::Scan).unwrap();
        assert!(matches!(
            jobs.claim(Job::Duplicates),
            Err(AppError::Busy(_))
        ));
        assert!(matches!(jobs.claim(Job::Operation), Err(AppError::Busy(_))));
        assert!(jobs.cancel(Job::Scan));
        assert!(flag.load(std::sync::atomic::Ordering::Relaxed));
        jobs.release(Job::Scan);
        assert!(jobs.claim(Job::Operation).is_ok());
        assert!(!jobs.cancel(Job::Scan));
    }
}
