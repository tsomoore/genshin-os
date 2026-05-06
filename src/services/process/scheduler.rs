// Process/Thread Scheduler
//
// 曾国藩曰：
// "治军之道，赏罚分明，进退有度。"
// 调度器决定进程之进退，当公平高效。

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use crate::messaging::{Pid, Tid, VirtAddr};

/// Scheduling policy
#[derive(Debug, Clone, Copy, PartialEq, Eq, )]
pub enum SchedulingPolicy {
    /// First-In-First-Out
    FIFO,

    /// Round Robin
    RoundRobin { quantum: u64 },

    /// Priority-based
    Priority,

    /// Shortest Job First (non-preemptive)
    SJF,

    /// Multilevel Feedback Queue
    MLFQ,
}

impl Default for SchedulingPolicy {
    fn default() -> Self {
        Self::RoundRobin { quantum: 10 }
    }
}

/// Scheduling decision
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SchedulingDecision {
    /// Run the specified process/thread
    Run { pid: Pid, tid: Tid },

    /// No process is ready to run (idle)
    Idle,

    /// All processes are terminated
    Halt,
}

/// Process/Thread ready queue entry
#[derive(Debug, Clone, )]
pub struct ReadyQueueEntry {
    pub pid: Pid,
    pub tid: Tid,
    pub priority: u8,
    pub ready_since: std::time::SystemTime,
}

impl ReadyQueueEntry {
    pub fn new(pid: Pid, tid: Tid, priority: u8) -> Self {
        Self {
            pid,
            tid,
            priority,
            ready_since: std::time::SystemTime::now(),
        }
    }
}

/// Scheduler state
#[derive(Debug, Clone)]
pub struct SchedulerState {
    pub policy: SchedulingPolicy,
    pub ready_count: usize,
    pub running_pid: Option<Pid>,
    pub running_tid: Option<Tid>,
    pub total_scheduled: u64,
    pub context_switches: u64,
}

/// Process/Thread Scheduler
///
/// 曾国藩曰：
/// "调度如点将，当知人善任。"
/// 调度器选择最优进程执行，确保系统高效运行。
#[derive(Debug)]
pub struct Scheduler {
    /// Scheduling policy
    policy: SchedulingPolicy,

    /// Ready queue (processes ready to run)
    ready_queue: VecDeque<ReadyQueueEntry>,

    /// Priority queue (for priority scheduling)
    priority_queue: Vec<ReadyQueueEntry>,

    /// Current running process
    current: Option<(Pid, Tid)>,

    /// Time quantum for Round Robin
    time_slice: u64,

    /// Time used in current time slice
    time_used: u64,

    /// Statistics
    total_scheduled: u64,
    context_switches: u64,
}

impl Scheduler {
    /// Create a new scheduler with the given policy
    pub fn new(policy: SchedulingPolicy) -> Self {
        let quantum = match policy {
            SchedulingPolicy::RoundRobin { quantum } => quantum,
            _ => 10,
        };

        Self {
            policy,
            ready_queue: VecDeque::new(),
            priority_queue: Vec::new(),
            current: None,
            time_slice: quantum,
            time_used: 0,
            total_scheduled: 0,
            context_switches: 0,
        }
    }

    /// Create a FIFO scheduler
    pub fn fifo() -> Self {
        Self::new(SchedulingPolicy::FIFO)
    }

    /// Create a Round Robin scheduler
    pub fn round_robin(quantum: u64) -> Self {
        Self::new(SchedulingPolicy::RoundRobin { quantum })
    }

    /// Create a Priority scheduler
    pub fn priority() -> Self {
        Self::new(SchedulingPolicy::Priority)
    }

    /// Add a process/thread to the ready queue
    pub fn ready(&mut self, pid: Pid, tid: Tid, priority: u8) {
        let entry = ReadyQueueEntry::new(pid, tid, priority);

        match self.policy {
            SchedulingPolicy::FIFO | SchedulingPolicy::RoundRobin { .. } => {
                self.ready_queue.push_back(entry);
            }
            SchedulingPolicy::Priority | SchedulingPolicy::SJF => {
                // Insert sorted by priority (higher = more priority)
                let pos = self.priority_queue
                    .iter()
                    .position(|e| e.priority < priority)
                    .unwrap_or(self.priority_queue.len());
                self.priority_queue.insert(pos, entry);
            }
            SchedulingPolicy::MLFQ => {
                // For MLFQ, use simple FIFO for now
                self.ready_queue.push_back(entry);
            }
        }
    }

    /// Remove a process/thread from the ready queue
    pub fn remove(&mut self, pid: Pid, tid: Tid) -> bool {
        // Check ready queue
        if let Some(pos) = self.ready_queue.iter().position(|e| e.pid == pid && e.tid == tid) {
            self.ready_queue.remove(pos);
            return true;
        }

        // Check priority queue
        if let Some(pos) = self.priority_queue.iter().position(|e| e.pid == pid && e.tid == tid) {
            self.priority_queue.remove(pos);
            return true;
        }

        false
    }

    /// Get the next scheduling decision
    pub fn schedule(&mut self) -> SchedulingDecision {
        match self.policy {
            SchedulingPolicy::RoundRobin { .. } => {
                self.schedule_round_robin()
            }
            SchedulingPolicy::FIFO => {
                self.schedule_fifo()
            }
            SchedulingPolicy::Priority => {
                self.schedule_priority()
            }
            SchedulingPolicy::SJF => {
                self.schedule_fifo() // SJF uses FIFO for simplicity
            }
            SchedulingPolicy::MLFQ => {
                self.schedule_fifo() // MLFQ uses FIFO for simplicity
            }
        }
    }

    /// FIFO scheduling
    fn schedule_fifo(&mut self) -> SchedulingDecision {
        // Check if current is still runnable
        if let Some((pid, tid)) = self.current {
            if !self.ready_queue.is_empty() || !self.priority_queue.is_empty() {
                // Switch to next
                return self.switch_to_next();
            }
            // No other processes, continue current
            return SchedulingDecision::Run { pid, tid };
        }

        // No current process, get next
        self.get_next()
    }

    /// Round Robin scheduling
    fn schedule_round_robin(&mut self) -> SchedulingDecision {
        // Check if current process used its time slice
        if let Some((pid, tid)) = self.current {
            self.time_used += 1;

            if self.time_used >= self.time_slice {
                // Time slice exhausted, switch
                self.time_used = 0;
                self.ready(pid, tid, 128); // Re-add to ready queue
                self.current = None;
                return self.switch_to_next();
            }

            // Still has time, but check if higher priority is waiting
            if !self.ready_queue.is_empty() {
                // For now, just continue (priority could be checked here)
                return SchedulingDecision::Run { pid, tid };
            }

            return SchedulingDecision::Run { pid, tid };
        }

        self.get_next()
    }

    /// Priority scheduling
    fn schedule_priority(&mut self) -> SchedulingDecision {
        if let Some((pid, tid)) = self.current {
            // Check if higher priority process is waiting
            let current_prio = self.get_current_priority(pid, tid);

            if let Some(next) = self.priority_queue.first() {
                if next.priority > current_prio {
                    // Higher priority process waiting, preempt
                    self.ready(pid, tid, current_prio);
                    self.current = None;
                    return self.get_next();
                }
            }

            return SchedulingDecision::Run { pid, tid };
        }

        self.get_next()
    }

    /// Switch to next process in queue
    fn switch_to_next(&mut self) -> SchedulingDecision {
        self.context_switches += 1;

        if let Some(entry) = self.get_next_entry() {
            self.current = Some((entry.pid, entry.tid));
            SchedulingDecision::Run { pid: entry.pid, tid: entry.tid }
        } else {
            self.current = None;
            SchedulingDecision::Idle
        }
    }

    /// Get the next entry to run
    fn get_next_entry(&mut self) -> Option<ReadyQueueEntry> {
        match self.policy {
            SchedulingPolicy::FIFO | SchedulingPolicy::RoundRobin { .. } => {
                self.ready_queue.pop_front()
            }
            SchedulingPolicy::Priority | SchedulingPolicy::SJF => {
                Some(self.priority_queue.remove(0))
            }
            SchedulingPolicy::MLFQ => {
                self.ready_queue.pop_front()
            }
        }
    }

    /// Get next scheduling decision
    fn get_next(&mut self) -> SchedulingDecision {
        if let Some(entry) = self.get_next_entry() {
            self.current = Some((entry.pid, entry.tid));
            self.total_scheduled += 1;
            SchedulingDecision::Run { pid: entry.pid, tid: entry.tid }
        } else {
            self.current = None;
            SchedulingDecision::Idle
        }
    }

    /// Get the priority of current process
    fn get_current_priority(&self, pid: Pid, tid: Tid) -> u8 {
        // Check ready queue for the process
        self.ready_queue.iter()
            .find(|e| e.pid == pid && e.tid == tid)
            .map(|e| e.priority)
            .unwrap_or(128)
    }

    /// Set current process as blocked (not running anymore)
    pub fn block(&mut self, pid: Pid, tid: Tid) {
        if self.current == Some((pid, tid)) {
            self.current = None;
            self.time_used = 0;
        }
        self.remove(pid, tid);
    }

    /// Set current process as terminated
    pub fn terminate(&mut self, pid: Pid, tid: Tid) {
        self.block(pid, tid);
    }

    /// Get the current running process
    pub fn current(&self) -> Option<(Pid, Tid)> {
        self.current
    }

    /// Get the number of ready processes
    pub fn ready_count(&self) -> usize {
        self.ready_queue.len() + self.priority_queue.len()
    }

    /// Check if there are any ready processes
    pub fn has_ready(&self) -> bool {
        !self.ready_queue.is_empty() || !self.priority_queue.is_empty()
    }

    /// Get scheduler state
    pub fn state(&self) -> SchedulerState {
        SchedulerState {
            policy: self.policy,
            ready_count: self.ready_count(),
            running_pid: self.current.map(|(p, _)| p),
            running_tid: self.current.map(|(_, t)| t),
            total_scheduled: self.total_scheduled,
            context_switches: self.context_switches,
        }
    }

    /// Change scheduling policy
    pub fn set_policy(&mut self, policy: SchedulingPolicy) {
        self.policy = policy;

        // Update time slice for round robin
        if let SchedulingPolicy::RoundRobin { quantum } = policy {
            self.time_slice = quantum;
        }
    }

    /// Reset scheduler state
    pub fn reset(&mut self) {
        self.ready_queue.clear();
        self.priority_queue.clear();
        self.current = None;
        self.time_used = 0;
        self.total_scheduled = 0;
        self.context_switches = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fifo_scheduler() {
        let mut scheduler = Scheduler::fifo();

        // Add processes
        scheduler.ready(1, 1, 128);
        scheduler.ready(2, 1, 128);
        scheduler.ready(3, 1, 128);

        assert_eq!(scheduler.ready_count(), 3);

        // Schedule in FIFO order
        let decision = scheduler.schedule();
        assert_eq!(decision, SchedulingDecision::Run { pid: 1, tid: 1 });
        assert_eq!(scheduler.current(), Some((1, 1)));

        // Next should be pid 2
        scheduler.block(1, 1);
        let decision = scheduler.schedule();
        assert_eq!(decision, SchedulingDecision::Run { pid: 2, tid: 1 });
    }

    #[test]
    fn test_round_robin_scheduler() {
        let mut scheduler = Scheduler::round_robin(3); // quantum = 3

        scheduler.ready(1, 1, 128);
        scheduler.ready(2, 1, 128);

        // First schedule
        let decision = scheduler.schedule();
        assert_eq!(decision, SchedulingDecision::Run { pid: 1, tid: 1 });

        // Continue with current process (quantum not exhausted yet)
        let decision = scheduler.schedule();
        assert_eq!(decision, SchedulingDecision::Run { pid: 1, tid: 1 });

        // One more schedule to exhaust quantum
        let decision = scheduler.schedule();
        assert_eq!(decision, SchedulingDecision::Run { pid: 1, tid: 1 });

        // Quantum exhausted, switch to next process
        let decision = scheduler.schedule();
        assert_eq!(decision, SchedulingDecision::Run { pid: 2, tid: 1 });

        // Verify context switch occurred
        let state = scheduler.state();
        assert_eq!(state.context_switches, 1);
    }

    #[test]
    fn test_priority_scheduler() {
        let mut scheduler = Scheduler::priority();

        // Add processes with different priorities
        scheduler.ready(1, 1, 100); // Lower priority
        scheduler.ready(2, 1, 200); // Higher priority
        scheduler.ready(3, 1, 150); // Medium priority

        // Should schedule highest priority first
        let decision = scheduler.schedule();
        assert_eq!(decision, SchedulingDecision::Run { pid: 2, tid: 1 });

        // Next should be medium priority
        scheduler.block(2, 1);
        let decision = scheduler.schedule();
        assert_eq!(decision, SchedulingDecision::Run { pid: 3, tid: 1 });

        // Then low priority
        scheduler.block(3, 1);
        let decision = scheduler.schedule();
        assert_eq!(decision, SchedulingDecision::Run { pid: 1, tid: 1 });
    }

    #[test]
    fn test_remove_from_ready_queue() {
        let mut scheduler = Scheduler::fifo();

        scheduler.ready(1, 1, 128);
        scheduler.ready(2, 1, 128);
        scheduler.ready(3, 1, 128);

        assert_eq!(scheduler.ready_count(), 3);

        // Remove middle process
        assert!(scheduler.remove(2, 1));
        assert_eq!(scheduler.ready_count(), 2);

        // Try to remove again
        assert!(!scheduler.remove(2, 1));

        // Verify remaining processes
        let decision = scheduler.schedule();
        assert_eq!(decision, SchedulingDecision::Run { pid: 1, tid: 1 });
    }

    #[test]
    fn test_block_current() {
        let mut scheduler = Scheduler::fifo();

        scheduler.ready(1, 1, 128);
        scheduler.ready(2, 1, 128);

        let decision = scheduler.schedule();
        assert_eq!(decision, SchedulingDecision::Run { pid: 1, tid: 1 });

        // Block current
        scheduler.block(1, 1);
        assert!(scheduler.current().is_none());

        // Next should be pid 2
        let decision = scheduler.schedule();
        assert_eq!(decision, SchedulingDecision::Run { pid: 2, tid: 1 });
    }

    #[test]
    fn test_empty_scheduler() {
        let mut scheduler = Scheduler::fifo();

        assert!(!scheduler.has_ready());
        assert_eq!(scheduler.ready_count(), 0);

        let decision = scheduler.schedule();
        assert_eq!(decision, SchedulingDecision::Idle);
    }

    #[test]
    fn test_scheduler_state() {
        let mut scheduler = Scheduler::round_robin(5);

        scheduler.ready(1, 1, 128);
        scheduler.ready(2, 1, 128);

        let _ = scheduler.schedule();

        let state = scheduler.state();
        assert_eq!(state.ready_count, 1);
        assert_eq!(state.running_pid, Some(1));
        assert_eq!(state.total_scheduled, 1);
    }

    #[test]
    fn test_context_switches() {
        let mut scheduler = Scheduler::round_robin(2);

        scheduler.ready(1, 1, 128);
        scheduler.ready(2, 1, 128);

        // Schedule first
        let _ = scheduler.schedule();

        // Use time slice and switch
        let _ = scheduler.schedule();
        let _ = scheduler.schedule();

        // Context switch should have occurred
        assert_eq!(scheduler.state().context_switches, 1);
    }

    #[test]
    fn test_reset_scheduler() {
        let mut scheduler = Scheduler::fifo();

        scheduler.ready(1, 1, 128);
        let _ = scheduler.schedule();

        scheduler.reset();

        assert!(!scheduler.has_ready());
        assert!(scheduler.current().is_none());
        assert_eq!(scheduler.state().total_scheduled, 0);
    }

    #[test]
    fn test_terminate() {
        let mut scheduler = Scheduler::fifo();

        scheduler.ready(1, 1, 128);
        scheduler.ready(2, 1, 128);

        let _ = scheduler.schedule();
        assert_eq!(scheduler.current(), Some((1, 1)));

        // Terminate current
        scheduler.terminate(1, 1);
        assert!(scheduler.current().is_none());
        assert_eq!(scheduler.ready_count(), 1);
    }

    #[test]
    fn test_ready_queue_entry() {
        let entry = ReadyQueueEntry::new(100, 1, 200);

        assert_eq!(entry.pid, 100);
        assert_eq!(entry.tid, 1);
        assert_eq!(entry.priority, 200);
    }

    #[test]
    fn test_set_policy() {
        let mut scheduler = Scheduler::fifo();

        scheduler.set_policy(SchedulingPolicy::RoundRobin { quantum: 20 });
        assert_eq!(scheduler.policy, SchedulingPolicy::RoundRobin { quantum: 20 });
    }
}
