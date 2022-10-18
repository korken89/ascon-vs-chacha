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
    /// Only updated on release of exclusive access.
    idx_out: FairSharePlace,
    /// This holds the new idx_out until end of exclusive access
    idx_out_next: FairSharePlace,
    queue_head: Option<NonNull<IntrusiveWakerNode>>,
    queue_tail: Option<NonNull<IntrusiveWakerNode>>,
    // queue: Queue<(Waker, FairSharePlace), 10>, // TODO: Should be an intrusive linked list probably...
}

unsafe impl Send for FairShareManagement {}

#[derive(Clone)]
struct IntrusiveWakerNode {
    waker: Waker,
    place: FairSharePlace,
    next: Option<NonNull<IntrusiveWakerNode>>,
}

impl IntrusiveWakerNode {
    fn new(waker: Waker) -> Self {
        IntrusiveWakerNode {
            waker,
            place: FairSharePlace(0),
            next: None,
        }
    }
}

impl FairShareManagement {
    // SAFETY: The pointer must live for the duration of its existence in the queue.
    unsafe fn enqueue(&mut self, mut node: NonNull<IntrusiveWakerNode>) {
        let current = self.idx_in;
        self.idx_in = FairSharePlace(current.0.wrapping_add(1));
        node.as_mut().place = current;

        defmt::debug!(
            "Enqueueing waker at place {}, node place = {}",
            current.0,
            node.as_ref().place.0
        );

        if let Some(mut tail) = self.queue_tail {
            let tail = unsafe { tail.as_mut() };
            tail.next = Some(node);
            self.queue_tail = tail.next;
        } else {
            self.queue_head = Some(node);
            self.queue_tail = self.queue_head;
        }

        defmt::debug!(
            "after enqueue, node = {:x}, head = {:x}, tail = {:x}",
            node.as_ptr() as u32,
            self.queue_head.map(|v| v.as_ptr() as u32),
            self.queue_tail.map(|v| v.as_ptr() as u32)
        );

        // if let Err(_) = self.queue.enqueue((waker, current)) {
        //     panic!("Oh no, more uses than space in the queue");
        // }

        // current
    }

    fn dequeue_head(&mut self) {
        if let Some(head) = self.queue_head {
            let node = unsafe { head.as_ref() }.clone();
            self.queue_head = node.next;
            self.idx_out_next = node.place;

            defmt::debug!("Dequeueing node at place {}", node.place.0);
        } else {
            // If the queue is empty make sure the indexes are aligned
            self.idx_out_next = self.idx_in;
            defmt::debug!("Dequeueing node with empty queue");
        }

        defmt::debug!(
            "after dequeue, head = {:x}, tail = {:x}",
            self.queue_head.map(|v| v.as_ptr() as u32),
            self.queue_tail.map(|v| v.as_ptr() as u32)
        );

        // if let Some((waker, current)) = self.queue.dequeue() {
        //     self.idx_out = current;
        //     defmt::debug!("Dequeueing waker at place {}", current.0);

        //     Some(waker)
        // } else {
        //     // If the queue is empty make sure the indexes are aligned
        //     self.idx_out = self.idx_in;
        //     defmt::debug!("Dequeueing waker with empty queue");

        //     None
        // }
    }

    fn try_direct_access(&mut self) -> bool {
        if self.queue_head.is_none() && self.idx_in == self.idx_out {
            // Update current counters to not get races
            let current = self.idx_in;
            self.idx_in = FairSharePlace(current.0.wrapping_add(1));
            self.idx_out_next = self.idx_in;

            defmt::debug!(
                "Direct access granted, current = {}, idx in = {}",
                current.0,
                self.idx_in.0
            );

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
                idx_out_next: FairSharePlace(0),
                queue_head: None,
                queue_tail: None,
                // queue: Queue::new(),
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
            node: None,
        }
    }
}

/// Access future.
pub struct FairShareAccessFuture<'a, T> {
    fs: &'a FairShare<T>,
    node: Option<IntrusiveWakerNode>,
}

impl<'a, T> Future for FairShareAccessFuture<'a, T> {
    type Output = FairShareExclusiveAccess<'a, T>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        critical_section::with(|mut token| {
            let fs = self.fs.get_management(&mut token);

            if let Some(node) = &self.node {
                defmt::debug!(
                    "Poll idx out = {}, node place = {}",
                    fs.idx_out.0,
                    node.place.0
                );

                if fs.idx_out == node.place {
                    // Our turn, remove us from the queue
                    fs.dequeue_head();

                    defmt::debug!("{}: Exclusive access granted", node.place.0);
                    Poll::Ready(FairShareExclusiveAccess { fs: self.fs })
                } else {
                    // Continue waiting
                    defmt::debug!("{}: Waiting for exclusive access", node.place.0);
                    Poll::Pending
                }
            } else {
                // Check if the queue is empty, then we don't need to wait
                if fs.try_direct_access() {
                    Poll::Ready(FairShareExclusiveAccess { fs: self.fs })
                } else {
                    // We are not in the queue yet, enqueue our waker
                    let node = self
                        .node
                        .insert(IntrusiveWakerNode::new(cx.waker().clone()))
                        .into();

                    // SAFETY: The node now has a sable address as we have pinned self and the
                    // only way to invalidate the pointer is to finish this future, which first
                    // will remove the pointer from the queue via `dequeue_head`.
                    unsafe { fs.enqueue(node) };
                    defmt::debug!("Waiting for exclusive access");
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

            // Move the out index
            fs.idx_out = fs.idx_out_next;

            defmt::debug!(
                "Returning exclusive access, idx in = {}, idx out = {}, idx out next = {}",
                fs.idx_in.0,
                fs.idx_out.0,
                fs.idx_out_next.0
            );

            // Get the next waker in line
            fs.queue_head
                .map(|head| unsafe { head.as_ref() }.waker.clone())
        });

        // Run the waker outside of the critical section to minimize its size
        if let Some(waker) = waker {
            waker.wake();
        }
    }
}
