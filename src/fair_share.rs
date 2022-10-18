use core::{
    cell::UnsafeCell,
    future::Future,
    ops::{Deref, DerefMut},
    pin::Pin,
    ptr::NonNull,
    task::{Context, Poll, Waker},
};
use critical_section::CriticalSection;
use heapless::spsc::Queue;

// Wrapper type of queue placement.
#[derive(Clone, Copy, PartialEq, PartialOrd, Eq, Ord)]
struct FairSharePlace(usize);

struct FairShareManagement {
    /// Only update on enqueue/try_direct_access.
    idx_in: FairSharePlace,
    /// Only updated on dequeue.
    idx_out: FairSharePlace,
    // queue_head: Option<NonNull<IntrusiveWakerNode>>,
    // queue_tail: Option<NonNull<IntrusiveWakerNode>>,
    queue: Queue<(Waker, FairSharePlace), 10>, // TODO: Should be an intrusive linked list probably...
}

struct IntrusiveWakerNode {
    waker: Waker,
    place: FairSharePlace,
    next: Option<NonNull<IntrusiveWakerNode>>,
}

impl FairShareManagement {
    fn enqueue(&mut self, waker: Waker) -> FairSharePlace {
        let current = self.idx_in;
        self.idx_in = FairSharePlace(current.0.wrapping_add(1));

        defmt::debug!("Enqueueing waker at place {}", current.0);

        if let Err(_) = self.queue.enqueue((waker, current)) {
            panic!("Oh no, more uses than space in the queue");
        }

        current
    }

    fn dequeue(&mut self) -> Option<Waker> {
        if let Some((waker, current)) = self.queue.dequeue() {
            self.idx_out = current;
            defmt::debug!("Dequeueing waker at place {}", current.0);

            Some(waker)
        } else {
            // If the queue is empty make sure the indexes are aligned
            self.idx_out = self.idx_in;
            defmt::debug!("Dequeueing waker with empty queue");

            None
        }
    }

    fn try_direct_access(&mut self) -> bool {
        if self.queue.is_empty() && self.idx_in == self.idx_out {
            // Update current counters to not get races
            let current = self.idx_in;
            self.idx_in = FairSharePlace(current.0.wrapping_add(1));

            defmt::debug!("Direct access granted");

            true
        } else {
            defmt::debug!("Direct access denied");

            false
        }
    }
}

/// Async fair sharing of an underlying value.
pub struct FairShare<T> {
    /// Holds the underying type, this can only safely be accessed from `FairShareExclusiveAccess`.
    storage: UnsafeCell<T>,
    /// Holds queue handling, this is guarded with critical section tokens.
    management: UnsafeCell<FairShareManagement>,
}

unsafe impl<T> Sync for FairShare<T> {}

impl<T> FairShare<T> {
    /// Create a new fair share, generally place this in static storage and pass around references.
    pub const fn new(val: T) -> Self {
        FairShare {
            storage: UnsafeCell::new(val),
            management: UnsafeCell::new(FairShareManagement {
                idx_in: FairSharePlace(0),
                idx_out: FairSharePlace(0),
                // queue_head: None,
                // queue_tail: None,
                queue: Queue::new(),
            }),
        }
    }

    fn get_management<'a>(&self, _token: &'a mut CriticalSection) -> &'a mut FairShareManagement {
        // Safety: Get the underlying storage if we are in a critical section
        unsafe { &mut *(self.management.get()) }
    }

    /// Request access, await the returned future to be woken when its available.
    pub fn access<'a>(&'a self) -> FairShareAccessFuture<'a, T> {
        FairShareAccessFuture {
            fs: self,
            place: None,
        }
    }
}

/// Access future.
pub struct FairShareAccessFuture<'a, T> {
    fs: &'a FairShare<T>,
    place: Option<FairSharePlace>,
}

impl<'a, T> Future for FairShareAccessFuture<'a, T> {
    type Output = FairShareExclusiveAccess<'a, T>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        critical_section::with(|mut token| {
            let fs = self.fs.get_management(&mut token);

            if let Some(place) = self.place {
                if fs.idx_out == place {
                    // Our turn
                    defmt::debug!("{}: Exclusive access granted", place.0);
                    Poll::Ready(FairShareExclusiveAccess { fs: self.fs })
                } else {
                    // Continue waiting
                    defmt::debug!("{}: Waiting for exclusive access", place.0);
                    Poll::Pending
                }
            } else {
                // Check if the queue is empty, then we don't need to wait
                if fs.try_direct_access() {
                    Poll::Ready(FairShareExclusiveAccess { fs: self.fs })
                } else {
                    // We are not in the queue yet, enqueue our waker
                    self.place = Some(fs.enqueue(cx.waker().clone()));
                    defmt::debug!("{}: Waiting for exclusive access", self.place.unwrap().0);
                    Poll::Pending
                }
            }
        })
    }
}

/// Excluseive access to the underlying storage until released or dropped.
pub struct FairShareExclusiveAccess<'a, T> {
    fs: &'a FairShare<T>,
}

impl<'a, T> Deref for FairShareExclusiveAccess<'a, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        // Safety: We can generate mulitple immutable references to the underlying type.
        // And if any mutable reference is generated we are protected via `&self`.
        unsafe { &*(self.fs.storage.get()) }
    }
}

impl<'a, T> DerefMut for FairShareExclusiveAccess<'a, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        // Safety: We can generate a single mutable references to the underlying type.
        // And if any immutable reference is generated we are protected via `&mut self`.
        unsafe { &mut *(self.fs.storage.get()) }
    }
}

impl<T> FairShareExclusiveAccess<'_, T> {
    /// Release exclusive access, equates to a drop.
    pub fn release(self) {
        // Run drop
    }
}

impl<T> Drop for FairShareExclusiveAccess<'_, T> {
    fn drop(&mut self) {
        let waker = critical_section::with(|mut token| {
            let fs = self.fs.get_management(&mut token);
            defmt::debug!("Returning exclusive access");
            fs.dequeue()
        });

        // Run the waker outside of the critical section to minimize its size
        if let Some(waker) = waker {
            waker.wake();
        }
    }
}
