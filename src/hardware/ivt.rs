// Interrupt Vector Table (IVT) Definition
//
// 曾国藩曰：
// "凡事预则立，不预则废。"
// 中断向量表乃系统应对突发之预备，当详加规划，不可有误。

use std::fmt;

/// Interrupt vector numbers following x86 convention
///
/// 曾国藩曰：
/// "名不正则言不顺，言不顺则事不成。"
/// 中断向量之命名，当循古制，方不至于混淆。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InterruptVector {
    /// 0x00 - Divide by zero exception
    DivideByZero = 0x00,

    /// 0x0E - Page fault exception
    PageFault = 0x0E,

    /// 0x20 - Timer interrupt (IRQ0)
    Timer = 0x20,

    /// 0x80 - System call interrupt
    Syscall = 0x80,
}

impl InterruptVector {
    /// Get interrupt vector number
    pub fn as_u8(self) -> u8 {
        self as u8
    }

    /// Create from u8
    pub fn from_u8(val: u8) -> Option<Self> {
        match val {
            0x00 => Some(Self::DivideByZero),
            0x0E => Some(Self::PageFault),
            0x20 => Some(Self::Timer),
            0x80 => Some(Self::Syscall),
            _ => None,
        }
    }
}

impl fmt::Display for InterruptVector {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DivideByZero => write!(f, "DivideByZero (0x00)"),
            Self::PageFault => write!(f, "PageFault (0x0E)"),
            Self::Timer => write!(f, "Timer (0x20)"),
            Self::Syscall => write!(f, "Syscall (0x80)"),
        }
    }
}

/// Interrupt type classification
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InterruptType {
    /// Exception (synchronous, caused by instruction execution)
    Exception,
    /// Interrupt (asynchronous, external event)
    Interrupt,
    /// Trap (synchronous, intentional like syscall)
    Trap,
}

impl InterruptType {
    /// Get the type for a given vector
    pub fn for_vector(vector: InterruptVector) -> Self {
        match vector {
            InterruptVector::DivideByZero => Self::Exception,
            InterruptVector::PageFault => Self::Exception,
            InterruptVector::Timer => Self::Interrupt,
            InterruptVector::Syscall => Self::Trap,
        }
    }
}

/// Interrupt Vector Table
///
/// 曾国藩曰：
/// "治大国若烹小鲜，需备齐调料。"
/// IVT 乃系统之调料箱，各类中断当一一对应，不可遗漏。
#[derive(Debug, Clone)]
pub struct IVT;

impl IVT {
    /// Get vector information
    pub fn get_vector(vector: u8) -> Option<(InterruptVector, InterruptType)> {
        InterruptVector::from_u8(vector)
            .map(|v| (v, InterruptType::for_vector(v)))
    }

    /// Get all defined vectors
    pub fn all_vectors() -> &'static [(InterruptVector, InterruptType)] {
        VECTORS
    }

    /// Format vector as string
    pub fn format_vector(vector: u8) -> String {
        match InterruptVector::from_u8(vector) {
            Some(v) => format!("{} ({:#04x})", v, vector),
            None => format!("Unknown ({:#04x})", vector),
        }
    }
}

/// Static interrupt vector table
const VECTORS: &[(InterruptVector, InterruptType)] = &[
    (InterruptVector::DivideByZero, InterruptType::Exception),
    (InterruptVector::PageFault, InterruptType::Exception),
    (InterruptVector::Timer, InterruptType::Interrupt),
    (InterruptVector::Syscall, InterruptType::Trap),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vector_values() {
        assert_eq!(InterruptVector::DivideByZero.as_u8(), 0x00);
        assert_eq!(InterruptVector::PageFault.as_u8(), 0x0E);
        assert_eq!(InterruptVector::Timer.as_u8(), 0x20);
        assert_eq!(InterruptVector::Syscall.as_u8(), 0x80);
    }

    #[test]
    fn test_vector_roundtrip() {
        assert_eq!(
            InterruptVector::from_u8(0x00),
            Some(InterruptVector::DivideByZero)
        );
        assert_eq!(
            InterruptVector::from_u8(0x0E),
            Some(InterruptVector::PageFault)
        );
        assert_eq!(
            InterruptVector::from_u8(0x20),
            Some(InterruptVector::Timer)
        );
        assert_eq!(
            InterruptVector::from_u8(0x80),
            Some(InterruptVector::Syscall)
        );
        assert_eq!(InterruptVector::from_u8(0xFF), None);
    }

    #[test]
    fn test_interrupt_types() {
        assert_eq!(
            InterruptType::for_vector(InterruptVector::DivideByZero),
            InterruptType::Exception
        );
        assert_eq!(
            InterruptType::for_vector(InterruptVector::Timer),
            InterruptType::Interrupt
        );
        assert_eq!(
            InterruptType::for_vector(InterruptVector::Syscall),
            InterruptType::Trap
        );
    }

    #[test]
    fn test_ivt_format() {
        assert_eq!(
            IVT::format_vector(0x00),
            "DivideByZero (0x00) (0x00)"
        );
        assert_eq!(
            IVT::format_vector(0xFF),
            "Unknown (0xff)"
        );
    }
}
