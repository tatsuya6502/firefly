use std::alloc::AllocError;
use std::sync::Arc;

use thiserror::Error;

use std::backtrace::Backtrace;

#[derive(Error, Debug, Clone)]
#[error("allocation error")]
pub struct Alloc {
    backtrace: Arc<Backtrace>,
}
impl Alloc {
    #[inline(always)]
    pub fn new() -> Self {
        Self {
            backtrace: Arc::new(Backtrace::capture()),
        }
    }

    pub fn with_trace(trace: Backtrace) -> Self {
        Self {
            backtrace: Arc::new(trace),
        }
    }
}

impl Eq for Alloc {}
impl PartialEq for Alloc {
    /// For equality purposes, the backtrace is ignored
    fn eq(&self, _other: &Self) -> bool {
        true
    }
}

impl From<AllocError> for Alloc {
    #[inline(always)]
    fn from(_: AllocError) -> Self {
        Self::new()
    }
}
