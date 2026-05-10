// Virtual CPU (VCPU) with Mock ISA
//
// 曾国藩曰：
// "为学之道，莫先于穷理；穷理之要，必在于读书。"
// CPU 乃系统之大脑，指令执行如读书，当逐字逐句，不可跳越。

use std::sync::Arc;
use std::fmt;

use crate::hardware::mmu::MMU;
use crate::error::{CPUError, AccessType as ErrorAccessType};
use crate::messaging::{KernelMsg, Pid, VirtAddr};
use crate::messaging::Interrupt;
use crate::messaging::MessageBus;
use crate::hardware::ivt::InterruptVector;

/// General-purpose registers
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Register {
    R0 = 0,
    R1 = 1,
    R2 = 2,
    R3 = 3,
}

impl Register {
    /// Get register index
    pub fn index(self) -> usize {
        self as usize
    }

    /// Create from index
    pub fn from_index(idx: usize) -> Option<Self> {
        match idx {
            0 => Some(Self::R0),
            1 => Some(Self::R1),
            2 => Some(Self::R2),
            3 => Some(Self::R3),
            _ => None,
        }
    }

    pub fn count() -> usize {
        4
    }
}

impl fmt::Display for Register {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::R0 => write!(f, "R0"),
            Self::R1 => write!(f, "R1"),
            Self::R2 => write!(f, "R2"),
            Self::R3 => write!(f, "R3"),
        }
    }
}

/// CPU flags register
///
/// 曾国藩曰：
/// "志之所向，无坚不入；锐兵精甲，不能御也。"
/// 标志位记录运算结果，如人之志向，指示后续行为。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CPUFlags {
    /// Zero flag (result was zero)
    pub zero: bool,

    /// Sign flag (result was negative)
    pub sign: bool,

    /// Overflow flag (signed overflow occurred)
    pub overflow: bool,

    /// Carry flag (unsigned overflow occurred)
    pub carry: bool,
}

impl CPUFlags {
    pub fn new() -> Self {
        Self::default()
    }

    /// Update flags based on arithmetic result
    pub fn from_result(result: u64, a: u64, b: u64) -> Self {
        let is_zero = result == 0;
        let is_negative = (result as i64) < 0;
        let did_carry = result < a; // For addition, this is simplified
        let did_overflow = {
            // Simplified overflow detection
            let a_signed = a as i64;
            let b_signed = b as i64;
            let result_signed = result as i64;
            // Overflow occurred if signs don't match the operation
            (a_signed >= 0 && b_signed >= 0 && result_signed < 0) ||
            (a_signed < 0 && b_signed < 0 && result_signed >= 0)
        };

        Self {
            zero: is_zero,
            sign: is_negative,
            overflow: did_overflow,
            carry: did_carry,
        }
    }
}

/// Instruction types for the Mock ISA
///
/// 曾国藩曰：
/// "兵法云：知己知彼，百战不殆。"
/// 指令集乃 CPU 之兵法，当熟记于心，方能运用自如。
#[derive(Debug, Clone, PartialEq)]
pub enum Instruction {
    /// MOV dst, src - Move value
    /// dst can be register, src can be register or immediate
    Mov {
        dst: Register,
        src: Operand,
    },

    /// ADD dst, src - Add src to dst
    Add {
        dst: Register,
        src: Operand,
    },

    /// SUB dst, src - Subtract src from dst
    Sub {
        dst: Register,
        src: Operand,
    },

    /// MUL dst, src - Multiply dst by src
    Mul {
        dst: Register,
        src: Operand,
    },

    /// DIV dst, src - Divide dst by src
    /// **CRITICAL**: If src == 0, this MUST trigger DivideByZero exception
    Div {
        dst: Register,
        src: Operand,
    },

    /// JMP addr - Jump to address
    Jmp {
        addr: VirtAddr,
    },

    /// INT vec - Software interrupt/trap
    /// Saves context and sends KernelMsg::Syscall
    Int {
        vector: u8,
    },

    /// LOAD.B dst, addr — load byte from memory into register
    Load {
        dst: Register,
        addr: Operand,
    },

    /// STORE.B src, addr — store byte from register to memory
    Store {
        src: Register,
        addr: Operand,
    },

    /// HALT - Stop execution
    Halt,
}

/// Operand can be register or immediate value
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Operand {
    Reg(Register),
    Imm(u64),
}

impl fmt::Display for Instruction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Mov { dst, src } => write!(f, "MOV {}, {}", dst, src),
            Self::Add { dst, src } => write!(f, "ADD {}, {}", dst, src),
            Self::Sub { dst, src } => write!(f, "SUB {}, {}", dst, src),
            Self::Mul { dst, src } => write!(f, "MUL {}, {}", dst, src),
            Self::Div { dst, src } => write!(f, "DIV {}, {}", dst, src),
            Self::Jmp { addr } => write!(f, "JMP {:#x}", addr),
            Self::Load { dst, addr } => write!(f, "LOAD {}, [{}]", dst, addr),
            Self::Store { src, addr } => write!(f, "STORE [{}], {}", addr, src),
            Self::Int { vector } => write!(f, "INT {:#x}", vector),
            Self::Halt => write!(f, "HALT"),
        }
    }
    }

impl fmt::Display for Operand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Reg(reg) => write!(f, "{}", reg),
            Self::Imm(val) => write!(f, "#{:#x}", val),
        }
    }
}

/// Virtual CPU state
#[derive(Debug, Clone)]
pub struct CPUState {
    pub registers: [u64; 4],
    pub pc: u64,
    pub sp: u64,
    pub flags: CPUFlags,
    pub halted: bool,
    pub current_pid: Pid,
    pub instruction_count: u64,
    pub pagefault_pending: bool,
}

/// Virtual CPU
///
/// Implements the complete fetch-decode-execute cycle.
/// When an exception occurs, it reports via the message bus
/// and halts execution waiting for kernel handling.
///
/// 曾国藩曰：
/// "行军之道，步步为营；治事之道，循规蹈矩。"
/// CPU 执行指令当步步为营，取指、译码、执行、查中断，
/// 缺一不可，乱一必败。
pub struct VirtualCPU {
    /// General-purpose registers R0-R3
    registers: [u64; 4],

    /// Program counter
    pc: u64,

    /// Stack pointer
    sp: u64,

    /// Flags register
    flags: CPUFlags,

    /// Current process ID (for address translation)
    current_pid: Pid,

    /// Halted state
    pub halted: bool,
    /// Pending syscall: registers at time of INT
    pub syscall_pending: bool,
    pub syscall_regs: [u64; 4],

    /// MMU for memory access
    mmu: Arc<MMU>,

    /// Message bus for reporting exceptions
    bus: Arc<dyn MessageBus>,

    /// Instruction counter (for stats/debugging)
    instruction_count: u64,
    pub pagefault_pending: bool,
}

impl VirtualCPU {
    /// Create a new virtual CPU
    pub fn new(mmu: Arc<MMU>, bus: Arc<dyn MessageBus>, pid: Pid) -> Self {
        Self {
            registers: [0; 4],
            pc: 0,
            sp: 0xFFFF_FFFF_FFFF_F000, // Default stack near top of address space
            flags: CPUFlags::new(),
            current_pid: pid,
            halted: false,
            mmu,
            bus,
            instruction_count: 0,
            pagefault_pending: false,
            syscall_pending: false,
            syscall_regs: [0; 4],
        }
    }

    /// Get current process ID
    pub fn pid(&self) -> Pid {
        self.current_pid
    }

    /// Set current process ID (for context switch)
    pub fn set_pid(&mut self, pid: Pid) {
        self.current_pid = pid;
    }

    /// Read register value
    pub fn read_register(&self, reg: Register) -> u64 {
        self.registers[reg.index()]
    }

    /// Write register value
    pub fn write_register(&mut self, reg: Register, value: u64) {
        self.registers[reg.index()] = value;
    }

    /// Get program counter
    pub fn pc(&self) -> u64 {
        self.pc
    }

    /// Set program counter
    pub fn set_pc(&mut self, pc: u64) {
        self.pc = pc;
    }

    /// Get stack pointer
    pub fn sp(&self) -> u64 {
        self.sp
    }

    /// Set stack pointer
    pub fn set_sp(&mut self, sp: u64) {
        self.sp = sp;
    }

    /// Get flags
    pub fn flags(&self) -> CPUFlags {
        self.flags
    }

    /// Check if CPU is halted
    pub fn is_halted(&self) -> bool {
        self.halted
    }

    /// Halt the CPU
    pub fn halt(&mut self) {
        self.halted = true;
    }

    /// Reset the CPU (clear all state)
    pub fn reset(&mut self) {
        self.registers = [0; 4];
        self.pc = 0;
        self.sp = 0xFFFF_FFFF_FFFF_F000;
        self.flags = CPUFlags::new();
        self.halted = false;
        self.instruction_count = 0;
        self.pagefault_pending = false;
    }

    /// Execute one instruction: fetch-decode-execute cycle
    ///
    /// 曾国藩曰：
    /// "读书之法，循序而渐进；执行之法，按步而推进。"
    /// 每一步都不能跳过，否则必出大错。
    pub fn step(&mut self) -> Result<(), CPUError> {
        if self.halted { return Err(CPUError::Halted); }
        if self.pagefault_pending { return Ok(()); }

        let saved_pc = self.pc;
        match self.fetch_instruction() {
            Ok(instr) => {
                match self.execute_instruction(instr) {
                    Ok(()) => {
                        self.instruction_count += 1;
                        Ok(())
                    }
                    Err(CPUError::PageFault { vaddr, .. }) => {
                        self.pc = saved_pc; let msg = KernelMsg::Interrupt(Interrupt::PageFault {
                            addr: vaddr, access_type: crate::messaging::AccessType::Read,
                        });
                        let _ = self.bus.send(msg);
                        self.pagefault_pending = true;
                        Ok(())
                    }
                    Err(e) => Err(e),
                }
            }
            Err(CPUError::PageFault { vaddr, .. }) => {
                self.pc = saved_pc; let msg = KernelMsg::Interrupt(Interrupt::PageFault {
                    addr: vaddr, access_type: crate::messaging::AccessType::Read,
                });
                let _ = self.bus.send(msg);
                self.pagefault_pending = true;
                Ok(())
            }
            Err(e) => Err(e),
        }
    }

    /// Fetch instruction from memory at PC
    fn fetch_instruction(&mut self) -> Result<Instruction, CPUError> {
        // Encoding: [opcode:1][dst:1][src_type:1][pad:1][value:4] = 8 bytes

        let opcode = self.fetch_byte()?;
        let dst_reg = self.fetch_byte()?;
        let src_type = self.fetch_byte()?;
        let _pad = self.fetch_byte()?;
        let src_value = self.fetch_qword()?;

        // Decode opcode
        let instr = match opcode {
            0x01 => {
                // MOV
                let dst = Register::from_index(dst_reg as usize)
                    .ok_or(CPUError::InvalidRegister { index: dst_reg as usize })?;
                let src = if src_type == 0 {
                    Operand::Reg(
                        Register::from_index(src_value as usize)
                            .ok_or(CPUError::InvalidRegister { index: src_value as usize })?
                    )
                } else {
                    Operand::Imm(src_value)
                };
                Instruction::Mov { dst, src }
            }
            0x02 => {
                // ADD
                let dst = Register::from_index(dst_reg as usize)
                    .ok_or(CPUError::InvalidRegister { index: dst_reg as usize })?;
                let src = if src_type == 0 {
                    Operand::Reg(
                        Register::from_index(src_value as usize)
                            .ok_or(CPUError::InvalidRegister { index: src_value as usize })?
                    )
                } else {
                    Operand::Imm(src_value)
                };
                Instruction::Add { dst, src }
            }
            0x03 => {
                // SUB
                let dst = Register::from_index(dst_reg as usize)
                    .ok_or(CPUError::InvalidRegister { index: dst_reg as usize })?;
                let src = if src_type == 0 {
                    Operand::Reg(
                        Register::from_index(src_value as usize)
                            .ok_or(CPUError::InvalidRegister { index: src_value as usize })?
                    )
                } else {
                    Operand::Imm(src_value)
                };
                Instruction::Sub { dst, src }
            }
            0x04 => {
                // MUL
                let dst = Register::from_index(dst_reg as usize)
                    .ok_or(CPUError::InvalidRegister { index: dst_reg as usize })?;
                let src = if src_type == 0 {
                    Operand::Reg(
                        Register::from_index(src_value as usize)
                            .ok_or(CPUError::InvalidRegister { index: src_value as usize })?
                    )
                } else {
                    Operand::Imm(src_value)
                };
                Instruction::Mul { dst, src }
            }
            0x05 => {
                // DIV - CRITICAL: Must check for zero
                let dst = Register::from_index(dst_reg as usize)
                    .ok_or(CPUError::InvalidRegister { index: dst_reg as usize })?;
                let src = if src_type == 0 {
                    Operand::Reg(
                        Register::from_index(src_value as usize)
                            .ok_or(CPUError::InvalidRegister { index: src_value as usize })?
                    )
                } else {
                    Operand::Imm(src_value)
                };
                Instruction::Div { dst, src }
            }
            0x06 => {
                // LOAD dst, addr
                let dst = Register::from_index(dst_reg as usize)
                    .ok_or(CPUError::InvalidRegister { index: dst_reg as usize })?;
                let addr = if src_type == 0 {
                    Operand::Reg(Register::from_index(src_value as usize)
                        .ok_or(CPUError::InvalidRegister { index: src_value as usize })?)
                } else {
                    Operand::Imm(src_value)
                };
                Instruction::Load { dst, addr }
            }
            0x07 => {
                // STORE src, addr
                let src = Register::from_index(dst_reg as usize)
                    .ok_or(CPUError::InvalidRegister { index: dst_reg as usize })?;
                let addr = if src_type == 0 {
                    Operand::Reg(Register::from_index(src_value as usize)
                        .ok_or(CPUError::InvalidRegister { index: src_value as usize })?)
                } else {
                    Operand::Imm(src_value)
                };
                Instruction::Store { src, addr }
            }
            0x10 => {
                Instruction::Jmp { addr: src_value }
            }
            0x80 => {
                // INT
                Instruction::Int { vector: src_value as u8 }
            }
            0xFF => {
                // HALT
                Instruction::Halt
            }
            _ => {
                return Err(CPUError::InvalidInstruction { pc: self.pc - 11, opcode });
            }
        };

        Ok(instr)
    }

    /// Fetch a single byte from memory
    fn fetch_byte(&mut self) -> Result<u8, CPUError> {
        let value = self.mmu.read_u8(self.current_pid, self.pc)
            .map_err(|_| CPUError::PageFault {
                vaddr: self.pc,
                access_type: ErrorAccessType::Read,
            })?;
        self.pc += 1;
        Ok(value)
    }

    /// Fetch a 64-bit qword from memory
    fn fetch_qword(&mut self) -> Result<u64, CPUError> {
        let value = self.mmu.read_u32(self.current_pid, self.pc)
            .map_err(|_| CPUError::PageFault {
                vaddr: self.pc,
                access_type: ErrorAccessType::Read,
            })? as u64;
        self.pc += 4;
        Ok(value)
    }

    /// Execute a single instruction
    ///
    /// 曾国藩曰：
    /// "临事而惧，好谋而成。"
    /// 执行指令当如临深渊，每一步都需谨慎检查。
    fn execute_instruction(&mut self, instr: Instruction) -> Result<(), CPUError> {
        match instr {
            Instruction::Mov { dst, src } => {
                let value = self.read_operand(src);
                self.write_register(dst, value);
            }

            Instruction::Add { dst, src } => {
                let dst_val = self.read_register(dst);
                let src_val = self.read_operand(src);
                let result = dst_val.wrapping_add(src_val);
                self.flags = CPUFlags::from_result(result, dst_val, src_val);
                self.write_register(dst, result);
            }

            Instruction::Sub { dst, src } => {
                let dst_val = self.read_register(dst);
                let src_val = self.read_operand(src);
                let result = dst_val.wrapping_sub(src_val);
                self.flags = CPUFlags::from_result(result, dst_val, src_val);
                self.write_register(dst, result);
            }

            Instruction::Mul { dst, src } => {
                let dst_val = self.read_register(dst);
                let src_val = self.read_operand(src);
                let result = dst_val.wrapping_mul(src_val);
                self.flags = CPUFlags::from_result(result, dst_val, src_val);
                self.write_register(dst, result);
            }

            Instruction::Div { dst, src } => {
                let dst_val = self.read_register(dst);
                let src_val = self.read_operand(src);

                // 曾国藩曰：
                // "除数为零，乃数学之大忌，系统之大忌。"
                // 此处必须严查，否则系统必崩。
                //
                // CRITICAL: Check for division by zero
                if src_val == 0 {
                    self.report_divide_by_zero();
                    return Err(CPUError::DivideByZero { pc: self.pc });
                }

                let result = dst_val / src_val;
                self.flags = CPUFlags::from_result(result, dst_val, src_val);
                self.write_register(dst, result);
            }

            Instruction::Load { dst, addr } => {
                let address = self.read_operand(addr);
                let value = self.mmu.read_u8(self.current_pid, address)
                    .map_err(|_| CPUError::PageFault { vaddr: address, access_type: ErrorAccessType::Read })? as u64;
                self.write_register(dst, value);
            }

            Instruction::Store { src, addr } => {
                let address = self.read_operand(addr);
                let value = self.read_register(src);
                self.mmu.write_u8(self.current_pid, address, value as u8)
                    .map_err(|_| CPUError::PageFault { vaddr: address, access_type: ErrorAccessType::Write })?;
            }

            Instruction::Jmp { addr } => {
                self.pc = addr;
            }

            Instruction::Int { vector } => {
                self.handle_software_interrupt(vector)?;
            }

            Instruction::Halt => {
                self.halt();
            }
        }

        Ok(())
    }

    /// Read operand value (register or immediate)
    fn read_operand(&self, operand: Operand) -> u64 {
        match operand {
            Operand::Reg(reg) => self.read_register(reg),
            Operand::Imm(val) => val,
        }
    }

    /// Handle software interrupt (trap)
    ///
    /// 曾国藩曰：
    /// "遇大事当静气，方寸不乱，方能成事。"
    /// 处理中断当保存现场，从容不迫。
    fn handle_software_interrupt(&mut self, vector: u8) -> Result<(), CPUError> {
        // Check if it's a syscall (INT 0x80)
        if vector == InterruptVector::Syscall.as_u8() {
            // Save registers for direct handler access
            self.syscall_regs = self.registers;
            self.syscall_pending = true;

            // Also send via bus for traditional path (backward compat)
            let msg = KernelMsg::Interrupt(Interrupt::SyscallTrap);
            let _ = self.bus.send(msg);

            Ok(())
        } else {
            // Unknown interrupt vector
            let msg = KernelMsg::Interrupt(Interrupt::HardwareFailure {
                component: format!("CPU: Unknown interrupt vector {:#x}", vector),
            });
            let _ = self.bus.send(msg);

            Ok(())
        }
    }

    /// Report divide-by-zero exception
    ///
    /// 曾国藩曰：
    /// "祸患常积于忽微，而智勇多困于所溺。"
    /// 除零错误虽小，然必导致系统崩溃，不可不慎。
    fn report_divide_by_zero(&self) {
        let msg = KernelMsg::Interrupt(Interrupt::HardwareFailure {
            component: format!("CPU: Divide by zero in PID {} at PC {:#x}", self.current_pid, self.pc),
        });
        let _ = self.bus.send(msg);
    }

    /// Save CPU state (for context switching)
    pub fn save_state(&self) -> CPUState {
        CPUState {
            registers: self.registers.clone(),
            pc: self.pc,
            sp: self.sp,
            flags: self.flags,
            halted: self.halted,
            current_pid: self.current_pid,
            instruction_count: self.instruction_count,
            pagefault_pending: self.pagefault_pending,
        }
    }

    /// Restore CPU state (for context switching)
    pub fn restore_state(&mut self, state: CPUState) {
        self.registers = state.registers;
        self.pc = state.pc;
        self.sp = state.sp;
        self.flags = state.flags;
        self.halted = state.halted;
        self.current_pid = state.current_pid;
        self.instruction_count = state.instruction_count;
        self.pagefault_pending = state.pagefault_pending;
    }

    /// Dump CPU state for debugging/TUI display
    ///
    /// 曾国藩曰：
    /// "每日检点自身，知其得失；每日检点 CPU，知其状态。"
    pub fn dump_state(&self) -> CPUState {
        self.save_state()
    }
}

impl fmt::Debug for VirtualCPU {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("VirtualCPU")
            .field("pid", &self.current_pid)
            .field("registers", &self.registers)
            .field("pc", &format!("{:#x}", self.pc))
            .field("sp", &format!("{:#x}", self.sp))
            .field("flags", &self.flags)
            .field("halted", &self.halted)
            .field("instruction_count", &self.instruction_count)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hardware::memory::PhysicalMemory;
    use crate::messaging::LockedBus;

    #[test]
    fn test_cpu_creation() {
        let mem = PhysicalMemory::new(1024 * 1024);
        let mmu = Arc::new(MMU::new(mem, 4096));
        let bus = Arc::new(LockedBus::new());
        let cpu = VirtualCPU::new(mmu, bus, 1);

        assert_eq!(cpu.pid(), 1);
        assert!(!cpu.is_halted());
    }

    #[test]
    fn test_register_access() {
        let mem = PhysicalMemory::new(1024 * 1024);
        let mmu = Arc::new(MMU::new(mem, 4096));
        let bus = Arc::new(LockedBus::new());
        let mut cpu = VirtualCPU::new(mmu, bus, 1);

        cpu.write_register(Register::R0, 0xDEADBEEF);
        assert_eq!(cpu.read_register(Register::R0), 0xDEADBEEF);
    }

    #[test]
    fn test_halt() {
        let mem = PhysicalMemory::new(1024 * 1024);
        let mmu = Arc::new(MMU::new(mem, 4096));
        let bus = Arc::new(LockedBus::new());
        let mut cpu = VirtualCPU::new(mmu, bus, 1);

        assert!(!cpu.is_halted());
        cpu.halt();
        assert!(cpu.is_halted());
    }

    #[test]
    fn test_pc_sp() {
        let mem = PhysicalMemory::new(1024 * 1024);
        let mmu = Arc::new(MMU::new(mem, 4096));
        let bus = Arc::new(LockedBus::new());
        let mut cpu = VirtualCPU::new(mmu, bus, 1);

        cpu.set_pc(0x1000);
        assert_eq!(cpu.pc(), 0x1000);

        cpu.set_sp(0x2000);
        assert_eq!(cpu.sp(), 0x2000);
    }

    #[test]
    fn test_save_restore_state() {
        let mem = PhysicalMemory::new(1024 * 1024);
        let mmu = Arc::new(MMU::new(mem, 4096));
        let bus = Arc::new(LockedBus::new());
        let mut cpu = VirtualCPU::new(mmu.clone(), bus.clone(), 1);

        // Set some state
        cpu.write_register(Register::R0, 0x1111);
        cpu.write_register(Register::R1, 0x2222);
        cpu.set_pc(0x3000);
        cpu.set_sp(0x4000);

        // Save state
        let state = cpu.save_state();

        // Modify CPU
        cpu.write_register(Register::R0, 0x9999);
        cpu.set_pc(0x5000);

        // Restore state
        cpu.restore_state(state.clone());

        // Verify restored
        assert_eq!(cpu.read_register(Register::R0), 0x1111);
        assert_eq!(cpu.read_register(Register::R1), 0x2222);
        assert_eq!(cpu.pc(), 0x3000);
        assert_eq!(cpu.sp(), 0x4000);
    }
}
