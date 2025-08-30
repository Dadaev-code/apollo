//! Lock-free ring buffer for frame pipeline

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use crossbeam::utils::CachePadded;
use ringbuf::{HeapRb, Rb};

use crate::Frame;

/// Lock-free SPSC ring buffer optimized for frame data
pub struct FrameRingBuffer {
    /// Ring buffer for frame references (not the data itself)
    ring: HeapRb<Option<Arc<Frame>>>,

    /// Statistics
    stats: CachePadded<Stats>,
}

#[derive(Default)]
struct Stats {
    frames_written: AtomicUsize,
    frames_read: AtomicUsize,
    frames_dropped: AtomicUsize,
}

impl FrameRingBuffer {
    pub fn new(capacity: usize) -> Self {
        Self {
            ring: HeapRb::new(capacity),
            stats: CachePadded::new(Stats::default()),
        }
    }

    /// Producer: Push frame to ring buffer
    pub fn push(&mut self, frame: Frame) -> bool {
        let arc_frame = Arc::new(frame);

        if self.ring.is_full() {
            // Drop oldest frame
            self.ring.pop();
            self.stats.frames_dropped.fetch_add(1, Ordering::Relaxed);
        }

        self.ring.push_overwrite(Some(arc_frame));
        self.stats.frames_written.fetch_add(1, Ordering::Relaxed);
        true
    }

    /// Consumer: Pop frame from ring buffer
    pub fn pop(&mut self) -> Option<Arc<Frame>> {
        if let Some(frame) = self.ring.pop()? {
            self.stats.frames_read.fetch_add(1, Ordering::Relaxed);
            frame
        } else {
            None
        }
    }

    pub fn len(&self) -> usize {
        self.ring.len()
    }

    pub fn stats(&self) -> (usize, usize, usize) {
        (
            self.stats.frames_written.load(Ordering::Relaxed),
            self.stats.frames_read.load(Ordering::Relaxed),
            self.stats.frames_dropped.load(Ordering::Relaxed),
        )
    }
}
