//! Sequence allocator used for outbound `PublishLocation` messages.
//!
//! Described in `08` §6.2 and in `12` §8: the device keeps a monotonically
//! increasing local sequence number; on `ServerHello` it raises its next value to
//! `max(local_next, sequence_floor + 1)` so the server can detect duplicates
//! after a reconnect.

#![forbid(unsafe_code)]

/// A monotonic allocator for publication sequences.
#[derive(Debug, Clone, Copy)]
pub struct SequenceAllocator {
    /// Next value to assign. `Option` so the allocator can be **cold**: the
    /// first call returns the floor and the first publish uses floor+1.
    pub next: u64,
    /// Highest floor applied from a server hello.
    pub floor_applied: u64,
}

impl SequenceAllocator {
    pub const fn new(start: u64) -> Self {
        Self {
            next: start,
            floor_applied: start,
        }
    }

    /// Returns the next sequence and advances by one.
    pub fn allocate(&mut self) -> u64 {
        let value = self.next;
        self.next = self.next.saturating_add(1);
        value
    }

    /// Applies the server's `sequence_floor`.
    pub fn apply_floor(&mut self, floor: u64) -> FloorAction {
        let required = floor.saturating_add(1);
        if required > self.next {
            self.floor_applied = floor;
            self.next = required;
            FloorAction::Raised {
                previous: self.floor_applied.saturating_add(1),
                new: required,
            }
        } else {
            FloorAction::Unchanged {
                floor,
                next: self.next,
            }
        }
    }

    #[must_use]
    pub const fn is_cold(&self) -> bool {
        self.next == 0 && self.floor_applied == 0
    }
}

impl Default for SequenceAllocator {
    fn default() -> Self {
        Self::new(1)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FloorAction {
    Raised { previous: u64, new: u64 },
    Unchanged { floor: u64, next: u64 },
}
