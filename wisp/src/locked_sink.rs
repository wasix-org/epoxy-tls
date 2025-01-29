//! unfair async mutex that doesn't have guards by default

use std::{
	cell::UnsafeCell,
	future::poll_fn,
	marker::PhantomData,
	ops::{Deref, DerefMut},
	pin::Pin,
	sync::{
		atomic::{AtomicBool, Ordering},
		Arc, Mutex, MutexGuard,
	},
	task::{Context, Poll, Waker},
};

use futures::Sink;
use slab::Slab;

use crate::ws::{Payload, TransportWrite};

// it would be nice to have type_alias_bounds but oh well
#[expect(type_alias_bounds)]
pub(crate) type LockedWebSocketWrite<I: TransportWrite> = LockedSink<I, Payload>;
#[expect(type_alias_bounds)]
pub type LockedWebSocketWriteGuard<I: TransportWrite> = LockedSinkGuard<I, Payload>;

pub(crate) enum Waiter {
	Sleeping(Waker),
	Woken,
}

impl Waiter {
	pub fn new(cx: &mut Context<'_>) -> Self {
		Self::Sleeping(cx.waker().clone())
	}

	pub fn register(&mut self, cx: &mut Context<'_>) {
		match self {
			Self::Sleeping(x) => x.clone_from(cx.waker()),
			Self::Woken => *self = Self::Sleeping(cx.waker().clone()),
		}
	}

	pub fn wake(&mut self) -> Option<Waker> {
		match std::mem::replace(self, Self::Woken) {
			Self::Sleeping(x) => Some(x),
			Self::Woken => None,
		}
	}
}

struct WakerList {
	inner: Slab<Waiter>,
}

impl WakerList {
	pub fn new() -> Self {
		Self { inner: Slab::new() }
	}

	pub fn add(&mut self, cx: &mut Context<'_>) -> usize {
		self.inner.insert(Waiter::new(cx))
	}

	pub fn update(&mut self, key: usize, cx: &mut Context<'_>) {
		self.inner
			.get_mut(key)
			.expect("task should never have invalid key")
			.register(cx);
	}

	pub fn remove(&mut self, key: usize) {
		self.inner.remove(key);
	}

	pub fn get_next(&mut self) -> Option<Waker> {
		self.inner.iter_mut().find_map(|x| x.1.wake())
	}
}

enum LockStatus {
	/// was locked, you are now in the list
	Joined(usize),
	/// was locked, you were already in the list
	Waiting,
	/// was unlocked, lock is yours now
	Unlocked,
}

struct SinkState<S: Sink<I>, I> {
	sink: UnsafeCell<S>,
	locked: AtomicBool,
	waiters: Mutex<WakerList>,

	phantom: PhantomData<I>,
}

unsafe impl<S: Sink<I> + Send, I> Send for SinkState<S, I> {}
unsafe impl<S: Sink<I>, I> Sync for SinkState<S, I> {}

impl<S: Sink<I>, I> SinkState<S, I> {
	pub fn new(sink: S) -> Self {
		Self {
			sink: UnsafeCell::new(sink),
			locked: AtomicBool::new(false),
			waiters: Mutex::new(WakerList::new()),

			phantom: PhantomData,
		}
	}

	fn lock_waiters(&self) -> MutexGuard<'_, WakerList> {
		self.waiters.lock().expect("waiters mutex was poisoned")
	}

	/// caller must make sure they are the ones locking the sink
	#[expect(clippy::mut_from_ref)]
	pub unsafe fn get_unpin(&self) -> &mut S {
		// SAFETY: we are locked
		unsafe { &mut *self.sink.get() }
	}

	/// caller must make sure they are the ones locking the sink
	pub unsafe fn get(&self) -> Pin<&mut S> {
		// SAFETY: we are locked
		let inner = unsafe { self.get_unpin() };
		// SAFETY: we never touch the UnsafeCell
		unsafe { Pin::new_unchecked(inner) }
	}

	pub fn lock(&self, key: Option<usize>, cx: &mut Context<'_>) -> LockStatus {
		let old_state = self.locked.swap(true, Ordering::AcqRel);
		match (key, old_state) {
			(Some(key), true) => {
				self.lock_waiters().update(key, cx);
				LockStatus::Waiting
			}
			(None, true) => {
				let pos = self.lock_waiters().add(cx);
				LockStatus::Joined(pos)
			}
			(_, false) => LockStatus::Unlocked,
		}
	}

	pub fn unlock(&self) {
		let mut locked = self.lock_waiters();
		self.locked.store(false, Ordering::Release);
		if let Some(next) = locked.get_next() {
			drop(locked);

			next.wake();
		}
	}

	pub fn remove(&self, key: usize) {
		self.lock_waiters().remove(key);
	}
}

pub(crate) struct LockedSink<S: Sink<I>, I> {
	inner: Arc<SinkState<S, I>>,

	pos: Option<usize>,
	locked: bool,
}

impl<S: Sink<I>, I> Clone for LockedSink<S, I> {
	fn clone(&self) -> Self {
		Self {
			inner: self.inner.clone(),

			pos: None,
			locked: false,
		}
	}
}

impl<S: Sink<I>, I> Drop for LockedSink<S, I> {
	fn drop(&mut self) {
		self.unlock();
	}
}

impl<S: Sink<I>, I> LockedSink<S, I> {
	pub fn new(sink: S) -> Self {
		Self {
			inner: Arc::new(SinkState::new(sink)),

			pos: None,
			locked: false,
		}
	}

	pub fn poll_lock(&mut self, cx: &mut Context<'_>) -> Poll<()> {
		if self.locked {
			Poll::Ready(())
		} else {
			match self.inner.lock(self.pos, cx) {
				LockStatus::Joined(pos) => {
					self.pos = Some(pos);

					// make sure we haven't raced an unlock
					if matches!(self.inner.lock(self.pos, cx), LockStatus::Unlocked) {
						if let Some(pos) = self.pos.take() {
							self.inner.remove(pos);
						}
						self.locked = true;
						return Poll::Ready(());
					}

					Poll::Pending
				}
				LockStatus::Waiting => {
					// make sure we haven't raced an unlock
					if matches!(self.inner.lock(self.pos, cx), LockStatus::Unlocked) {
						if let Some(pos) = self.pos.take() {
							self.inner.remove(pos);
						}
						self.locked = true;
						return Poll::Ready(());
					}

					Poll::Pending
				}
				LockStatus::Unlocked => {
					if let Some(pos) = self.pos.take() {
						self.inner.remove(pos);
					}
					self.locked = true;
					Poll::Ready(())
				}
			}
		}
	}
	pub async fn lock(&mut self) {
		poll_fn(|cx| self.poll_lock(cx)).await;
	}

	pub fn unlock(&mut self) {
		if self.locked {
			self.locked = false;
			self.inner.unlock();
		}
	}

	pub fn get(&self) -> Pin<&mut S> {
		debug_assert!(self.locked);
		// SAFETY: we are locked
		unsafe { self.inner.get() }
	}
	pub fn get_handle(&mut self) -> LockedSinkHandle<S, I> {
		debug_assert!(self.locked);
		self.locked = false;

		LockedSinkHandle {
			inner: self.inner.clone(),
		}
	}
	pub fn get_guard(&mut self) -> LockedSinkGuard<S, I> {
		debug_assert!(self.locked);
		self.locked = false;

		LockedSinkGuard {
			inner: self.inner.clone(),
		}
	}
}

// always locked sink "guard" of lockedsink
pub(crate) struct LockedSinkHandle<S: Sink<I>, I> {
	inner: Arc<SinkState<S, I>>,
}

impl<S: Sink<I>, I> Sink<I> for LockedSinkHandle<S, I> {
	type Error = S::Error;

	fn poll_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
		unsafe { self.inner.get() }.poll_ready(cx)
	}

	fn start_send(self: Pin<&mut Self>, item: I) -> Result<(), Self::Error> {
		unsafe { self.inner.get() }.start_send(item)
	}

	fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
		unsafe { self.inner.get() }.poll_flush(cx)
	}

	fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
		unsafe { self.inner.get() }.poll_close(cx)
	}
}

impl<S: Sink<I>, I> Drop for LockedSinkHandle<S, I> {
	fn drop(&mut self) {
		self.inner.unlock();
	}
}

// always locked "guard" of lockedsink
pub struct LockedSinkGuard<S: Sink<I>, I> {
	inner: Arc<SinkState<S, I>>,
}

impl<S: Sink<I>, I> Deref for LockedSinkGuard<S, I> {
	type Target = S;

	fn deref(&self) -> &Self::Target {
		unsafe { &*self.inner.get_unpin() }
	}
}

impl<S: Sink<I> + Unpin, I> DerefMut for LockedSinkGuard<S, I> {
	fn deref_mut(&mut self) -> &mut Self::Target {
		unsafe { self.inner.get_unpin() }
	}
}

impl<S: Sink<I>, I> LockedSinkGuard<S, I> {
	pub fn deref_pin(&mut self) -> Pin<&mut S> {
		unsafe { self.inner.get() }
	}
}

impl<S: Sink<I>, I> Drop for LockedSinkGuard<S, I> {
	fn drop(&mut self) {
		self.inner.unlock();
	}
}
