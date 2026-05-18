use crate::vprintln;
// ProcessService - Main Process Management Service
//
// 曾国藩曰：
// "为将之道，当知进退，明赏罚。"
// 进程服务管理进程之生死、调度与通信，当公平高效。

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use crossbeam_channel::{Receiver, Sender};
use crate::messaging::{
    KernelMsg, ProcessRequest, Syscall, Interrupt, Pid, Tid,
    VirtAddr, PhysAddr, IPCMessage, SignalType, BlockReason, MemProt,
    MessageBus, RequestWithResponse, Response, ResponseData,
    ServiceError as MsgServiceError,
};
use crate::messaging::bus::Envelope;
use crate::hardware::CPUState;
use crate::error::{GenshinError, ServiceError};
use crate::GenshinResult;

// Import process service components
use super::pcb::{PCB, TCB, ProcessState, ThreadState};
use super::ipc::{IPCManager, MessageQueue, SharedMemoryRegion};
use super::sync::{SyncManager, Semaphore, MutexLock};
use super::scheduler::{Scheduler, SchedulingPolicy, SchedulingDecision};

/// Process Service - Main process management service
///
/// 曾国藩曰：
/// "治军如治家，赏罚分明，进退有度。"
/// 进程服务统筹进程管理、调度、IPC与同步，确保系统高效运行。

pub struct ProcessService {
    /// Message bus for sending/receiving messages
    bus: Arc<dyn MessageBus>,

    /// Receiver from Kernel (service requests)
    receiver: Receiver<Envelope>,
    /// Direct bus receiver for hardware interrupts
    intr_rx: Receiver<Envelope>,

    /// Process table (pid -> PCB)
    process_table: Arc<Mutex<HashMap<Pid, Arc<Mutex<PCB>>>>>,

    /// Next process ID to assign
    next_pid: Arc<Mutex<Pid>>,

    /// IPC manager
    ipc_manager: Arc<Mutex<IPCManager>>,

    /// Sync manager
    sync_manager: Arc<Mutex<SyncManager>>,

    /// Process/thread scheduler
    scheduler: Arc<Mutex<Scheduler>>,

    /// Parent-child relationships (pid -> children pids)
    parent_children: Arc<Mutex<HashMap<Pid, Vec<Pid>>>>,
    _hw: crate::hardware::PhysicalMemory,
    _mmu: Arc<crate::hardware::MMU>,
    cpus: Arc<Mutex<HashMap<Pid, crate::hardware::VirtualCPU>>>,

    /// Pending fork requests: list of caller PIDs to process in main loop
    pending_forks: Arc<Mutex<Vec<Pid>>>,
    /// Parents waiting on children: (child_pid, (parent_pid, response_channel))
    waiting_parents: Arc<Mutex<Vec<(Pid, (Pid, crossbeam_channel::Sender<Response>))>>>,
    /// Last scheduled PID per CPU: track for Running→Ready on preemption
    last_running: Arc<Mutex<Vec<Option<Pid>>>>,
    cpu_count: usize,
}

impl std::fmt::Debug for ProcessService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProcessService").finish_non_exhaustive()
    }
}

impl ProcessService {
    pub fn new(bus: Arc<dyn MessageBus>, hw: crate::hardware::PhysicalMemory, mmu: Arc<crate::hardware::MMU>, receiver: Receiver<Envelope>, intr_rx: Receiver<Envelope>) -> Self {
        Self {
            bus,
            receiver,
            intr_rx,
            process_table: Arc::new(Mutex::new(HashMap::new())),
            next_pid: Arc::new(Mutex::new(1)),
            ipc_manager: Arc::new(Mutex::new(IPCManager::new())),
            sync_manager: Arc::new(Mutex::new(SyncManager::new())),
            scheduler: Arc::new(Mutex::new(Scheduler::new(SchedulingPolicy::RoundRobin { quantum: 3 }, 2))),
            parent_children: Arc::new(Mutex::new(HashMap::new())),
            _hw: hw, _mmu: mmu,
            cpus: Arc::new(Mutex::new(HashMap::new())),
            pending_forks: Arc::new(Mutex::new(Vec::new())),
            waiting_parents: Arc::new(Mutex::new(Vec::new())),
            cpu_count: 2,
            last_running: Arc::new(Mutex::new(vec![None, None])),
        }
    }

    /// Run the process service (main loop)
    pub fn run(&self) {
        println!("ProcessService starting...");

        // Create init process (PID 1) — root of user process tree
        match self.fork_impl(0) {
            Ok(pid) => println!("PS: Init PID = {}", pid),
            Err(e) => eprintln!("PS: Init failed: {}", e),
        }

        loop {
            // Process timer interrupts (max 50/iter to avoid starving receiver)
            let mut ticked = false;
            for _ in 0..10 {
                match self.intr_rx.try_recv() {
                    Ok(env) => {
                        if matches!(&env.message, KernelMsg::Interrupt(_)) {
                            self.handle_timer_interrupt().ok();
                            ticked = true;
                        }
                    }
                    Err(_) => break,
                }
            }
            // Always check receiver
            match self.receiver.try_recv() {
                Ok(envelope) => {
                    if let Err(e) = self.handle_envelope(envelope) {
                        eprintln!("ProcessService error: {}", e);
                    }
                }
                Err(crossbeam_channel::TryRecvError::Empty) => {
                    if !ticked { std::thread::sleep(std::time::Duration::from_millis(1)); }
                }
                Err(crossbeam_channel::TryRecvError::Disconnected) => {
                    eprintln!("Message bus disconnected");
                    return;
                }
            }
        }
    }

    /// Handle incoming envelope
    fn handle_envelope(&self, envelope: Envelope) -> GenshinResult<()> {
        let msg = envelope.message.clone();

        // Handle the message
        let result = match msg {
            KernelMsg::Process(req) => {
                if envelope.expects_response() {
                    self.handle_process_request_with_response(req, &envelope)
                } else {
                    self.handle_process_request(req)
                }
            }
            KernelMsg::Syscall(req) => {
                if envelope.expects_response() {
                    self.handle_syscall_with_response(req, &envelope)
                } else {
                    self.handle_syscall(req)
                }
            }
            KernelMsg::Interrupt(int) => self.handle_interrupt(int),
            _ => {
                // Ignore other messages
                Ok(())
            }
        };

        // Log errors but don't fail the service
        if let Err(e) = result {
            eprintln!("ProcessService error handling message: {}", e);

            // If this was a request, send error response
            if envelope.expects_response() {
                let _ = envelope.respond_error(MsgServiceError::Other {
                    code: 1,
                    msg: e.to_string(),
                });
            }
        }

        Ok(())
    }

    /// Handle process service request
    fn handle_process_request(&self, req: ProcessRequest) -> GenshinResult<()> {
        match req {
            // ========== Scheduling ==========
            ProcessRequest::Schedule { pid, tid } => {
                self.handle_schedule(pid, tid)?;
            }

            ProcessRequest::Block { pid, tid, reason } => {
                self.handle_block(pid, tid, reason)?;
            }

            ProcessRequest::Unblock { pid, tid } => {
                self.handle_unblock(pid, tid)?;
            }

            ProcessRequest::QueryState { pid } => {
                self.handle_query_state(pid)?;
            }

            ProcessRequest::ContextSwitch { from_pid, to_pid } => {
                self.handle_context_switch(from_pid, to_pid)?;
            }

            // ========== IPC: Message Passing ==========
            ProcessRequest::SendMessage { from_pid, to_pid, msg } => {
                self.handle_send_message(from_pid, to_pid, msg)?;
            }

            ProcessRequest::ReceiveMessage { pid, blocking } => {
                self.handle_receive_message(pid, blocking)?;
            }

            ProcessRequest::PeekMessage { pid } => {
                self.handle_peek_message(pid)?;
            }

            // ========== IPC: Shared Memory ==========
            ProcessRequest::CreateSharedMemory { pid, size, prot } => {
                self.handle_create_shared_memory(pid, size, prot)?;
            }

            ProcessRequest::AttachSharedMemory { pid, shmid } => {
                self.handle_attach_shared_memory(pid, shmid)?;
            }

            ProcessRequest::DetachSharedMemory { pid, shmid } => {
                self.handle_detach_shared_memory(pid, shmid)?;
            }

            // ========== IPC: Synchronization ==========
            ProcessRequest::CreateSemaphore { pid, initial_value } => {
                self.handle_create_semaphore(pid, initial_value)?;
            }

            ProcessRequest::WaitSemaphore { pid, semid } => {
                self.handle_wait_semaphore(pid, semid)?;
            }

            ProcessRequest::SignalSemaphore { pid, semid } => {
                self.handle_signal_semaphore(pid, semid)?;
            }

            ProcessRequest::CreateLock { pid } => {
                self.handle_create_lock(pid)?;
            }

            ProcessRequest::AcquireLock { pid, lock_id } => {
                self.handle_acquire_lock(pid, lock_id)?;
            }

            ProcessRequest::ReleaseLock { pid, lock_id } => {
                self.handle_release_lock(pid, lock_id)?;
            }

            // ========== Process Lifecycle ==========
            ProcessRequest::ForkProcess { parent_pid } => {
                self.handle_fork(parent_pid)?;
            }

            ProcessRequest::ExecProcess { pid, executable, args, .. } => {
                self.handle_exec(pid, executable, args)?;
            }

            ProcessRequest::WaitChild { pid, child_pid } => {
                self.handle_wait_child(pid, child_pid)?;
            }

            ProcessRequest::Signal { pid, signal } => {
                self.handle_signal(pid, signal)?;
            }

            ProcessRequest::GetProcessInfo { pid } => {
                self.handle_get_process_info(pid)?;
            }

            ProcessRequest::ListProcesses => {
                self.handle_list_processes()?;
            }
            ProcessRequest::Spawn { program, .. } => {
                let pname = if program == "dual" { "busy" } else if program.starts_with("dual:") { &program[5..] } else { &program };
                let count = if program.starts_with("dual") { 2 } else { 1 };
                let mut shared_frame: Option<u64> = None;
                if pname == "rwlock2" {
                    let mut sync = self.sync_manager.lock().unwrap();
                    sync.create_semaphore(0, 1);
                    sync.create_semaphore(0, 1);
                    drop(sync);
                    if let Ok(rx) = self.bus.send_request(KernelMsg::Memory(crate::messaging::MemoryRequest::AllocFrame { count: 1, pid: 0 })) {
                        if let Ok(resp) = rx.recv_timeout(std::time::Duration::from_millis(200)) {
                            if let Some(crate::messaging::ResponseData::PhysicalAddr(addr)) = resp.data() {
                                shared_frame = Some(*addr);
                            }
                        }
                    }
                }
                let mut child_pids: Vec<Pid> = Vec::new();
                for _ in 0..count {
                    if let Ok(child) = self.fork_impl(0) {
                        let _ = self.exec_impl(child, pname.to_string(), vec![]);
                        child_pids.push(child);
                    }
                }
                if let Some(frame_addr) = shared_frame {
                    for &pid in &child_pids {
                        if let Ok(rx) = self.bus.send_request(KernelMsg::Memory(crate::messaging::MemoryRequest::MapPage {
                            pid, virt: 0x10000, phys: frame_addr,
                            prot: crate::messaging::MemProt { readable: true, writable: true, executable: false },
                        })) { let _ = rx.recv_timeout(std::time::Duration::from_secs(1)); }
                    }
                }
            }

            ProcessRequest::GetStats => {} // handled via response path

            ProcessRequest::GetMemoryMap => {} // handled via response path
        }

        Ok(())
    }

    /// Handle process service request with response
    fn handle_process_request_with_response(&self, req: ProcessRequest, envelope: &Envelope) -> GenshinResult<()> {
        match req {
            // ========== Process Lifecycle ==========
            ProcessRequest::ForkProcess { parent_pid } => {
                self.handle_fork_with_response(parent_pid, envelope)?;
            }

            ProcessRequest::ExecProcess { pid, executable, args, .. } => {
                self.handle_exec_with_response(pid, executable, args, envelope)?;
            }

            ProcessRequest::WaitChild { pid, child_pid } => {
                self.handle_wait_child_with_response(pid, child_pid, envelope)?;
            }

            ProcessRequest::GetProcessInfo { pid } => {
                self.handle_get_process_info_with_response(pid, envelope)?;
            }

            ProcessRequest::GetStats => {
                // Collect process table for TUI monitor
                let table = Self::lock_mutex(&self.process_table)?;
                let mut lines: Vec<String> = Vec::new();
                let mut pids: Vec<Pid> = table.keys().cloned().collect();
                pids.sort();
                for pid in pids {
                    if let Some(pcb) = table.get(&pid) {
                        if let Ok(pcb) = pcb.lock() {
                            let state = format!("{:?}", pcb.state)
                                .replace("ProcessState::", "")
                                .replace("Zombie { exit_code: ", "Zombie(")
                                .replace(" }", ")")
                                .replace("Blocked(", "Blocked");
                            let ppid_str = pcb.parent_pid.map(|p| p.to_string()).unwrap_or_else(|| "-".into());
                            lines.push(format!("{:>4} {:>12} {:<20} {:>4}", pid, state, pcb.name, ppid_str));
                        }
                    }
                }
                let _ = envelope.respond_success(ResponseData::StringList(lines));
            }

            ProcessRequest::GetMemoryMap => {
                // Return per-process virtual address ranges
                let table = Self::lock_mutex(&self.process_table)?;
                let mut pids: Vec<Pid> = table.keys().cloned().collect();
                pids.sort();
                let mut lines: Vec<String> = Vec::new();
                for pid in pids {
                    let mut entries = self._mmu.get_page_entries(pid);
                    if entries.is_empty() { continue; }
                    entries.sort_by_key(|(v, _, _)| *v);
                    let mut ranges: Vec<String> = Vec::new();
                    for (_, paddr, _) in &entries {
                        let pend = paddr + 0xFFF;
                        ranges.push(format!("0x{:04X}-0x{:04X}", paddr, pend));
                    }
                    let total_kb = entries.len() * 4;
                    lines.push(format!("PID {:>3}: {} ({:>4} KB)", pid, ranges.join(", "), total_kb));
                }
                let _ = envelope.respond_success(ResponseData::StringList(lines));
            }

            ProcessRequest::ListProcesses => {
                self.handle_list_processes_with_response(envelope)?;
            }

            ProcessRequest::Spawn { program, params } => {
                self.handle_spawn(program, params, envelope)?;
            }

            _ => {
                // For other requests, try the regular handler and return void response
                self.handle_process_request(req)?;
                envelope.respond_success(ResponseData::Void)?;
            }
        }

        Ok(())
    }

    /// Handle syscall with response
    fn handle_syscall_with_response(&self, syscall: Syscall, envelope: &Envelope) -> GenshinResult<()> {
        match syscall {
            Syscall::CreateProcess { executable, args } => {
                let pid = self.create_process(&executable, args)?;
                envelope.respond_success(ResponseData::Pid(pid))?;
                vprintln!("ProcessService: Created process {} ({})", pid, executable);
            }

            Syscall::ExitProcess { exit_code } => {
                // Need to get current PID from context
                // For now, we'll implement a simpler version
                println!("ProcessService: Exit with code {}", exit_code);
                envelope.respond_success(ResponseData::Void)?;
            }

            _ => {
                // Handle other syscalls and return void response
                self.handle_syscall(syscall)?;
                envelope.respond_success(ResponseData::Void)?;
            }
        }

        Ok(())
    }

    /// Handle system call
    fn handle_syscall(&self, syscall: Syscall) -> GenshinResult<()> {
        match syscall {
            Syscall::CreateProcess { executable, args } => {
                let pid = self.create_process(&executable, args)?;
                vprintln!("ProcessService: Created process {} ({})", pid, executable);
            }

            Syscall::ExitProcess { exit_code } => {
                // Need to get current PID from context
                // For now, we'll implement a simpler version
                println!("ProcessService: Exit with code {}", exit_code);
            }

            Syscall::CreateThread { entry_point } => {
                // Create thread in current process
                // For now, just log
                println!("ProcessService: Create thread at {:#x}", entry_point);
            }

            _ => {
                println!("ProcessService: Received syscall {:?}", syscall);
            }
        }

        Ok(())
    }

    /// Handle hardware interrupt
    fn handle_interrupt(&self, interrupt: Interrupt) -> GenshinResult<()> {
        match interrupt {
            Interrupt::Timer => {
                // Timer interrupt - trigger scheduling
                self.handle_timer_interrupt()?;
            }

            Interrupt::PageFault { addr, access_type } => {
                // Page fault - forward to memory service
                let msg = crate::messaging::MemoryRequest::PageFaultHandler {
                    pid: 0, // Need to get current PID
                    faulting_addr: addr,
                    access_type,
                };
                let _ = self.bus.send(KernelMsg::Memory(msg));
            }

            Interrupt::HardwareFailure { component } => {
                eprintln!("ProcessService: Hardware failure in {}", component);
            }

            _ => {
                println!("ProcessService: Received interrupt {:?}", interrupt);
            }
        }

        Ok(())
    }

    // ========== Scheduling Handlers ==========

    fn handle_schedule(&self, pid: Pid, tid: Tid) -> GenshinResult<()> {


        // Get process priority
        let table = Self::lock_mutex(&self.process_table)?;
        let priority = if let Some(pcb) = table.get(&pid) {
            let pcb = Self::lock_mutex(pcb)?;
            pcb.priority
        } else {
            128 // Default priority
        };

        let mut scheduler = Self::lock_mutex(&self.scheduler)?;
        scheduler.ready(pid, tid, priority);
        Ok(())
    }

    fn handle_block(&self, pid: Pid, tid: Tid, reason: BlockReason) -> GenshinResult<()> {
        let mut scheduler = Self::lock_mutex(&self.scheduler)?;
        scheduler.block(pid, tid);

        // Update PCB state
        let table = Self::lock_mutex(&self.process_table)?;
        if let Some(pcb) = table.get(&pid) {
            let mut pcb = Self::lock_mutex(pcb)?;
            pcb.state = ProcessState::Blocked(BlockReason::WaitingForIo { device_id: 0 });
        }

        println!("ProcessService: Blocked {}:{} ({:?})", pid, tid, reason);
        Ok(())
    }

    fn handle_unblock(&self, pid: Pid, tid: Tid) -> GenshinResult<()> {
        let table = Self::lock_mutex(&self.process_table)?;
        if let Some(pcb) = table.get(&pid) {
            let mut pcb = Self::lock_mutex(pcb)?;
            pcb.state = ProcessState::Ready;
        }

        // Add to ready queue
        drop(table); // Release lock before calling handle_schedule
        self.handle_schedule(pid, tid)?;

        println!("ProcessService: Unblocked {}:{}", pid, tid);
        Ok(())
    }

    fn handle_query_state(&self, pid: Pid) -> GenshinResult<()> {
        let table = Self::lock_mutex(&self.process_table)?;
        if let Some(pcb) = table.get(&pid) {
            let pcb = Self::lock_mutex(pcb)?;
            println!("ProcessService: {} state: {:?}", pid, pcb.state);
        } else {
            return Err(GenshinError::Service(ServiceError::NotFound {
                resource_type: "Process".to_string(),
                id: pid.to_string(),
            }));
        }
        Ok(())
    }

    fn handle_context_switch(&self, from_pid: Pid, to_pid: Pid) -> GenshinResult<()> {
        println!("ProcessService: Context switch {} -> {}", from_pid, to_pid);

        // Update scheduler
        let mut scheduler = Self::lock_mutex(&self.scheduler)?;

        // Block current process
        scheduler.block(from_pid, 1);

        // Schedule next process
        scheduler.ready(to_pid, 1, 128);

        Ok(())
    }

    // Scheduler quantum: 10 timer ticks per time slice (= ~9 instructions)

    fn handle_timer_interrupt(&self) -> GenshinResult<()> {
        // SMP: schedule one unique process per vCPU each tick
        let mut scheduled_this_tick = std::collections::HashSet::new();
        for cpu_id in 0..self.cpu_count {
            let mut scheduler = Self::lock_mutex(&self.scheduler)?;
            let mut decision = scheduler.schedule(cpu_id);
            // Dedup: if PID already assigned to another CPU, try next from queue
            if let SchedulingDecision::Run { pid, .. } = &decision {
                if scheduled_this_tick.contains(pid) {
                    // Re-queue to back, then pick next from queue
                    decision = scheduler.yield_current(cpu_id);
                    if let SchedulingDecision::Run { pid: pid2, .. } = &decision {
                        scheduled_this_tick.insert(*pid2);
                    }
                } else {
                    scheduled_this_tick.insert(*pid);
                }
            }
            drop(scheduler);

        // State machine: transition previous Running→Ready on preemption
        {
            let mut last = self.last_running.lock().unwrap();
            if let SchedulingDecision::Run { pid, .. } = &decision {
                let prev = last[cpu_id];
                if prev != Some(*pid) {
                    if let Some(prev_pid) = prev {
                        // Only transition to Ready if NOT running on another CPU
                        let busy_on_other = (0..self.cpu_count).any(|i| i != cpu_id && last[i] == Some(prev_pid));
                        if !busy_on_other {
                            if let Some(pcb) = self.process_table.lock().unwrap().get(&prev_pid) {
                                if let Ok(mut p) = pcb.lock() {
                                    if p.state == ProcessState::Running {
                                        p.state = ProcessState::Ready;
                                    }
                                }
                            }
                        }
                    }
                    last[cpu_id] = Some(*pid);
                }
            } else {
                // Idle: just clear last — process state managed by scheduler
                last[cpu_id] = None;
            }
        }

        if let SchedulingDecision::Run { pid, .. } = decision {
            // State machine: mark as Running
            if let Some(pcb) = self.process_table.lock().unwrap().get(&pid) {
                if let Ok(mut p) = pcb.lock() {
                    if p.state == ProcessState::Ready || p.state == ProcessState::Creating {
                        p.state = ProcessState::Running;
                    }
                }
            }

            let mut cpus = self.cpus.lock().map_err(|_| GenshinError::Service(ServiceError::Other { code: 60, msg: "cpus".into() }))?;
            if let Some(cpu) = cpus.get_mut(&pid) {
                // Unhalt if process was just unblocked (sem_signal no longer does this)
                if cpu.is_halted() {
                    let is_ready = if let Ok(t) = self.process_table.lock() {
                        t.get(&pid).and_then(|p| p.lock().ok())
                            .map(|pcb| pcb.state == ProcessState::Ready).unwrap_or(false)
                    } else { false };
                    if is_ready { cpu.halted = false; }
                }
                if !cpu.is_halted() {
                    for _ in 0..3 {
                        if cpu.is_halted() { break; }
                        if let Err(e) = cpu.step() {
                            if pid == 1 { break; }
                            if !matches!(e, crate::error::CPUError::PageFault { .. }) {
                                cpu.halt(); break;
                            }
                        }
                        // Direct syscall handling (no bus round-trip needed)
                        if cpu.syscall_pending {
                            cpu.syscall_pending = false;
                            let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                self.handle_file_syscall(cpu, cpu.syscall_regs[0], cpu.syscall_regs[1], cpu.syscall_regs[2]);
                            }));
                            if r.is_err() {
                                eprintln!("PS: syscall panic for pid={}", pid);
                                cpu.halt();
                                break;
                            }
                            if cpu.is_halted() { break; }
                        }
                        // Handle bus-based interrupts (pagefault etc)
                        for _ in 0..5 {
                            while let Ok(env) = self.intr_rx.try_recv() {
                                if let KernelMsg::Interrupt(int) = &env.message {
                                    match int {
                                        crate::messaging::Interrupt::SyscallTrap => {
                                            // Already handled via syscall_pending above; skip
                                        }
                                        crate::messaging::Interrupt::PageFault { addr, .. } => {
                                            if let Ok(rx) = self.bus.send_request(KernelMsg::Memory(
                                                crate::messaging::MemoryRequest::PageFaultHandler {
                                                    pid: cpu.pid(), faulting_addr: *addr,
                                                    access_type: crate::messaging::AccessType::Read,
                                                })) {
                                                let _ = rx.recv_timeout(std::time::Duration::from_millis(200));
                                            }
                                            cpu.pagefault_pending = false;
                                        }
                                        _ => {}
                                    }
                                }
                            }
                            if cpu.pagefault_pending { break; }
                            std::thread::sleep(std::time::Duration::from_micros(50));
                        }
                    }
                    let s = cpu.dump_state();
                    let pname = self.process_table.lock().unwrap()
                        .get(&pid).and_then(|p| p.lock().ok())
                        .map(|pcb| pcb.name.clone()).unwrap_or_default();
                    if pname != "loop" {
                        vprintln!("CPU{}[{}]: PC={:#06x} R0={} R1={} R2={} R3={} | IC={} {}",
                            cpu_id, pid, s.pc, s.registers[0] as i64, s.registers[1] as i64,
                            s.registers[2] as i64, s.registers[3] as i64,
                            s.instruction_count, if cpu.is_halted() { "[HALTED]" } else { "" });
                    }
                }
            }

            // xv6-style: halt → Zombie (parent or init will reap)
            let is_blocked = self.process_table.lock().unwrap().get(&pid).map(|p| p.lock().ok().map(|pcb| pcb.state.is_blocked()).unwrap_or(false)).unwrap_or(false);
            if pid != 1 && !is_blocked && cpus.get(&pid).map(|c| c.is_halted()).unwrap_or(false) {
                self.scheduler.lock().unwrap().remove(pid, 1);
                if let Some(pcb) = self.process_table.lock().unwrap().get(&pid) {
                    if let Ok(mut p) = pcb.lock() {
                        if !p.state.is_terminated() {
                            p.state = ProcessState::Zombie { exit_code: 0 };
                            // Release sem 0: HALT-without-exit shouldn't deadlock waiters
                            if let Ok(mut sync) = self.sync_manager.lock() {
                                if let Some(sem) = sync.get_semaphore(0) { sem.signal(); }
                            }
                            vprintln!("PS: PID {} → Zombie", pid);
                            // Notify waiting parent
                            let mut wp = self.waiting_parents.lock().unwrap();
                            if let Some(pos) = wp.iter().position(|(cpid, _)| *cpid == pid) {
                                let (_, (_, tx)) = wp.remove(pos);
                                let exit_code = match p.state {
                                    ProcessState::Zombie { exit_code } => exit_code,
                                    _ => 0,
                                };
                                let _ = tx.send(Response::success(0, ResponseData::Integer(exit_code as u64)));
                                vprintln!("PS: notified parent, child {} exit={}", pid, exit_code);
                            }
                        }
                    }
                }
            }
        }
        // Reaper: always scan for 1 zombie per tick (regardless of idle)
        {
            let zombie_pid = if let Ok(table) = Self::lock_mutex(&self.process_table) {
                table.iter().find_map(|(&pid, pcb)| {
                    if let Ok(p) = pcb.lock() {
                        if matches!(p.state, ProcessState::Zombie { .. }) { Some(pid) } else { None }
                    } else { None }
                })
            } else { None };
            if let Some(pid) = zombie_pid {
                self.reap_process(pid);
            }
        }
        } // end for cpu_count
        Ok(())
    }

    /// Reap a zombie process: free memory, remove from table
    fn reap_process(&self, pid: Pid) {
        // Init (PID 1) must never die — root of process tree
        if pid == 1 { return; }
        // Children may still need to fork from this process's page table
        // Remove CPU
        { self.cpus.lock().unwrap().remove(&pid); }
        { self.cpus.lock().unwrap().remove(&pid); }
        // Remove from process table
        if let Ok(mut table) = Self::lock_mutex(&self.process_table) {
            table.remove(&pid);
            vprintln!("PS: PID {} reaped", pid);
        }
    }

    // ========== IPC: Message Passing Handlers ==========

    fn handle_send_message(&self, from_pid: Pid, to_pid: Pid, msg: IPCMessage) -> GenshinResult<()> {
        // Verify both processes exist
        let table = Self::lock_mutex(&self.process_table)?;
        if !table.contains_key(&from_pid) {
            return Err(GenshinError::Service(ServiceError::NotFound {
                resource_type: "Process".to_string(),
                id: from_pid.to_string(),
            }));
        }
        if !table.contains_key(&to_pid) {
            return Err(GenshinError::Service(ServiceError::NotFound {
                resource_type: "Process".to_string(),
                id: to_pid.to_string(),
            }));
        }

        // Get target process's main thread
        let tid = 1; // Main thread

        // Send via IPC manager
        drop(table);
        let mut ipc = Self::lock_mutex(&self.ipc_manager)?;
        let queue_arc = ipc.ensure_message_queue(to_pid);
        let mut queue = queue_arc.lock().map_err(|e| GenshinError::Service(ServiceError::InvalidArguments { param: "message_queue".to_string(), reason: format!("Mutex poisoned: {}", e) }))?;

        queue.send(from_pid, tid, msg).map_err(|e| GenshinError::Service(ServiceError::Other { code: 10, msg: format!("Queue error: {:?}", e) }))?;

        println!("ProcessService: Message sent from {} to {}", from_pid, to_pid);
        Ok(())
    }

    fn handle_receive_message(&self, pid: Pid, blocking: bool) -> GenshinResult<()> {
        let mut ipc = Self::lock_mutex(&self.ipc_manager)?;
        let queue_arc = ipc.get_message_queue(pid);

        let mut queue = queue_arc.lock().map_err(|e| GenshinError::Service(ServiceError::InvalidArguments { param: "message_queue".to_string(), reason: format!("Mutex poisoned: {}", e) }))?;
        if let Some(msg) = queue.receive() {
            println!("ProcessService: Process {} received message: {:?}", pid, msg.message);
            return Ok(());
        }

        if blocking {
            // Block the process
            drop(ipc);
            self.handle_block(pid, 1, BlockReason::WaitingForIo { device_id: 0 })?;
        }

        println!("ProcessService: Process {} has no messages", pid);
        Ok(())
    }

    fn handle_peek_message(&self, pid: Pid) -> GenshinResult<()> {
        let ipc = Self::lock_mutex(&self.ipc_manager)?;
        let queue_arc = ipc.get_message_queue(pid);

        let queue = queue_arc.lock().map_err(|e| GenshinError::Service(ServiceError::InvalidArguments { param: "message_queue".to_string(), reason: format!("Mutex poisoned: {}", e) }))?;
        if let Some(msg) = queue.peek() {
            println!("ProcessService: Process {} has message: {:?}", pid, msg.message);
            return Ok(());
        }

        println!("ProcessService: Process {} has no messages", pid);
        Ok(())
    }

    // ========== IPC: Shared Memory Handlers ==========

    fn handle_create_shared_memory(&self, pid: Pid, size: usize, prot: MemProt) -> GenshinResult<()> {
        let mut ipc = Self::lock_mutex(&self.ipc_manager)?;

        // Allocate physical memory (in real implementation, would request from MemoryService)
        let physical_addr = 0x1000 + (pid as PhysAddr * 0x1000); // Simplified

        let shmid = ipc.create_shared_memory(pid, size, physical_addr, prot);

        vprintln!("ProcessService: Created shared memory {} for process {} (size: {})", shmid, pid, size);
        Ok(())
    }

    fn handle_attach_shared_memory(&self, pid: Pid, shmid: u64) -> GenshinResult<()> {
        let ipc = Self::lock_mutex(&self.ipc_manager)?;

        // Allocate virtual address
        let vaddr = 0x5000 + (shmid * 0x1000);

        drop(ipc);
        let ipc = Self::lock_mutex(&self.ipc_manager)?;
        ipc.attach_shared_memory(shmid, pid, vaddr).map_err(|e| GenshinError::Service(ServiceError::Other { code: 11, msg: format!("IPC error: {:?}", e) }))?;

        println!("ProcessService: Process {} attached to shared memory {} at {:#x}", pid, shmid, vaddr);
        Ok(())
    }

    fn handle_detach_shared_memory(&self, pid: Pid, shmid: u64) -> GenshinResult<()> {
        let ipc = Self::lock_mutex(&self.ipc_manager)?;
        ipc.detach_shared_memory(shmid, pid).map_err(|e| GenshinError::Service(ServiceError::Other { code: 12, msg: format!("IPC error: {:?}", e) }))?;

        println!("ProcessService: Process {} detached from shared memory {}", pid, shmid);
        Ok(())
    }

    // ========== IPC: Synchronization Handlers ==========

    fn handle_create_semaphore(&self, pid: Pid, initial_value: u32) -> GenshinResult<()> {
        let mut sync = Self::lock_mutex(&self.sync_manager)?;
        let semid = sync.create_semaphore(pid, initial_value);

        vprintln!("ProcessService: Created semaphore {} for process {} (initial: {})", semid, pid, initial_value);
        Ok(())
    }

    fn handle_wait_semaphore(&self, pid: Pid, semid: u64) -> GenshinResult<()> {
        let sync = Self::lock_mutex(&self.sync_manager)?;
        if let Some(sem) = sync.get_semaphore(semid) {
            let result = sem.wait();
            match result {
                super::sync::SemaphoreResult::Acquired => {
                    println!("ProcessService: Process {} acquired semaphore {}", pid, semid);
                }
                super::sync::SemaphoreResult::WouldBlock => {
                    drop(sync);
                    self.handle_block(pid, 1, BlockReason::WaitingForLock { lock_addr: semid as VirtAddr })?;
                }
                _ => {
                    eprintln!("ProcessService: Semaphore wait error for {}", semid);
                }
            }
        } else {
            return Err(GenshinError::Service(ServiceError::NotFound {
                resource_type: "Semaphore".to_string(),
                id: semid.to_string(),
            }));
        }
        Ok(())
    }

    fn handle_signal_semaphore(&self, pid: Pid, semid: u64) -> GenshinResult<()> {
        let sync = Self::lock_mutex(&self.sync_manager)?;
        if let Some(sem) = sync.get_semaphore(semid) {
            let result = sem.signal();
            if let super::sync::SemaphoreResult::Released = result {
                // Check if any process was waiting
                // In real implementation, would unblock one waiter
                println!("ProcessService: Process {} signaled semaphore {}", pid, semid);
            }
        }
        Ok(())
    }

    fn handle_create_lock(&self, pid: Pid) -> GenshinResult<()> {
        let mut sync = Self::lock_mutex(&self.sync_manager)?;
        let lock_id = sync.create_mutex(pid, false);

        vprintln!("ProcessService: Created lock {} for process {}", lock_id, pid);
        Ok(())
    }

    fn handle_acquire_lock(&self, pid: Pid, lock_id: u64) -> GenshinResult<()> {
        let sync = Self::lock_mutex(&self.sync_manager)?;
        if let Some(mutex) = sync.get_mutex(lock_id) {
            let result = mutex.try_acquire(pid);
            match result {
                super::sync::MutexResult::Acquired => {
                    println!("ProcessService: Process {} acquired lock {}", pid, lock_id);
                }
                super::sync::MutexResult::WouldBlock => {
                    drop(sync);
                    self.handle_block(pid, 1, BlockReason::WaitingForLock { lock_addr: lock_id as VirtAddr })?;
                }
                _ => {
                    eprintln!("ProcessService: Lock acquire error for {}", lock_id);
                }
            }
        }
        Ok(())
    }

    fn handle_release_lock(&self, pid: Pid, lock_id: u64) -> GenshinResult<()> {
        let sync = Self::lock_mutex(&self.sync_manager)?;
        if let Some(mutex) = sync.get_mutex(lock_id) {
            let result = mutex.release(pid);
            if let super::sync::MutexResult::Released = result {
                println!("ProcessService: Process {} released lock {}", pid, lock_id);
            }
        }
        Ok(())
    }

    // ========== Process Lifecycle Handlers ==========

    fn handle_fork(&self, parent_pid: Pid) -> GenshinResult<()> {
        self.fork_impl(parent_pid)?;
        Ok(())
    }

    fn handle_fork_with_response(&self, parent_pid: Pid, envelope: &Envelope) -> GenshinResult<()> {
        let child_pid = self.fork_impl(parent_pid)?;
        envelope.respond_success(ResponseData::Pid(child_pid))?;
        Ok(())
    }

    /// Fork: clone parent memory + CPU state. Returns child PID.
    fn fork_impl(&self, parent_pid: Pid) -> GenshinResult<Pid> {
        // PID 0 = kernel: create a fresh process (no parent, no memory)
        if parent_pid == 0 {
            let child_pid = {let mut n=self.next_pid.lock().unwrap(); let p=*n; *n+=1; p};
            use crate::hardware::VirtualCPU;
            let cpu = VirtualCPU::new(self._mmu.clone(), self.bus.clone(), child_pid);
            {self.cpus.lock().unwrap().insert(child_pid, cpu);}
            let pcb = crate::services::process::PCB::new(child_pid, "init".into(), None);
            self.process_table.lock().unwrap().insert(child_pid, Arc::new(Mutex::new(pcb)));
            // Not scheduled: init has no code; exec will schedule after loading program
            vprintln!("PS: Fork 0 -> {} (fresh, not scheduled)", child_pid);
            return Ok(child_pid);
        }

        // Check parent exists for normal fork
        {let t = Self::lock_mutex(&self.process_table)?;
         if !t.contains_key(&parent_pid) {
            return Err(GenshinError::Service(ServiceError::NotFound { resource_type: "Process".into(), id: parent_pid.to_string() }));
         }}

        let child_pid = {let mut n=self.next_pid.lock().unwrap(); let p=*n; *n+=1; p};

        // Clone page table entries
        for (vaddr, _paddr, flags) in self._mmu.get_page_entries(parent_pid) {
            let rx = self.bus.send_request(KernelMsg::Memory(crate::messaging::MemoryRequest::AllocFrame{count:1, pid: child_pid}))
                .map_err(|_| GenshinError::Service(ServiceError::Other{code:90,msg:"alloc".into()}))?;
            let resp = rx.recv_timeout(std::time::Duration::from_millis(200))
                .map_err(|_| GenshinError::Service(ServiceError::Other{code:91,msg:"timeout".into()}))?;
            let new_frame = match resp.data() {
                Some(ResponseData::PhysicalAddr(a)) => *a,
                _ => continue,
            };
            // Map child page
            if let Ok(rx)=self.bus.send_request(KernelMsg::Memory(crate::messaging::MemoryRequest::MapPage{
                pid:child_pid, virt:vaddr, phys:new_frame,
                prot:crate::messaging::MemProt{readable:true,writable:true,executable:!flags.writable}
            })) { let _=rx.recv_timeout(std::time::Duration::from_millis(200)); }
            // Copy page content
            for o in 0..4096u64 {
                if let Ok(b)=self._mmu.read_u8(parent_pid, vaddr+o) {
                    let _=self._mmu.write_u8(child_pid, vaddr+o, b);
                }
            }
        }

        // Clone CPU state
        use crate::hardware::VirtualCPU;
        let mut child_cpu = VirtualCPU::new(self._mmu.clone(), self.bus.clone(), child_pid);
        if let Some(parent_cpu) = self.cpus.lock().unwrap().get(&parent_pid) {
            let st = parent_cpu.dump_state();
            child_cpu.set_pc(st.pc); child_cpu.set_sp(st.sp);
            // Copy registers (clone parent state)
            for r in 0..4 {
                if let Some(reg) = crate::hardware::Register::from_index(r) {
                    child_cpu.write_register(reg, st.registers[r]);
                }
            }
        } else { child_cpu.set_pc(0); child_cpu.set_sp(0xFFFF); }
        // xv6: child returns 0 from fork
        child_cpu.write_register(crate::hardware::Register::R0, 0);
        {self.cpus.lock().unwrap().insert(child_pid, child_cpu);}

        // Create PCB
        let mut pcb = crate::services::process::PCB::new(child_pid, format!("(fork of {})",parent_pid), Some(parent_pid));
        pcb.state = ProcessState::Ready;
        self.process_table.lock().unwrap().insert(child_pid, Arc::new(Mutex::new(pcb)));

        // Parent-child link
        {Self::lock_mutex(&self.parent_children)?.entry(parent_pid).or_default().push(child_pid);}
        // Clone file descriptors (Unix fork inheritance)
        let _ = self.bus.send_request(KernelMsg::File(crate::messaging::FileRequest::CloneFds {
            from_pid: parent_pid, to_pid: child_pid,
        })).map(|rx| rx.recv_timeout(std::time::Duration::from_millis(100)));
        // Schedule child immediately — runs from parent's PC with R0=0
        self.handle_schedule(child_pid, 1)?;
        vprintln!("PS: Fork {} -> {} (child scheduled, R0=0)", parent_pid, child_pid);
        Ok(child_pid)
    }
    /// Process pending fork requests (called from main loop, not timer)
    fn process_pending_forks(&self) {
        let pids: Vec<Pid> = {
            let mut pf = self.pending_forks.lock().unwrap();
            let pids = pf.clone();
            pf.clear();
            pids
        };
        for pid in pids {
            match self.fork_impl(pid) {
                Ok(child_pid) => {
                    // Write child_pid to parent's CPU R0
                    if let Some(cpu) = self.cpus.lock().unwrap().get_mut(&pid) {
                        cpu.write_register(crate::hardware::Register::R0, child_pid);
                    }
                    // Unblock parent
                    if let Some(p) = self.process_table.lock().unwrap().get(&pid) {
                        if let Ok(mut pcb) = p.lock() {
                            pcb.state = ProcessState::Ready;
                        }
                    }
                    self.scheduler.lock().unwrap().ready(pid, 1, 128);
                    vprintln!("PS: async fork {} -> {} (parent unblocked)", pid, child_pid);
                }
                Err(_) => {
                    // Fork failed: unblock with R0=0
                    if let Some(cpu) = self.cpus.lock().unwrap().get_mut(&pid) {
                        cpu.write_register(crate::hardware::Register::R0, 0);
                    }
                    if let Some(p) = self.process_table.lock().unwrap().get(&pid) {
                        if let Ok(mut pcb) = p.lock() {
                            pcb.state = ProcessState::Ready;
                        }
                    }
                    self.scheduler.lock().unwrap().ready(pid, 1, 128);
                }
            }
        }
    }
    fn handle_exec(&self, pid: Pid, executable: String, args: Vec<String>) -> GenshinResult<()> {
        self.exec_impl(pid, executable, args)
    }

    fn handle_exec_with_response(&self, pid: Pid, executable: String, args: Vec<String>, envelope: &Envelope) -> GenshinResult<()> {
        self.exec_impl(pid, executable, args)?;
        envelope.respond_success(ResponseData::Void)?;
        Ok(())
    }

    /// Exec: replace process memory and code
    fn exec_impl(&self, pid: Pid, executable: String, args: Vec<String>) -> GenshinResult<()> {
        // Verify process exists (even zombie is OK — exec replaces everything)
        {
            let table = Self::lock_mutex(&self.process_table)?;
            if !table.contains_key(&pid) {
                return Err(GenshinError::Service(ServiceError::NotFound{resource_type:"Process".into(),id:pid.to_string()}));
            }
        }

        let code = if let Some(c) = self.load_program(&executable) { c } else {
            let data_len = args.get(1).map(|s| s.len()).unwrap_or(0);
            let b = self.gen_builtin_program(&executable, data_len);
            if b[0] != 0xFF { b } else {
                return Err(GenshinError::Service(ServiceError::NotFound{resource_type:"Program".into(),id:executable}));
            }
        };
        // Unmap old pages
        for (vaddr, _, _) in self._mmu.get_page_entries(pid) {
            self.bus.send(KernelMsg::Memory(crate::messaging::MemoryRequest::UnmapPage{pid,virt:vaddr})).ok();
        }
        // Allocate + map new frames
        let frames = self.alloc_frames((code.len()+4095)/4096, pid)?;
        for (i, &addr) in frames.iter().enumerate() {
            if let Ok(rx)=self.bus.send_request(KernelMsg::Memory(crate::messaging::MemoryRequest::MapPage{
                pid, virt:(i*4096)as u64, phys:addr,
                prot:crate::messaging::MemProt{readable:true,writable:true,executable:true}
            })) { let _=rx.recv_timeout(std::time::Duration::from_secs(2)); }
        }
        self.write_slice_virt(pid, 0, &code);
        // Write args to process memory (0x100 = arg0, 0x200 = arg1)
        if let Some(path) = args.first() {
            self.write_slice_virt(pid, 0x100, path.as_bytes());
        }
        if args.len() > 1 {
            self.write_slice_virt(pid, 0x200, args[1].as_bytes());
        }
        // Reset CPU
        if let Some(cpu) = self.cpus.lock().unwrap().get_mut(&pid) {
            cpu.set_pc(0); cpu.set_sp(0xFFFF);
            cpu.halted = false; // unhalt: timer may have killed the empty fork
        }
        // Update PCB
        if let Some(p) = Self::lock_mutex(&self.process_table)?.get(&pid) {
            let mut pcb = p.lock().unwrap();
            pcb.name = executable.clone(); pcb.args = args; pcb.state = ProcessState::Ready;
        }
        // Re-schedule: exec resets the process, must be in ready queue
        self.handle_schedule(pid, 1)?;
        vprintln!("PS: Exec '{}' in PID {}", executable, pid);
        Ok(())
    }

    fn handle_wait_child(&self, pid: Pid, child_pid: Option<Pid>) -> GenshinResult<()> {
        let parent_children = Self::lock_mutex(&self.parent_children)?;

        if let Some(child_pid) = child_pid {
            // Check if this is our child
            if let Some(children) = parent_children.get(&pid) {
                if !children.contains(&child_pid) {
                    return Err(GenshinError::Service(ServiceError::PermissionDenied { operation: "wait".to_string(), reason: "Not a child".to_string() }));
                }
            }
        } else {
            // Wait for any child
            if let Some(children) = parent_children.get(&pid) {
                if children.is_empty() {
                    return Err(GenshinError::Service(ServiceError::NotFound {
                        resource_type: "Child".to_string(),
                        id: "any".to_string(),
                    }));
                }
            }
        }

        // Block parent until child exits
        drop(parent_children);
        self.handle_block(pid, 1, BlockReason::WaitingForChild { pid: child_pid.unwrap_or(0) })?;

        println!("ProcessService: Process {} waiting for child {:?}", pid, child_pid);
        Ok(())
    }

    /// Handle wait child with response
    fn handle_wait_child_with_response(&self, pid: Pid, child_pid: Option<Pid>, envelope: &Envelope) -> GenshinResult<()> {
        let child_pid = child_pid.ok_or_else(|| GenshinError::Service(ServiceError::InvalidArguments {
            param: "child_pid".into(), reason: "must specify child PID".into(),
        }))?;

        // Verify child is our child
        let parent_children = Self::lock_mutex(&self.parent_children)?;
        if !parent_children.get(&pid).map(|c| c.contains(&child_pid)).unwrap_or(false) {
            let _ = envelope.respond_error(MsgServiceError::PermissionDenied { operation: "wait".into() });
            return Ok(());
        }
        drop(parent_children);

        // Check if child was already reaped (not in table but was our child)
        let already_reaped = !self.process_table.lock().unwrap().contains_key(&child_pid);
        if already_reaped {
            vprintln!("PS: PID {} wait: child {} already reaped", pid, child_pid);
            let _ = envelope.respond_success(ResponseData::Integer(0));
            return Ok(());
        }

        // Check if child is already zombie
        let is_zombie = self.process_table.lock().unwrap()
            .get(&child_pid)
            .map(|p| p.lock().ok().map(|pcb| pcb.state.is_terminated()).unwrap_or(false))
            .unwrap_or(false);

        if is_zombie {
            // Reap immediately: remove child, return exit code
            self.scheduler.lock().unwrap().remove(child_pid, 1);
            if let Some(pcb) = self.process_table.lock().unwrap().remove(&child_pid) {
                let exit_code = pcb.lock().ok().and_then(|p| {
                    match p.state {
                        ProcessState::Zombie { exit_code } => Some(exit_code),
                        _ => Some(0),
                    }
                }).unwrap_or(0);
                vprintln!("PS: PID {} waited, child {} reaped (exit {})", pid, child_pid, exit_code);
                let _ = envelope.respond_success(ResponseData::Integer(exit_code as u64));
            }
        } else {
            // Child still alive: store waiting entry, respond later
            if let Some(tx) = envelope.response_channel.clone() {
                self.waiting_parents.lock().unwrap().push((child_pid, (pid, tx)));
            }
            vprintln!("PS: PID {} waiting for child {}", pid, child_pid);
        }
        Ok(())
    }

    fn handle_signal(&self, pid: Pid, signal: SignalType) -> GenshinResult<()> {
        let table = Self::lock_mutex(&self.process_table)?;
        if let Some(pcb) = table.get(&pid) {
            let mut pcb = Self::lock_mutex(pcb)?;

            match signal {
                SignalType::Terminate | SignalType::Kill => {
                    pcb.state = ProcessState::Zombie { exit_code: 0 };
                    println!("ProcessService: Killed process {} ({})", pid, signal);
                }
                SignalType::Stop => {
                    pcb.state = ProcessState::Blocked(BlockReason::WaitingForIo { device_id: 0 });
                    println!("ProcessService: Stopped process {}", pid);
                }
                SignalType::Continue => {
                    pcb.state = ProcessState::Ready;
                    println!("ProcessService: Continued process {}", pid);
                }
                _ => {
                    pcb.pending_signals.push(signal);
                    println!("ProcessService: Queued signal {} for process {}", signal, pid);
                }
            }
        } else {
            return Err(GenshinError::Service(ServiceError::NotFound {
                resource_type: "Process".to_string(),
                id: pid.to_string(),
            }));
        }
        Ok(())
    }

    fn handle_get_process_info(&self, pid: Pid) -> GenshinResult<()> {
        let table = Self::lock_mutex(&self.process_table)?;
        if let Some(pcb) = table.get(&pid) {
            let pcb = Self::lock_mutex(pcb)?;
            println!("ProcessService: Process {} info:", pid);
            println!("  Executable: {}", pcb.name);
            println!("  State: {:?}", pcb.state);
            println!("  Threads: {}", pcb.threads.len());
            println!("  Parent: {:?}", pcb.parent_pid);
        } else {
            return Err(GenshinError::Service(ServiceError::NotFound {
                resource_type: "Process".to_string(),
                id: pid.to_string(),
            }));
        }
        Ok(())
    }

    /// Handle get process info with response
    fn handle_get_process_info_with_response(&self, pid: Pid, envelope: &Envelope) -> GenshinResult<()> {
        let table = Self::lock_mutex(&self.process_table)?;
        if let Some(pcb) = table.get(&pid) {
            let pcb = Self::lock_mutex(pcb)?;

            // Create process info string
            let info = format!("PID: {}, Executable: {}, State: {:?}, Threads: {}",
                pid, pcb.name, pcb.state, pcb.threads.len());

            println!("ProcessService: Process {} info: {}", pid, info);

            envelope.respond_success(ResponseData::String(info))?;
        } else {
            envelope.respond_error(MsgServiceError::NotFound {
                resource: "Process".to_string(),
                id: pid.to_string(),
            })?;
        }

        Ok(())
    }

    fn handle_list_processes(&self) -> GenshinResult<()> {
        // Replaced by handle_list_processes_with_response
        println!("use pstree");
        Ok(())
    }

    fn handle_list_processes_with_response(&self, envelope: &Envelope) -> GenshinResult<()> {
        let table = Self::lock_mutex(&self.process_table)?;
        let parent_children = Self::lock_mutex(&self.parent_children)?;

        // Collect process info
        let mut procs: Vec<(Pid, String, String, Option<Pid>)> = Vec::new();
        for (&pid, pcb) in table.iter() {
            if let Ok(p) = pcb.lock() {
                procs.push((pid, p.name.clone(), format!("{:?}", p.state), p.parent_pid));
            }
        }
        procs.sort_by_key(|(pid, _, _, _)| *pid);

        // Build tree: find roots (parent_pid not in table, or None)
        let pids: std::collections::HashSet<Pid> = procs.iter().map(|(p,_,_,_)| *p).collect();

        let mut output = String::new();
        for (pid, name, state, ppid) in &procs {
            let is_root = ppid.map_or(true, |pp| !pids.contains(&pp));
            if is_root {
                self.format_tree(&procs, *pid, "", true, &mut output);
            }
        }

        let _ = envelope.respond_success(ResponseData::String(if output.is_empty() { "(empty)".into() } else { output }));
        Ok(())
    }

    fn format_tree(&self, procs: &[(Pid, String, String, Option<Pid>)], pid: Pid, prefix: &str, is_last: bool, output: &mut String) {
        let info = procs.iter().find(|(p,_,_,_)| p == &pid);
        if let Some((_, name, state, _)) = info {
            let connector = if is_last { "└── " } else { "├── " };
            output.push_str(&format!("{}{}PID {} [{}] {}\n", prefix, connector, pid, state, name));
        } else { return; }

        let children: Vec<Pid> = procs.iter()
            .filter(|(_, _, _, pp)| *pp == Some(pid))
            .map(|(p,_,_,_)| *p)
            .collect();
        let child_prefix = format!("{}{}", prefix, if is_last { "    " } else { "│   " });
        for (i, &child_pid) in children.iter().enumerate() {
            self.format_tree(procs, child_pid, &child_prefix, i == children.len() - 1, output);
        }
    }

    // ========== Helper Methods ==========

    /// Helper to lock a Mutex and convert PoisonError
    fn lock_mutex<T>(mutex: &Mutex<T>) -> GenshinResult<std::sync::MutexGuard<T>> {
        mutex.lock().map_err(|e| {
            GenshinError::Service(ServiceError::InvalidArguments {
                param: "mutex".to_string(),
                reason: format!("Mutex poisoned: {}", e)
            })
        })
    }

    fn load_program(&self, name: &str) -> Option<Vec<u8>> {
        // Try assembler file first
        let path = format!("programs/{}.asm", name);
        if let Ok((_, code)) = super::assembler::assemble_file(&path) {
            vprintln!("PS: Loaded {}", path);
            return Some(code);
        }
        None
    }

    fn create_process(&self, executable: &str, args: Vec<String>) -> GenshinResult<Pid> {
        // Unix-style: fork + exec into a single create
        let child_pid = self.fork_impl(0)?;
        self.exec_impl(child_pid, executable.to_string(), args)?;
        vprintln!("PS: Created {} (PID {})", executable, child_pid);
        Ok(child_pid)
    }

    fn handle_spawn(&self, program: String, params: Vec<u8>, envelope: &Envelope) -> GenshinResult<()> {
        let pname = if program == "dual" { "busy" } else if program.starts_with("dual:") { &program[5..] } else { &program };
        let count = if program.starts_with("dual") { 2 } else { 1 };
        // rwlock2: allocate shared frame + extra semaphores
        let mut shared_frame: Option<u64> = None;
        if pname == "rwlock2" {
            let mut sync = self.sync_manager.lock().unwrap();
            sync.create_semaphore(0, 1); // sem 1: mutex
            sync.create_semaphore(0, 1); // sem 2: wrt
            drop(sync);
            if let Ok(rx) = self.bus.send_request(KernelMsg::Memory(crate::messaging::MemoryRequest::AllocFrame { count: 1, pid: 0 })) {
                if let Ok(resp) = rx.recv_timeout(std::time::Duration::from_millis(200)) {
                    if let Some(crate::messaging::ResponseData::PhysicalAddr(addr)) = resp.data() {
                        shared_frame = Some(*addr);
                    }
                }
            }
        }
        let mut child_pids: Vec<Pid> = Vec::new();
        for _ in 0..count {
            let child = self.fork_impl(0)?;
            self.exec_impl(child, pname.to_string(), vec![])?;
            child_pids.push(child);
        }
        if let Some(frame_addr) = shared_frame {
            for &pid in &child_pids {
                if let Ok(rx) = self.bus.send_request(KernelMsg::Memory(crate::messaging::MemoryRequest::MapPage {
                    pid, virt: 0x10000, phys: frame_addr,
                    prot: crate::messaging::MemProt { readable: true, writable: true, executable: false },
                })) { let _ = rx.recv_timeout(std::time::Duration::from_secs(1)); }
            }
        }
        let _ = envelope.respond_success(ResponseData::Void);
        Ok(())
    }

    fn gen_builtin_program(&self, name: &str, data_len: usize) -> Vec<u8> {
        let halt = vec![0x01,0x00,0x01,0x00, 0x00,0x00,0x00,0x00, 0x80,0x00,0x00,0x00, 0x80,0x00,0x00,0x00];
        let int = vec![0x80,0x00,0x00,0x00, 0x80,0x00,0x00,0x00];
        let mov = |r: u8, v: u8| vec![0x01, r, 0x01, 0x00, v, 0x00, 0x00, 0x00];
        match name {
            "ls"|"listdir" => [&mov(0, 18)[..], &int[..], &halt[..]].concat(),
            "mkdir" => [&mov(0, 14)[..], &int[..], &halt[..]].concat(),
            "rm"|"unlink" => [&mov(0, 16)[..], &int[..], &halt[..]].concat(),
            "touch"|"open" => [&mov(1, 1)[..], &mov(0, 10)[..], &int[..], &mov(0, 11)[..], &int[..], &halt[..]].concat(),
            "cat"|"read" => [&mov(0, 10)[..], &int[..], &mov(0, 12)[..], &mov(2, 0x10)[..], &int[..], &mov(0, 11)[..], &int[..], &halt[..]].concat(),
            "write" => [&mov(1, 1)[..], &mov(0, 10)[..], &int[..], &mov(0, 13)[..], &mov(2, data_len as u8)[..], &int[..], &mov(0, 11)[..], &int[..], &halt[..]].concat(),
            "stat" => [&mov(0, 17)[..], &int[..], &halt[..]].concat(),
            "busy" => {
                // 15 MOV instructions (each 8 bytes, 1 cycle) + halt = >120 bytes, >10 quantum
                let mut prog = Vec::new();
                for i in 0..15u8 {
                    prog.extend_from_slice(&mov(1, i));
                }
                prog.extend_from_slice(&halt);
                prog
            }
            "fork" => [&mov(0, 100)[..], &int[..], &halt[..]].concat(),
            "exec" => [&mov(0, 101)[..], &int[..], &halt[..]].concat(),
            "tree" => [&mov(0, 102)[..], &int[..], &halt[..]].concat(),
            _ => vec![0xFF,0x00,0x00,0x00, 0x00,0x00,0x00,0x00],
        }
    }

    fn alloc_frames(&self, count: usize, pid: Pid) -> GenshinResult<Vec<u64>> {
        let rx = self.bus.send_request(KernelMsg::Memory(crate::messaging::MemoryRequest::AllocFrame { count, pid }))
            .map_err(|_| GenshinError::Service(ServiceError::Other { code: 90, msg: "AllocFrame failed".into() }))?;
        let resp = rx.recv_timeout(std::time::Duration::from_secs(2))
            .map_err(|_| GenshinError::Service(ServiceError::Other { code: 91, msg: "AllocFrame timeout".into() }))?;
        if let Some(ResponseData::PhysicalAddr(addr)) = resp.data() {
            let start = *addr;
            Ok((0..count as u64).map(|i| start + i * 4096).collect())
        } else {
            Err(GenshinError::Service(ServiceError::ResourceExhausted {
                resource: "memory".into(), available: 0, requested: count,
            }))
        }
    }

    fn handle_file_syscall(&self, cpu: &mut crate::hardware::VirtualCPU, r0: u64, r1: u64, r2: u64) {
        let pid = cpu.pid();
        let path = self.read_string_virt(pid, 0x100);
        use crate::messaging::{FileRequest, OpenFlags, ResponseData};
        match r0 {
            0 => {
                if pid == 1 { return; } // init never exits
                let exit_code = r1 as i32;

                // 1. Unmap and free all pages

                // 2. Unmap and free all pages
                let entries = self._mmu.get_page_entries(pid);
                let frame_count = entries.len();
                vprintln!("PS: exit({}) pid={} — freeing {} pages", exit_code, pid, frame_count);
                for (vaddr, paddr, _) in &entries {
                    self.bus.send(KernelMsg::Memory(crate::messaging::MemoryRequest::UnmapPage { pid, virt: *vaddr })).ok();
                    self.bus.send(KernelMsg::Memory(crate::messaging::MemoryRequest::FreeFrame { paddr: *paddr })).ok();
                }

                // 3. Mark as Zombie and release held semaphore 0
                if let Some(pcb) = self.process_table.lock().unwrap().get(&pid) {
                    if let Ok(mut p) = pcb.lock() {
                        p.state = ProcessState::Zombie { exit_code };
                    }
                }
                // Release sem 0: unblock any waiter (prevent permanent deadlock)
                if let Ok(mut sync) = self.sync_manager.lock() {
                    if let Some(sem) = sync.get_semaphore(0) {
                        sem.signal();
                    }
                }

                // 4. Remove from scheduler
                self.scheduler.lock().unwrap().block(pid, 1);

                // 5. Halt CPU
                cpu.halt();

                vprintln!("PS: PID {} exited with code {}", pid, exit_code);
            },
            1 => vprintln!("[PRINT] {}", r1 as i64),
            2 => {
                let data = self.read_bytes_virt(pid, r1, r2 as usize);
                let s = String::from_utf8_lossy(&data);
                vprintln!("{}", s);
            },
            10 => {
                let flags = if r1 == 0 { OpenFlags::read_only() } else { OpenFlags::create() };
                if let Ok(rx) = self.bus.send_request(KernelMsg::File(FileRequest::Open { path, flags })) {
                    if let Ok(resp) = rx.recv_timeout(std::time::Duration::from_millis(10)) {
                        if let Some(ResponseData::Fd(fd)) = resp.data() {
                            cpu.write_register(crate::hardware::Register::R1, *fd as u64);
                        }
                    }
                }
            }
            11 => { self.bus.send(KernelMsg::File(FileRequest::Close { fd: r1 as u32 })).ok(); }
            12 => {
                // Loop-read until EOF for cat-like behavior
                let fd = r1 as u32;
                let mut offset = 0u64;
                let chunk = std::cmp::min(r2, 256) as usize;
                loop {
                    if let Ok(rx) = self.bus.send_request(KernelMsg::File(FileRequest::Read { fd, offset, buf: 0, size: chunk })) {
                        if let Ok(resp) = rx.recv_timeout(std::time::Duration::from_millis(50)) {
                            if let Some(ResponseData::Bytes(data)) = resp.data() {
                                if data.is_empty() { break; }
                                print!("{}", String::from_utf8_lossy(&data));
                                use std::io::Write;
                                let _ = std::io::stdout().flush();
                                offset += data.len() as u64;
                                if data.len() < chunk { break; }
                            } else { break; }
                        } else { break; }
                    } else { break; }
                }
                println!(); // newline after file content
            }
            13 => {
                let data = self.read_bytes_virt(pid, 0x200, r2 as usize);
                self.bus.send(KernelMsg::File(FileRequest::WriteData { fd: r1 as u32, data })).ok();
            }
            14 => { self.bus.send(KernelMsg::File(FileRequest::CreateDirectory { path })).ok(); }
            16 => { self.bus.send(KernelMsg::File(FileRequest::Unlink { path })).ok(); }
            17 => { self.bus.send(KernelMsg::File(FileRequest::Stat { path })).ok(); }
            18 => {
                if let Ok(rx) = self.bus.send_request(KernelMsg::File(FileRequest::ListDir { path })) {
                    if let Ok(resp) = rx.recv_timeout(std::time::Duration::from_millis(10)) {
                        if let Some(ResponseData::StringList(entries)) = resp.data() {
                            for e in entries { println!("{}", e); }
                        }
                    }
                }
            }
            // ========== Process syscalls (triggered by CPU INT) ==========
            100 => {
                // FORK: synchronous clone — child runs from same PC with R0=0
                match self.fork_impl(pid) {
                    Ok(child_pid) => {
                        cpu.write_register(crate::hardware::Register::R0, child_pid);
                    }
                    Err(_) => {
                        cpu.write_register(crate::hardware::Register::R0, 0);
                    }
                }
            }
            101 => {
                // EXEC: replace current process with program at 0x100
                let prog = self.read_string_virt(pid, 0x100);
                let _ = self.exec_impl(pid, prog, vec![]);
                // If exec succeeded, CPU was reset. If failed, continue.
            }
            102 => {
                // TREE: recursively list directory tree
                let path = self.read_string_virt(pid, 0x100);
                let tree = self.build_tree(&path);
                for line in &tree { println!("{}", line); }
            }
            // ── Synchronization syscalls ──
            200 => {
                // SEM_CREATE: returns sem_id in R1
                let mut sync = self.sync_manager.lock().unwrap();
                let sem_id = sync.create_semaphore(pid, 1);
                cpu.write_register(crate::hardware::Register::R1, sem_id);
            }
            201 => {
                // SEM_WAIT(sem_id): block if count=0
                let sem_id = r1;
                let blocked = {
                    let sync = self.sync_manager.lock().unwrap();
                    if let Some(sem) = sync.get_semaphore(sem_id) {
                        sem.wait() != super::sync::SemaphoreResult::Acquired
                    } else { false }
                };
                if blocked {
                    if let Ok(table) = self.process_table.lock() {
                        if let Some(p) = table.get(&pid) {
                            if let Ok(mut pcb) = p.lock() {
                                pcb.state = ProcessState::Blocked(BlockReason::WaitingForLock { lock_addr: sem_id });
                            }
                        }
                    }
                    if let Ok(mut sched) = self.scheduler.lock() { sched.block(pid, 1); }
                    cpu.halt();
                }
            }
            202 => {
                let sem_id = r1;
                // Find blocked waiter BEFORE signaling
                let waiter: Option<Pid> = {
                    if let Ok(table) = self.process_table.lock() {
                        table.iter().find_map(|(&p, pcb)| {
                            if let Ok(pcb) = pcb.lock() {
                                if let ProcessState::Blocked(BlockReason::WaitingForLock { lock_addr }) = &pcb.state {
                                    if *lock_addr == sem_id { return Some(p); }
                                }
                            }
                            None
                        })
                    } else { None }
                };
                if let Some(wpid) = waiter {
                    if let Ok(table) = self.process_table.lock() {
                        if let Some(p) = table.get(&wpid) {
                            if let Ok(mut pcb) = p.lock() { pcb.state = ProcessState::Ready; }
                        }
                    }
                    if let Ok(mut sched) = self.scheduler.lock() { sched.ready(wpid, 1, 128); }
                    // Defer unhalt: scheduler handles this when process is picked next tick
                    // (Avoids deadlock: cpus lock is held by handle_timer_interrupt)
                    vprintln!("PS: sem_signal {} transferred to PID {}", sem_id, wpid);
                } else {
                    if let Ok(mut sync) = self.sync_manager.lock() {
                        if let Some(sem) = sync.get_semaphore(sem_id) { sem.signal(); }
                    }
                }
            }
            203 => {
                // LOCK_CREATE: returns lock_id in R1
                let mut sync = self.sync_manager.lock().unwrap();
                let lock_id = sync.create_mutex(pid, false);
                cpu.write_register(crate::hardware::Register::R1, lock_id);
            }
            204 => {
                // LOCK_ACQUIRE(lock_id): block if locked
                let lock_id = r1;
                let blocked = {
                    let sync = self.sync_manager.lock().unwrap();
                    if let Some(mutex) = sync.get_mutex(lock_id) {
                        mutex.try_acquire(pid) != super::sync::MutexResult::Acquired
                    } else { false }
                };
                if blocked {
                    if let Some(p) = self.process_table.lock().unwrap().get(&pid) {
                        if let Ok(mut pcb) = p.lock() {
                            pcb.state = ProcessState::Blocked(BlockReason::WaitingForLock { lock_addr: lock_id });
                        }
                    }
                    self.scheduler.lock().unwrap().block(pid, 1);
                    cpu.halt();
                }
            }
            205 => {
                let lock_id = r1;
                let waiter: Option<Pid> = {
                    let table = self.process_table.lock().unwrap();
                    table.iter().find_map(|(&p, pcb)| {
                        if let Ok(pcb) = pcb.lock() {
                            if let ProcessState::Blocked(BlockReason::WaitingForLock { lock_addr }) = &pcb.state {
                                if *lock_addr == lock_id { return Some(p); }
                            }
                        }
                        None
                    })
                };
                if let Some(wpid) = waiter {
                    if let Some(p) = self.process_table.lock().unwrap().get(&wpid) {
                        if let Ok(mut pcb) = p.lock() { pcb.state = ProcessState::Ready; }
                    }
                    self.scheduler.lock().unwrap().ready(wpid, 1, 128);
                    if let Some(c) = self.cpus.lock().unwrap().get_mut(&wpid) { c.halted = false; }
                } else {
                    let mut sync = self.sync_manager.lock().unwrap();
                    if let Some(mutex) = sync.get_mutex(lock_id) { mutex.release(pid); }
                }
            }
            // ── Device syscalls ──
            208 => {
                // Device open: request clipboard device
                println!("[DEVICE] pid={} requests clipboard", pid);
                cpu.write_register(crate::hardware::Register::R1, 0); // device_id = 0
            }
            209 => {
                // Device close: release clipboard device
                println!("[DEVICE] pid={} releases clipboard", pid);
            }
            210 => {
                let max_size = r1 as usize;
                if let Ok(rx) = self.bus.send_request(KernelMsg::Device(crate::messaging::DeviceRequest::ClipboardGet { max_size })) {
                    if let Ok(resp) = rx.recv_timeout(std::time::Duration::from_millis(200)) {
                        if let Some(ResponseData::Bytes(data)) = resp.data() {
                            for (i, &b) in data.iter().enumerate() {
                                let _ = self._mmu.write_u8(pid, 0x200 + i as u64, b);
                            }
                            cpu.write_register(crate::hardware::Register::R2, data.len() as u64);
                        }
                    }
                }
            }
            211 => {
                let size = r2 as usize;
                let data = self.read_bytes_virt(pid, 0x200, size);
                self.bus.send(KernelMsg::Device(crate::messaging::DeviceRequest::ClipboardSet { data })).ok();
            }
            _ => {}
        }
    }

    fn build_tree(&self, path: &str) -> Vec<String> {
        let mut result = Vec::new();
        result.push(path.to_string());
        self.build_tree_recursive(path, "", &mut result);
        result
    }

    fn build_tree_recursive(&self, path: &str, prefix: &str, result: &mut Vec<String>) {
        if let Ok(rx) = self.bus.send_request(KernelMsg::File(crate::messaging::FileRequest::ListDir { path: path.to_string() })) {
            if let Ok(resp) = rx.recv_timeout(std::time::Duration::from_millis(200)) {
                if let Some(ResponseData::StringList(entries)) = resp.data() {
                    let mut entries = entries.clone();
                    entries.sort();
                    for (i, entry) in entries.iter().enumerate() {
                        let is_last = i == entries.len() - 1;
                        let connector = if is_last { "\u{2514}\u{2500}\u{2500} " } else { "\u{251c}\u{2500}\u{2500} " };
                        let epath = if path == "/" { format!("/{}", entry) } else { format!("{}/{}", path, entry) };
                        result.push(format!("{}{}{}", prefix, connector, entry));
                        let new_prefix = format!("{}{}", prefix, if is_last { "    " } else { "\u{2502}   " });
                        self.build_tree_recursive(&epath, &new_prefix, result);
                    }
                }
            }
        }
    }

    fn read_string_virt(&self, pid: u64, vaddr: u64) -> String {
        let mut buf = vec![0u8; 256];
        for (i, b) in buf.iter_mut().enumerate() { *b = self._mmu.read_u8(pid, vaddr + i as u64).unwrap_or(0); }
        let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
        String::from_utf8_lossy(&buf[..end]).to_string()
    }

    fn read_bytes_virt(&self, pid: u64, vaddr: u64, len: usize) -> Vec<u8> {
        let mut buf = vec![0u8; len];
        for (i, b) in buf.iter_mut().enumerate() { *b = self._mmu.read_u8(pid, vaddr + i as u64).unwrap_or(0); }
        buf
    }

    fn write_slice_virt(&self, pid: u64, vaddr: u64, data: &[u8]) {
        for (i, &b) in data.iter().enumerate() { self._mmu.write_u8(pid, vaddr + i as u64, b).ok(); }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::messaging::LockedBus;

    fn make_service() -> ProcessService {
        let bus = Arc::new(LockedBus::new());
        let mem = crate::hardware::PhysicalMemory::new(1024 * 1024);
        let mmu = Arc::new(crate::hardware::MMU::new(mem.clone(), 4096));
        let (prx, _) = crossbeam_channel::unbounded();
        let (irx, _) = crossbeam_channel::unbounded();
        ProcessService::new(bus, mem, mmu, prx, irx)
    }

    #[test]
    fn test_process_service_creation() {
        let bus = Arc::new(LockedBus::new());
        let service = make_service();

        // Service should be created successfully
        assert_eq!(service.process_table.lock().unwrap().len(), 0);
    }

    #[test]
    fn test_create_process() {
        let bus = Arc::new(LockedBus::new());
        let service = make_service();

        // Create process directly via create_process method
        let pid = service.create_process("/bin/test", vec!["--help".to_string()]).unwrap();

        // Process should be created
        let table = service.process_table.lock().unwrap();
        assert!(!table.is_empty());
        assert!(table.contains_key(&pid));
    }

    #[test]
    fn test_process_schedule() {
        let bus = Arc::new(LockedBus::new());
        let service = make_service();

        // Create a process first
        let pid = service.create_process("/bin/sched", Vec::new()).unwrap();

        // Schedule it
        let result = service.handle_schedule(pid, 1);
        assert!(result.is_ok());

        // Check scheduler state
        let scheduler = service.scheduler.lock().unwrap();
        assert!(scheduler.ready_count() > 0);
    }

    #[test]
    fn test_process_block_unblock() {
        let bus = Arc::new(LockedBus::new());
        let service = make_service();

        // Create a process
        let pid = service.create_process("/bin/block", Vec::new()).unwrap();

        // Block it
        let result = service.handle_block(pid, 1, BlockReason::WaitingForIo { device_id: 1 });
        assert!(result.is_ok());

        // Check state
        let table = service.process_table.lock().unwrap();
        let pcb = table.get(&pid).unwrap().lock().unwrap();
        assert_eq!(pcb.state, ProcessState::Blocked(BlockReason::WaitingForIo { device_id: 0 }));
        drop(pcb);
        drop(table);

        // Unblock it
        let result = service.handle_unblock(pid, 1);
        assert!(result.is_ok());

        // Check state again
        let table = service.process_table.lock().unwrap();
        let pcb = table.get(&pid).unwrap().lock().unwrap();
        assert_eq!(pcb.state, ProcessState::Ready);
    }

    #[test]
    fn test_send_receive_message() {
        let bus = Arc::new(LockedBus::new());
        let service = make_service();

        // Create two processes
        let pid1 = service.create_process("/bin/sender", Vec::new()).unwrap();
        let pid2 = service.create_process("/bin/receiver", Vec::new()).unwrap();

        // Send message
        let msg = IPCMessage::Text { data: "Hello!".to_string() };
        let result = service.handle_send_message(pid1, pid2, msg);
        assert!(result.is_ok());

        // Receive message
        let result = service.handle_receive_message(pid2, false);
        assert!(result.is_ok());
    }

    #[test]
    fn test_shared_memory() {
        let bus = Arc::new(LockedBus::new());
        let service = make_service();

        let pid = service.create_process("/bin/shm", Vec::new()).unwrap();

        // Create shared memory
        let result = service.handle_create_shared_memory(pid, 4096, MemProt::read_write());
        assert!(result.is_ok());

        // Attach to it (shmid would be 1)
        let result = service.handle_attach_shared_memory(pid, 1);
        assert!(result.is_ok());

        // Detach from it
        let result = service.handle_detach_shared_memory(pid, 1);
        assert!(result.is_ok());
    }

    #[test]
    fn test_semaphore() {
        let bus = Arc::new(LockedBus::new());
        let service = make_service();

        let pid = service.create_process("/bin/sem", Vec::new()).unwrap();

        // Create semaphore
        let result = service.handle_create_semaphore(pid, 2);
        assert!(result.is_ok());

        // Wait on semaphore (semid would be 1)
        let result = service.handle_wait_semaphore(pid, 1);
        assert!(result.is_ok());

        // Signal semaphore
        let result = service.handle_signal_semaphore(pid, 1);
        assert!(result.is_ok());
    }

    #[test]
    fn test_mutex_lock() {
        let bus = Arc::new(LockedBus::new());
        let service = make_service();

        let pid = service.create_process("/bin/mutex", Vec::new()).unwrap();

        // Create mutex
        let result = service.handle_create_lock(pid);
        assert!(result.is_ok());

        // Acquire lock (lock_id would be 1)
        let result = service.handle_acquire_lock(pid, 1);
        assert!(result.is_ok());

        // Release lock
        let result = service.handle_release_lock(pid, 1);
        assert!(result.is_ok());
    }

    #[test]
    fn test_signal_handling() {
        let bus = Arc::new(LockedBus::new());
        let service = make_service();

        let pid = service.create_process("/bin/signal", Vec::new()).unwrap();

        // Send stop signal
        let result = service.handle_signal(pid, SignalType::Stop);
        assert!(result.is_ok());

        // Check state
        let table = service.process_table.lock().unwrap();
        let pcb = table.get(&pid).unwrap().lock().unwrap();
        assert_eq!(pcb.state, ProcessState::Blocked(BlockReason::WaitingForIo { device_id: 0 }));
        drop(pcb);
        drop(table);

        // Send continue signal
        let result = service.handle_signal(pid, SignalType::Continue);
        assert!(result.is_ok());

        // Check state again
        let table = service.process_table.lock().unwrap();
        let pcb = table.get(&pid).unwrap().lock().unwrap();
        assert_eq!(pcb.state, ProcessState::Ready);
    }

    #[test]
    fn test_fork_process() {
        let bus = Arc::new(LockedBus::new());
        let service = make_service();

        let parent_pid = service.create_process("/bin/parent", Vec::new()).unwrap();

        // Fork
        let result = service.handle_fork(parent_pid);
        assert!(result.is_ok());

        // Check parent-child relationship
        let parent_children = service.parent_children.lock().unwrap();
        assert!(parent_children.contains_key(&parent_pid));
        assert!(!parent_children.get(&parent_pid).unwrap().is_empty());
    }

    #[test]
    fn test_exec_process() {
        let bus = Arc::new(LockedBus::new());
        let service = make_service();

        let pid = service.create_process("/bin/original", Vec::new()).unwrap();

        // Exec new program
        let result = service.handle_exec(
            pid,
            "/bin/new".to_string(),
            vec!["--arg1".to_string(), "--arg2".to_string()],
        );
        assert!(result.is_ok());

        // Check PCB was updated
        let table = service.process_table.lock().unwrap();
        let pcb = table.get(&pid).unwrap().lock().unwrap();
        assert_eq!(pcb.name, "/bin/new");
        assert_eq!(pcb.args.len(), 2);
    }

    #[test]
    fn test_query_state() {
        let bus = Arc::new(LockedBus::new());
        let service = make_service();

        let pid = service.create_process("/bin/query", Vec::new()).unwrap();

        // Query state
        let result = service.handle_query_state(pid);
        assert!(result.is_ok());

        // Query non-existent process
        let result = service.handle_query_state(9999);
        assert!(result.is_err());
    }

    #[test]
    fn test_list_processes() {
        let bus = Arc::new(LockedBus::new());
        let service = make_service();

        // Create some processes
        let _ = service.create_process("/bin/p1", Vec::new()).unwrap();
        let _ = service.create_process("/bin/p2", Vec::new()).unwrap();

        // List processes
        let result = service.handle_list_processes();
        assert!(result.is_ok());
    }

    #[test]
    fn test_timer_interrupt() {
        let bus = Arc::new(LockedBus::new());
        let service = make_service();

        // Create a process
        let _ = service.create_process("/bin/timer", Vec::new()).unwrap();

        // Simulate timer interrupt
        let result = service.handle_timer_interrupt();
        assert!(result.is_ok());
    }

    #[test]
    fn test_get_process_info() {
        let bus = Arc::new(LockedBus::new());
        let service = make_service();

        let pid = service.create_process("/bin/info", Vec::new()).unwrap();

        // Get process info
        let result = service.handle_get_process_info(pid);
        assert!(result.is_ok());

        // Get info for non-existent process
        let result = service.handle_get_process_info(9999);
        assert!(result.is_err());
    }

    #[test]
    fn test_peek_message() {
        let bus = Arc::new(LockedBus::new());
        let service = make_service();

        let pid = service.create_process("/bin/peek", Vec::new()).unwrap();

        // Peek at empty queue
        let result = service.handle_peek_message(pid);
        assert!(result.is_ok());

        // Send a message
        let msg = IPCMessage::Text { data: "Test".to_string() };
        let _ = service.handle_send_message(pid, pid, msg);

        // Peek again
        let result = service.handle_peek_message(pid);
        assert!(result.is_ok());
    }

    #[test]
    fn test_multiple_thread_schedule() {
        let bus = Arc::new(LockedBus::new());
        let service = make_service();

        let pid = service.create_process("/bin/threads", Vec::new()).unwrap();

        // Schedule multiple threads
        let result1 = service.handle_schedule(pid, 1);
        let result2 = service.handle_schedule(pid, 2);
        let result3 = service.handle_schedule(pid, 3);

        assert!(result1.is_ok());
        assert!(result2.is_ok());
        assert!(result3.is_ok());

        // Check scheduler
        let scheduler = service.scheduler.lock().unwrap();
        assert_eq!(scheduler.ready_count(), 4); // 1 from create_process + 3 from handle_schedule
    }
}
