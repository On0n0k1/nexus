#[cfg(loom)]
pub(crate) use loom::sync::atomic::{AtomicBool, Ordering};

#[cfg(not(loom))]
pub(crate) use core::sync::atomic::{AtomicBool, Ordering};

// =============================================================================
// Flags — abstracts Arc<[AtomicBool]> vs Arc<Vec<AtomicBool>> under loom
// =============================================================================

#[cfg(not(loom))]
#[derive(Clone)]
pub(crate) struct Flags(std::sync::Arc<[AtomicBool]>);

#[cfg(not(loom))]
impl Flags {
    pub(crate) fn new(count: usize) -> Self {
        let vec: Vec<AtomicBool> = (0..count).map(|_| AtomicBool::new(false)).collect();
        Self(vec.into())
    }

    #[inline(always)]
    pub(crate) fn len(&self) -> usize {
        self.0.len()
    }

    #[inline(always)]
    pub(crate) fn get(&self, idx: usize) -> &AtomicBool {
        &self.0[idx]
    }
}

#[cfg(loom)]
#[derive(Clone)]
pub(crate) struct Flags(loom::sync::Arc<Vec<AtomicBool>>);

#[cfg(loom)]
impl Flags {
    pub(crate) fn new(count: usize) -> Self {
        let vec: Vec<AtomicBool> = (0..count).map(|_| AtomicBool::new(false)).collect();
        Self(loom::sync::Arc::new(vec))
    }

    #[inline(always)]
    pub(crate) fn len(&self) -> usize {
        self.0.len()
    }

    #[inline(always)]
    pub(crate) fn get(&self, idx: usize) -> &AtomicBool {
        &self.0[idx]
    }
}
