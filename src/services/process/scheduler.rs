// Process/Thread Scheduler — SMP-aware per-CPU state
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use crate::messaging::{Pid, Tid, VirtAddr};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchedulingPolicy {
    FIFO,
    RoundRobin { quantum: u64 },
    Priority,
    SJF,
    MLFQ,
}

impl Default for SchedulingPolicy {
    fn default() -> Self { Self::RoundRobin { quantum: 10 } }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SchedulingDecision {
    Run { pid: Pid, tid: Tid },
    Idle,
    Halt,
}

#[derive(Debug, Clone)]
pub struct ReadyQueueEntry {
    pub pid: Pid,
    pub tid: Tid,
    pub priority: u8,
    pub ready_since: std::time::SystemTime,
}

impl ReadyQueueEntry {
    pub fn new(pid: Pid, tid: Tid, priority: u8) -> Self {
        Self { pid, tid, priority, ready_since: std::time::SystemTime::now() }
    }
}

#[derive(Debug, Clone)]
pub struct SchedulerState {
    pub policy: SchedulingPolicy,
    pub ready_count: usize,
    pub running_pid: Option<Pid>,
    pub running_tid: Option<Tid>,
    pub total_scheduled: u64,
    pub context_switches: u64,
}

/// SMP-aware scheduler with per-CPU state
#[derive(Debug)]
pub struct Scheduler {
    policy: SchedulingPolicy,
    ready_queue: VecDeque<ReadyQueueEntry>,
    priority_queue: Vec<ReadyQueueEntry>,

    /// Per-CPU: which process is currently running
    cpu_current: Vec<Option<(Pid, Tid)>>,
    /// Per-CPU: ticks consumed in current quantum
    cpu_ticks: Vec<u64>,

    time_slice: u64,
    total_scheduled: u64,
    context_switches: u64,
}

impl Scheduler {
    pub fn new(policy: SchedulingPolicy, cpu_count: usize) -> Self {
        let quantum = match policy {
            SchedulingPolicy::RoundRobin { quantum } => quantum,
            _ => 10,
        };
        Self {
            policy,
            ready_queue: VecDeque::new(),
            priority_queue: Vec::new(),
            cpu_current: vec![None; cpu_count],
            cpu_ticks: vec![0; cpu_count],
            time_slice: quantum,
            total_scheduled: 0,
            context_switches: 0,
        }
    }

    pub fn fifo() -> Self { Self::new(SchedulingPolicy::FIFO, 1) }
    pub fn round_robin(quantum: u64) -> Self { Self::new(SchedulingPolicy::RoundRobin { quantum }, 1) }
    pub fn priority() -> Self { Self::new(SchedulingPolicy::Priority, 1) }

    /// Add a process to the ready queue
    pub fn ready(&mut self, pid: Pid, tid: Tid, priority: u8) {
        let entry = ReadyQueueEntry::new(pid, tid, priority);
        match self.policy {
            SchedulingPolicy::FIFO | SchedulingPolicy::RoundRobin { .. } => {
                self.ready_queue.push_back(entry);
            }
            SchedulingPolicy::Priority | SchedulingPolicy::SJF => {
                let pos = self.priority_queue.iter()
                    .position(|e| e.priority < priority)
                    .unwrap_or(self.priority_queue.len());
                self.priority_queue.insert(pos, entry);
            }
            SchedulingPolicy::MLFQ => { self.ready_queue.push_back(entry); }
        }
    }

    /// Remove a process from ready queue and all CPU currents
    pub fn remove(&mut self, pid: Pid, tid: Tid) -> bool {
        for cpu in 0..self.cpu_current.len() {
            if self.cpu_current[cpu] == Some((pid, tid)) {
                self.cpu_current[cpu] = None;
                self.cpu_ticks[cpu] = 0;
            }
        }
        if let Some(pos) = self.ready_queue.iter().position(|e| e.pid == pid && e.tid == tid) {
            self.ready_queue.remove(pos);
            return true;
        }
        false
    }

    /// Block a process: remove from scheduler entirely
    pub fn block(&mut self, pid: Pid, tid: Tid) -> bool {
        self.remove(pid, tid)
    }

    /// SMP-aware schedule: pick a process for CPU `cpu_id`
    pub fn schedule(&mut self, cpu_id: usize) -> SchedulingDecision {
        // 1. Check if current process on this CPU still has quantum remaining
        if let Some((pid, tid)) = self.cpu_current[cpu_id] {
            self.cpu_ticks[cpu_id] += 1;
            if self.cpu_ticks[cpu_id] < self.time_slice {
                return SchedulingDecision::Run { pid, tid };
            }
            // Quantum expired: re-queue and pick next
            self.ready(pid, tid, 128);
            self.cpu_current[cpu_id] = None;
            self.cpu_ticks[cpu_id] = 0;
        }

        // 2. Pick next from ready queue
        self.dequeue_next(cpu_id)
    }

    /// Dequeue next, skipping PIDs already running on other CPUs
    fn dequeue_next(&mut self, cpu_id: usize) -> SchedulingDecision {
        let busy: Vec<Pid> = self.cpu_current.iter()
            .enumerate().filter(|&(i,_)| i != cpu_id)
            .filter_map(|(_,c)| c.map(|(p,_)| p)).collect();

        let n = self.ready_queue.len();
        for _ in 0..n {
            let entry = self.ready_queue.pop_front();
            if let Some(e) = entry {
                if busy.contains(&e.pid) {
                    self.ready_queue.push_back(e);
                    continue;
                }
                self.cpu_current[cpu_id] = Some((e.pid, e.tid));
                self.cpu_ticks[cpu_id] = 0;
                self.total_scheduled += 1;
                self.context_switches += 1;
                return SchedulingDecision::Run { pid: e.pid, tid: e.tid };
            }
        }
        self.cpu_current[cpu_id] = None;
        SchedulingDecision::Idle
    }

    /// Re-queue current process on this CPU and pick next (for SMP dedup)
    pub fn yield_current(&mut self, cpu_id: usize) -> SchedulingDecision {
        if let Some((pid, tid)) = self.cpu_current[cpu_id] {
            self.ready(pid, tid, 128);
            self.cpu_current[cpu_id] = None;
            self.cpu_ticks[cpu_id] = 0;
        }
        self.dequeue_next(cpu_id)
    }

    /// How many processes are in the ready queue
    pub fn ready_count(&self) -> usize { self.ready_queue.len() + self.priority_queue.len() }

    /// Get current process on a CPU
    pub fn current_on(&self, cpu_id: usize) -> Option<(Pid, Tid)> {
        self.cpu_current.get(cpu_id).and_then(|c| *c)
    }

    pub fn total_scheduled(&self) -> u64 { self.total_scheduled }
    pub fn context_switches(&self) -> u64 { self.context_switches }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_smp_schedule_two_cpus() {
        let mut s = Scheduler::new(SchedulingPolicy::RoundRobin { quantum: 3 }, 2);
        s.ready(1, 1, 128);
        s.ready(2, 1, 128);

        // CPU0 picks PID 1
        let d0 = s.schedule(0);
        assert_eq!(d0, SchedulingDecision::Run { pid: 1, tid: 1 });

        // CPU1 picks PID 2 (not PID 1 — different CPU)
        let d1 = s.schedule(1);
        assert_eq!(d1, SchedulingDecision::Run { pid: 2, tid: 1 });
    }

    #[test]
    fn test_quantum_expiry() {
        let mut s = Scheduler::new(SchedulingPolicy::RoundRobin { quantum: 2 }, 1);
        s.ready(1, 1, 128);
        s.ready(2, 1, 128);

        // Tick 1: PID 1
        assert_eq!(s.schedule(0), SchedulingDecision::Run { pid: 1, tid: 1 });
        // Tick 2: PID 1 still (quantum not expired)
        assert_eq!(s.schedule(0), SchedulingDecision::Run { pid: 1, tid: 1 });
        // Tick 3: quantum expired, switch to PID 2
        assert_eq!(s.schedule(0), SchedulingDecision::Run { pid: 2, tid: 1 });
    }

    #[test]
    fn test_smp_dedup_prevented_by_yield() {
        let mut s = Scheduler::new(SchedulingPolicy::RoundRobin { quantum: 3 }, 2);
        s.ready(1, 1, 128);
        s.ready(2, 1, 128);

        // CPU0 takes PID 1
        assert_eq!(s.schedule(0), SchedulingDecision::Run { pid: 1, tid: 1 });
        // CPU1 would get PID 1 too (quantum not expired), but dedup should yield
        // Caller must use yield_current(1) if pid==1 is already assigned
        let d1 = s.yield_current(1);
        assert_eq!(d1, SchedulingDecision::Run { pid: 2, tid: 1 });
    }

    #[test]
    fn test_empty_queue_returns_idle() {
        let mut s = Scheduler::new(SchedulingPolicy::RoundRobin { quantum: 3 }, 2);
        assert_eq!(s.schedule(0), SchedulingDecision::Idle);
        assert_eq!(s.schedule(1), SchedulingDecision::Idle);
    }

    #[test]
    fn test_remove_from_cpu_and_queue() {
        let mut s = Scheduler::new(SchedulingPolicy::RoundRobin { quantum: 3 }, 2);
        s.ready(1, 1, 128);
        s.ready(2, 1, 128);

        // CPU0 picks PID 1
        let _ = s.schedule(0);
        // Remove PID 1
        assert!(s.remove(1, 1));
        // CPU0 should not have PID 1 anymore
        assert_eq!(s.current_on(0), None);
        // CPU0 picks next
        assert_eq!(s.schedule(0), SchedulingDecision::Run { pid: 2, tid: 1 });
    }

    #[test]
    fn test_block_is_alias_for_remove() {
        let mut s = Scheduler::new(SchedulingPolicy::RoundRobin { quantum: 3 }, 1);
        s.ready(1, 1, 128);
        let _ = s.schedule(0);
        assert!(s.block(1, 1));
        assert_eq!(s.ready_count(), 0);
    }

    #[test]
    fn test_round_robin_three_processes() {
        let mut s = Scheduler::new(SchedulingPolicy::RoundRobin { quantum: 2 }, 1);
        s.ready(1, 1, 128);
        s.ready(2, 1, 128);
        s.ready(3, 1, 128);

        // Q1: PID 1
        assert_eq!(s.schedule(0), SchedulingDecision::Run { pid: 1, tid: 1 });
        assert_eq!(s.schedule(0), SchedulingDecision::Run { pid: 1, tid: 1 });
        // Q1 expired → PID 2
        assert_eq!(s.schedule(0), SchedulingDecision::Run { pid: 2, tid: 1 });
        assert_eq!(s.schedule(0), SchedulingDecision::Run { pid: 2, tid: 1 });
        // Q2 expired → PID 3
        assert_eq!(s.schedule(0), SchedulingDecision::Run { pid: 3, tid: 1 });
        assert_eq!(s.schedule(0), SchedulingDecision::Run { pid: 3, tid: 1 });
        // Q3 expired → PID 1 again (round-robin)
        assert_eq!(s.schedule(0), SchedulingDecision::Run { pid: 1, tid: 1 });
    }

    #[test]
    fn test_smp_two_cpus_full_round() {
        let mut s = Scheduler::new(SchedulingPolicy::RoundRobin { quantum: 3 }, 2);
        s.ready(1, 1, 128);
        s.ready(2, 1, 128);
        s.ready(3, 1, 128);

        // Tick 1: CPU0=PID1, CPU1=PID2
        assert_eq!(s.schedule(0), SchedulingDecision::Run { pid: 1, tid: 1 });
        assert_eq!(s.schedule(1), SchedulingDecision::Run { pid: 2, tid: 1 });

        // Tick 2: keep same (quantum not expired)
        assert_eq!(s.schedule(0), SchedulingDecision::Run { pid: 1, tid: 1 });
        assert_eq!(s.schedule(1), SchedulingDecision::Run { pid: 2, tid: 1 });

        // Tick 3: keep same (last before expiry)
        assert_eq!(s.schedule(0), SchedulingDecision::Run { pid: 1, tid: 1 });
        assert_eq!(s.schedule(1), SchedulingDecision::Run { pid: 2, tid: 1 });

        // Tick 4: quantum expired → CPU0=PID3, CPU1=PID1 (re-queued)
        assert_eq!(s.schedule(0), SchedulingDecision::Run { pid: 3, tid: 1 });
        assert_eq!(s.schedule(1), SchedulingDecision::Run { pid: 1, tid: 1 });
    }

    #[test]
    fn test_yield_current_requeues_and_picks_next() {
        let mut s = Scheduler::new(SchedulingPolicy::RoundRobin { quantum: 3 }, 1);
        s.ready(1, 1, 128);
        s.ready(2, 1, 128);
        s.ready(3, 1, 128);

        // Pick PID 1
        assert_eq!(s.schedule(0), SchedulingDecision::Run { pid: 1, tid: 1 });
        // Yield: PID 1 back to queue, pick next
        let d = s.yield_current(0);
        assert_eq!(d, SchedulingDecision::Run { pid: 2, tid: 1 });
        // PID 1 should be back in queue
        assert_eq!(s.ready_count(), 2); // PID 1 + PID 3
    }

    #[test]
    fn test_remove_nonexistent_returns_false() {
        let mut s = Scheduler::new(SchedulingPolicy::RoundRobin { quantum: 3 }, 1);
        assert!(!s.remove(99, 1));
    }

    #[test]
    fn test_new_process_joins_back_of_queue() {
        let mut s = Scheduler::new(SchedulingPolicy::RoundRobin { quantum: 3 }, 1);
        s.ready(1, 1, 128);
        s.ready(2, 1, 128);
        let _ = s.schedule(0); // gets PID 1

        // Add PID 3 while PID 1 is running
        s.ready(3, 1, 128);

        // Quantum expires → PID 2 (was in queue before PID 3)
        s.schedule(0); s.schedule(0); // tick 2,3 of PID 1
        assert_eq!(s.schedule(0), SchedulingDecision::Run { pid: 2, tid: 1 });
    }

    #[test]
    fn test_block_removes_from_cpu_and_ready_queue() {
        let mut s = Scheduler::new(SchedulingPolicy::RoundRobin { quantum: 3 }, 2);
        s.ready(1, 1, 128); s.ready(2, 1, 128); s.ready(3, 1, 128);
        assert_eq!(s.schedule(0), SchedulingDecision::Run { pid: 1, tid: 1 });
        assert_eq!(s.schedule(1), SchedulingDecision::Run { pid: 2, tid: 1 });
        assert!(s.block(1, 1));
        assert_eq!(s.current_on(0), None);
        assert_eq!(s.ready_count(), 1);
    }

    #[test]
    fn test_blocked_not_returned_on_expiry() {
        let mut s = Scheduler::new(SchedulingPolicy::RoundRobin { quantum: 3 }, 1);
        s.ready(1, 1, 128); s.ready(2, 1, 128);
        let _ = s.schedule(0); s.block(1, 1);
        s.schedule(0); s.schedule(0);
        assert_eq!(s.schedule(0), SchedulingDecision::Run { pid: 2, tid: 1 });
    }

    #[test]
    fn test_unblock_readds_to_queue() {
        let mut s = Scheduler::new(SchedulingPolicy::RoundRobin { quantum: 3 }, 1);
        s.ready(1, 1, 128); s.ready(2, 1, 128);
        let _ = s.schedule(0); s.block(1, 1);
        s.ready(1, 1, 128);
        assert_eq!(s.ready_count(), 2);
    }
}
