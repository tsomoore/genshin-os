// Synchronization Primitives Module
//
// 曾国藩曰：
// "凡事皆有度，过犹不及。"
// 进程同步当有度，不可过度竞争，亦不可过度等待。

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicU64, AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

/// Semaphore identifier
pub type SemaphoreId = u64;

/// Mutex lock identifier
pub type LockId = u64;

/// Semaphore for process synchronization
///
/// 曾国藩曰：
/// "守门者，计数出入者也。"
/// 信号量计数资源，管理并发访问。
#[derive(Debug)]
pub struct Semaphore {
    /// Unique semaphore ID
    pub id: SemaphoreId,

    /// Current semaphore value
    value: AtomicU32,

    /// Initial value (for reset)
    initial_value: u32,

    /// Maximum value (to prevent overflow)
    max_value: u32,

    /// Owner process ID
    pub owner_pid: crate::messaging::Pid,

    /// Number of waiting processes
    wait_count: AtomicU32,

    /// Whether semaphore is valid (not destroyed)
    valid: AtomicBool,
}

impl Semaphore {
    /// Create a new semaphore
    pub fn new(id: SemaphoreId, owner_pid: crate::messaging::Pid, initial_value: u32) -> Self {
        Self {
            id,
            value: AtomicU32::new(initial_value),
            initial_value,
            max_value: u32::MAX,
            owner_pid,
            wait_count: AtomicU32::new(0),
            valid: AtomicBool::new(true),
        }
    }

    /// Create a bounded semaphore
    pub fn with_max(id: SemaphoreId, owner_pid: crate::messaging::Pid, initial_value: u32, max_value: u32) -> Self {
        Self {
            id,
            value: AtomicU32::new(initial_value.min(max_value)),
            initial_value,
            max_value,
            owner_pid,
            wait_count: AtomicU32::new(0),
            valid: AtomicBool::new(true),
        }
    }

    /// Get current value
    pub fn value(&self) -> u32 {
        self.value.load(Ordering::Acquire)
    }

    /// Wait operation (P operation) - decrement value, block if zero
    /// Returns true if value was decremented, false if blocked
    pub fn wait(&self) -> SemaphoreResult {
        if !self.is_valid() {
            return SemaphoreResult::Invalid;
        }

        let mut current = self.value.load(Ordering::Acquire);

        loop {
            if current == 0 {
                // Would block
                self.wait_count.fetch_add(1, Ordering::AcqRel);
                return SemaphoreResult::WouldBlock;
            }

            // Try to decrement
            match self.value.compare_exchange_weak(
                current,
                current - 1,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return SemaphoreResult::Acquired,
                Err(new_current) => current = new_current,
            }
        }
    }

    /// Try to wait without blocking
    pub fn try_wait(&self) -> SemaphoreResult {
        if !self.is_valid() {
            return SemaphoreResult::Invalid;
        }

        let mut current = self.value.load(Ordering::Acquire);

        loop {
            if current == 0 {
                return SemaphoreResult::WouldBlock;
            }

            match self.value.compare_exchange_weak(
                current,
                current - 1,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return SemaphoreResult::Acquired,
                Err(new_current) => current = new_current,
            }
        }
    }

    /// Signal operation (V operation) - increment value
    pub fn signal(&self) -> SemaphoreResult {
        if !self.is_valid() {
            return SemaphoreResult::Invalid;
        }

        let mut current = self.value.load(Ordering::Acquire);

        loop {
            let new_value = current.saturating_add(1);
            if new_value > self.max_value {
                return SemaphoreResult::Overflow;
            }

            match self.value.compare_exchange_weak(
                current,
                new_value,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => {
                    // Decrease wait count if anyone was waiting
                    self.wait_count.fetch_sub(1, Ordering::AcqRel);
                    return SemaphoreResult::Released;
                }
                Err(new_current) => current = new_current,
            }
        }
    }

    /// Get the number of waiting processes
    pub fn wait_count(&self) -> u32 {
        self.wait_count.load(Ordering::Acquire)
    }

    /// Reset to initial value
    pub fn reset(&self) -> SemaphoreResult {
        if !self.is_valid() {
            return SemaphoreResult::Invalid;
        }

        self.value.store(self.initial_value, Ordering::Release);
        SemaphoreResult::Reset
    }

    /// Invalidate the semaphore (marks as destroyed)
    pub fn invalidate(&self) {
        self.valid.store(false, Ordering::Release);
    }

    /// Check if semaphore is still valid
    pub fn is_valid(&self) -> bool {
        self.valid.load(Ordering::Acquire)
    }
}

/// Result of a semaphore operation
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SemaphoreResult {
    /// Successfully acquired (wait succeeded)
    Acquired,

    /// Successfully released (signal succeeded)
    Released,

    /// Operation would block (for non-blocking operations)
    WouldBlock,

    /// Semaphore is invalid/destroyed
    Invalid,

    /// Value would overflow
    Overflow,

    /// Semaphore was reset
    Reset,
}

/// Mutex lock for mutual exclusion
///
/// 曾国藩曰：
/// "一门锁钥，当归一人掌管。"
/// 互斥锁确保同时只有一个进程访问资源。
#[derive(Debug)]
pub struct MutexLock {
    /// Unique lock ID
    pub id: LockId,

    /// Current owner (None = unlocked)
    owner: AtomicU64, // Using u64 for Pid

    /// Lock count (for recursive locks)
    count: AtomicU32,

    /// Creator process ID
    pub creator_pid: crate::messaging::Pid,

    /// Whether this is a recursive lock
    recursive: bool,

    /// Number of waiting processes
    wait_count: AtomicU32,

    /// Whether lock is valid
    valid: AtomicBool,
}

impl MutexLock {
    /// Create a new mutex lock
    pub fn new(id: LockId, creator_pid: crate::messaging::Pid, recursive: bool) -> Self {
        Self {
            id,
            owner: AtomicU64::new(u64::MAX), // u64::MAX represents unlocked
            count: AtomicU32::new(0),
            creator_pid,
            recursive,
            wait_count: AtomicU32::new(0),
            valid: AtomicBool::new(true),
        }
    }

    /// Get current owner (None if unlocked)
    pub fn owner(&self) -> Option<crate::messaging::Pid> {
        let owner = self.owner.load(Ordering::Acquire);
        if owner == u64::MAX {
            None
        } else {
            Some(owner as crate::messaging::Pid)
        }
    }

    /// Set owner directly (for TOCTOU transfer)
    pub fn set_owner(&self, pid: crate::messaging::Pid) {
        self.owner.store(pid as u64, Ordering::Release);
    }

    /// Check if lock is held
    pub fn is_locked(&self) -> bool {
        self.owner.load(Ordering::Acquire) != u64::MAX
    }

    /// Try to acquire the lock
    pub fn try_acquire(&self, pid: crate::messaging::Pid) -> MutexResult {
        if !self.is_valid() {
            return MutexResult::Invalid;
        }

        let current_owner = self.owner.load(Ordering::Acquire);

        // Already unlocked
        if current_owner == u64::MAX {
            let _ = self.owner.compare_exchange(
                u64::MAX,
                pid as u64,
                Ordering::AcqRel,
                Ordering::Acquire,
            );
            self.count.store(1, Ordering::Release);
            return MutexResult::Acquired;
        }

        // Owned by this process
        if current_owner == pid as u64 {
            if self.recursive {
                self.count.fetch_add(1, Ordering::AcqRel);
                return MutexResult::Acquired;
            }
            return MutexResult::Deadlock;
        }

        // Owned by another process
        MutexResult::WouldBlock
    }

    /// Acquire the lock (would block if not available)
    pub fn acquire(&self, pid: crate::messaging::Pid) -> MutexResult {
        if !self.is_valid() {
            return MutexResult::Invalid;
        }

        match self.try_acquire(pid) {
            MutexResult::Acquired => MutexResult::Acquired,
            MutexResult::WouldBlock => {
                self.wait_count.fetch_add(1, Ordering::AcqRel);
                MutexResult::WouldBlock
            }
            other => other,
        }
    }

    /// Release the lock
    pub fn release(&self, pid: crate::messaging::Pid) -> MutexResult {
        if !self.is_valid() {
            return MutexResult::Invalid;
        }

        let current_owner = self.owner.load(Ordering::Acquire);

        if current_owner != pid as u64 {
            return MutexResult::NotOwner;
        }

        let count = self.count.fetch_sub(1, Ordering::AcqRel);

        if count == 1 {
            // Last reference, unlock
            self.owner.store(u64::MAX, Ordering::Release);
            if self.wait_count.load(Ordering::Acquire) > 0 {
                self.wait_count.fetch_sub(1, Ordering::AcqRel);
            }
        }

        MutexResult::Released
    }

    /// Get the number of waiting processes
    pub fn wait_count(&self) -> u32 {
        self.wait_count.load(Ordering::Acquire)
    }

    /// Get the recursion count
    pub fn count(&self) -> u32 {
        self.count.load(Ordering::Acquire)
    }

    /// Invalidate the lock
    pub fn invalidate(&self) {
        self.valid.store(false, Ordering::Release);
    }

    /// Check if lock is still valid
    pub fn is_valid(&self) -> bool {
        self.valid.load(Ordering::Acquire)
    }
}

/// Result of a mutex operation
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MutexResult {
    /// Successfully acquired
    Acquired,

    /// Successfully released
    Released,

    /// Would block (lock held by another)
    WouldBlock,

    /// Not the owner (can't release)
    NotOwner,

    /// Potential deadlock (recursive lock would deadlock)
    Deadlock,

    /// Lock is invalid/destroyed
    Invalid,
}

/// Synchronization Manager - Manages all sync primitives
///
/// 曾国藩曰：
/// "总管百官，当知其职守。"
/// 同步管理器统筹所有信号量与互斥锁。
#[derive(Debug)]
pub struct SyncManager {
    /// Semaphores (id -> semaphore)
    semaphores: HashMap<SemaphoreId, Arc<Semaphore>>,

    /// Mutex locks (id -> lock)
    mutexes: HashMap<LockId, Arc<MutexLock>>,

    /// Next semaphore ID
    next_sem_id: SemaphoreId,

    /// Next mutex ID
    next_mutex_id: LockId,
}

impl SyncManager {
    /// Create a new sync manager
    pub fn new() -> Self {
        let mut sm = Self {
            semaphores: HashMap::new(),
            mutexes: HashMap::new(),
            next_sem_id: 0,
            next_mutex_id: 0,
        };
        // Pre-create global semaphore 0 (binary mutex) for rwlock/dual demos
        sm.create_semaphore(0, 1);
        sm.create_mutex(0, false); // mutex 0 for syncdemo
        sm
    }

    // ========== Semaphore Management ==========

    /// Create a new semaphore
    pub fn create_semaphore(&mut self, owner_pid: crate::messaging::Pid, initial_value: u32) -> SemaphoreId {
        let id = self.next_sem_id;
        self.next_sem_id += 1;

        let sem = Arc::new(Semaphore::new(id, owner_pid, initial_value));
        self.semaphores.insert(id, sem);
        id
    }

    /// Create a bounded semaphore
    pub fn create_bounded_semaphore(
        &mut self,
        owner_pid: crate::messaging::Pid,
        initial_value: u32,
        max_value: u32,
    ) -> SemaphoreId {
        let id = self.next_sem_id;
        self.next_sem_id += 1;

        let sem = Arc::new(Semaphore::with_max(id, owner_pid, initial_value, max_value));
        self.semaphores.insert(id, sem);
        id
    }

    /// Get a semaphore
    pub fn get_semaphore(&self, id: SemaphoreId) -> Option<Arc<Semaphore>> {
        self.semaphores.get(&id).cloned()
    }

    /// Destroy a semaphore
    pub fn destroy_semaphore(&mut self, id: SemaphoreId) -> bool {
        if let Some(sem) = self.semaphores.remove(&id) {
            sem.invalidate();
            true
        } else {
            false
        }
    }

    // ========== Mutex Management ==========

    /// Create a new mutex lock
    pub fn create_mutex(&mut self, creator_pid: crate::messaging::Pid, recursive: bool) -> LockId {
        let id = self.next_mutex_id;
        self.next_mutex_id += 1;

        let mutex = Arc::new(MutexLock::new(id, creator_pid, recursive));
        self.mutexes.insert(id, mutex);
        id
    }

    /// Get a mutex
    pub fn get_mutex(&self, id: LockId) -> Option<Arc<MutexLock>> {
        self.mutexes.get(&id).cloned()
    }

    /// Destroy a mutex
    pub fn destroy_mutex(&mut self, id: LockId) -> bool {
        if let Some(mutex) = self.mutexes.remove(&id) {
            mutex.invalidate();
            true
        } else {
            false
        }
    }

    /// Clean up invalid primitives
    pub fn cleanup(&mut self) -> (usize, usize) {
        let mut removed_sem = 0;
        let mut removed_mutex = 0;

        self.semaphores.retain(|_, sem| {
            if sem.is_valid() {
                true
            } else {
                removed_sem += 1;
                false
            }
        });

        self.mutexes.retain(|_, mutex| {
            if mutex.is_valid() {
                true
            } else {
                removed_mutex += 1;
                false
            }
        });

        (removed_sem, removed_mutex)
    }
}

impl Default for SyncManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_semaphore_basic() {
        let sem = Semaphore::new(1, 100, 2);

        assert_eq!(sem.value(), 2);

        // Wait twice
        assert_eq!(sem.wait(), SemaphoreResult::Acquired);
        assert_eq!(sem.value(), 1);

        assert_eq!(sem.wait(), SemaphoreResult::Acquired);
        assert_eq!(sem.value(), 0);

        // Third wait would block
        assert_eq!(sem.wait(), SemaphoreResult::WouldBlock);

        // Signal twice
        assert_eq!(sem.signal(), SemaphoreResult::Released);
        assert_eq!(sem.value(), 1);

        assert_eq!(sem.signal(), SemaphoreResult::Released);
        assert_eq!(sem.value(), 2);
    }

    #[test]
    fn test_semaphore_try_wait() {
        let sem = Semaphore::new(1, 100, 1);

        assert_eq!(sem.try_wait(), SemaphoreResult::Acquired);
        assert_eq!(sem.try_wait(), SemaphoreResult::WouldBlock);

        // Signal and try again
        assert_eq!(sem.signal(), SemaphoreResult::Released);
        assert_eq!(sem.try_wait(), SemaphoreResult::Acquired);
    }

    #[test]
    fn test_semaphore_reset() {
        let sem = Semaphore::new(1, 100, 5);

        // Wait a few times
        sem.wait();
        sem.wait();
        sem.wait();

        assert_eq!(sem.value(), 2);

        // Reset
        assert_eq!(sem.reset(), SemaphoreResult::Reset);
        assert_eq!(sem.value(), 5);
    }

    #[test]
    fn test_bounded_semaphore() {
        let sem = Semaphore::with_max(1, 100, 1, 3);

        assert_eq!(sem.value(), 1);

        // Signal up to max
        assert_eq!(sem.signal(), SemaphoreResult::Released);
        assert_eq!(sem.value(), 2);

        assert_eq!(sem.signal(), SemaphoreResult::Released);
        assert_eq!(sem.value(), 3);

        // Would overflow
        assert_eq!(sem.signal(), SemaphoreResult::Overflow);
        assert_eq!(sem.value(), 3);
    }

    #[test]
    fn test_semaphore_invalid() {
        let sem = Semaphore::new(1, 100, 1);

        sem.invalidate();

        assert!(!sem.is_valid());
        assert_eq!(sem.wait(), SemaphoreResult::Invalid);
        assert_eq!(sem.signal(), SemaphoreResult::Invalid);
    }

    #[test]
    fn test_mutex_basic() {
        let mutex = MutexLock::new(1, 100, false);

        assert!(!mutex.is_locked());
        assert!(mutex.owner().is_none());

        // Acquire
        assert_eq!(mutex.try_acquire(100), MutexResult::Acquired);
        assert!(mutex.is_locked());
        assert_eq!(mutex.owner(), Some(100));
        assert_eq!(mutex.count(), 1);

        // Try to acquire again (non-recursive)
        assert_eq!(mutex.try_acquire(100), MutexResult::Deadlock);

        // Another process can't acquire
        assert_eq!(mutex.try_acquire(200), MutexResult::WouldBlock);

        // Release
        assert_eq!(mutex.release(100), MutexResult::Released);
        assert!(!mutex.is_locked());
    }

    #[test]
    fn test_recursive_mutex() {
        let mutex = MutexLock::new(1, 100, true);

        // Acquire multiple times
        assert_eq!(mutex.try_acquire(100), MutexResult::Acquired);
        assert_eq!(mutex.count(), 1);

        assert_eq!(mutex.try_acquire(100), MutexResult::Acquired);
        assert_eq!(mutex.count(), 2);

        assert_eq!(mutex.try_acquire(100), MutexResult::Acquired);
        assert_eq!(mutex.count(), 3);

        // Must release the same number of times
        assert_eq!(mutex.release(100), MutexResult::Released);
        assert_eq!(mutex.count(), 2);
        assert!(mutex.is_locked());

        assert_eq!(mutex.release(100), MutexResult::Released);
        assert_eq!(mutex.count(), 1);
        assert!(mutex.is_locked());

        assert_eq!(mutex.release(100), MutexResult::Released);
        assert_eq!(mutex.count(), 0);
        assert!(!mutex.is_locked());
    }

    #[test]
    fn test_mutex_not_owner() {
        let mutex = MutexLock::new(1, 100, false);

        // Process 100 acquires
        assert_eq!(mutex.try_acquire(100), MutexResult::Acquired);

        // Process 200 tries to release
        assert_eq!(mutex.release(200), MutexResult::NotOwner);
    }

    #[test]
    fn test_mutex_invalid() {
        let mutex = MutexLock::new(1, 100, false);

        mutex.invalidate();
        assert!(!mutex.is_valid());

        assert_eq!(mutex.try_acquire(100), MutexResult::Invalid);
        assert_eq!(mutex.release(100), MutexResult::Invalid);
    }

    #[test]
    fn test_sync_manager() {
        let mut manager = SyncManager::new();

        // Create semaphore
        let sem_id = manager.create_semaphore(100, 5);
        assert_eq!(sem_id, 1);

        let sem = manager.get_semaphore(sem_id);
        assert!(sem.is_some());
        assert_eq!(sem.unwrap().value(), 5);

        // Create mutex
        let mutex_id = manager.create_mutex(100, false);
        assert_eq!(mutex_id, 1);

        let mutex = manager.get_mutex(mutex_id);
        assert!(mutex.is_some());

        // Destroy
        assert!(manager.destroy_semaphore(sem_id));
        assert!(manager.get_semaphore(sem_id).is_none());
    }

    #[test]
    fn test_wait_count() {
        let sem = Semaphore::new(1, 100, 0);

        // All waits would block
        assert_eq!(sem.wait(), SemaphoreResult::WouldBlock);
        assert_eq!(sem.wait(), SemaphoreResult::WouldBlock);
        assert_eq!(sem.wait_count(), 2);

        // Signal unblocks one
        assert_eq!(sem.signal(), SemaphoreResult::Released);
        assert_eq!(sem.wait_count(), 1);
    }

    #[test]
    fn test_mutex_wait_count() {
        let mutex = MutexLock::new(1, 100, false);

        // Lock it
        let _ = mutex.try_acquire(100);

        // Try to acquire (would block)
        mutex.acquire(200);
        mutex.acquire(300);

        assert_eq!(mutex.wait_count(), 2);
    }

    #[test]
    fn test_semaphore_wait_acquires_then_blocks() {
        let sem = Semaphore::new(0, 0, 1); // id=0, initial=1
        assert_eq!(sem.wait(), SemaphoreResult::Acquired); // count 1→0
        assert_eq!(sem.wait(), SemaphoreResult::WouldBlock); // count=0
    }

    #[test]
    fn test_semaphore_signal_increments() {
        let sem = Semaphore::new(0, 0, 1);
        sem.wait(); // count 1→0
        sem.signal(); // count 0→1
        assert_eq!(sem.wait(), SemaphoreResult::Acquired); // count 1→0 again
    }

    #[test]
    fn test_semaphore_binary_mutex_behavior() {
        let sem = Semaphore::new(0, 0, 1);
        // Only one acquirer at a time
        assert_eq!(sem.wait(), SemaphoreResult::Acquired);
        assert_eq!(sem.wait(), SemaphoreResult::WouldBlock);
        sem.signal();
        assert_eq!(sem.wait(), SemaphoreResult::Acquired);
    }
}
