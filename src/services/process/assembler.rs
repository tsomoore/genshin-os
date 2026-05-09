// Mock ISA Assembler
//
// Converts .asm files to machine code for the mock ISA.
// Instruction format: [opcode:1][dst:1][src_type:1][pad:1][value:4 LE] = 8 bytes

use std::collections::HashMap;

/// Opcode constants matching VirtualCPU's fetch_instruction
const OP_MOV: u8 = 0x01;
const OP_ADD: u8 = 0x02;
const OP_SUB: u8 = 0x03;
const OP_MUL: u8 = 0x04;
const OP_DIV: u8 = 0x05;
const OP_LOAD: u8 = 0x06;
const OP_STORE: u8 = 0x07;
const OP_JMP: u8 = 0x10;
const OP_INT: u8 = 0x80;
const OP_HALT: u8 = 0xFF;

/// Source type: register or immediate
const SRC_REG: u8 = 0;
const SRC_IMM: u8 = 1;

/// Register index map
fn reg_index(name: &str) -> Option<u8> {
    match name.to_uppercase().as_str() {
        "R0" => Some(0),
        "R1" => Some(1),
        "R2" => Some(2),
        "R3" => Some(3),
        _ => None,
    }
}

/// Parse an operand: register name or immediate value
fn parse_operand(s: &str) -> Option<(u8, u64)> {
    let s = s.trim();
    if let Some(stripped) = s.strip_prefix('#') {
        // Immediate: #42 or #0x2A
        let val = if let Some(hex) = stripped.strip_prefix("0x").or_else(|| stripped.strip_prefix("0X")) {
            u64::from_str_radix(hex, 16).ok()?
        } else {
            stripped.parse::<u64>().ok()?
        };
        Some((SRC_IMM, val))
    } else if let Some(idx) = reg_index(s) {
        Some((SRC_REG, idx as u64))
    } else {
        None
    }
}

/// Encode one instruction into 8 bytes
fn encode(opcode: u8, dst_reg: u8, src_type: u8, value: u64) -> Vec<u8> {
    let mut bytes = vec![opcode, dst_reg, src_type, 0x00]; // padding byte
    bytes.extend_from_slice(&(value as u32).to_le_bytes());
    bytes
}

/// Parse a single line of assembly
fn parse_line(line: &str, line_num: usize) -> Option<Vec<u8>> {
    let line = line.trim();

    // Skip empty lines and comments
    if line.is_empty() || line.starts_with(';') || line.starts_with("//") {
        return None;
    }

    // Remove inline comment
    let line = match line.find(';') {
        Some(pos) => line[..pos].trim(),
        None => line,
    };

    // Split by whitespace, handle commas
    let parts: Vec<&str> = line.split_whitespace().collect();
    if parts.is_empty() {
        return None;
    }

    let mnemonic = parts[0].to_uppercase();

    match mnemonic.as_str() {
        "HALT" => Some(encode(OP_HALT, 0, 0, 0)),

        "MOV" | "ADD" | "SUB" | "MUL" | "DIV" => {
            if parts.len() < 3 {
                eprintln!("  asm:{}: {} requires 2 operands", line_num, mnemonic);
                return None;
            }
            let dst = reg_index(parts[1].trim_end_matches(','))?;
            let (src_type, value) = parse_operand(parts[2])?;

            let opcode = match mnemonic.as_str() {
                "MOV" => OP_MOV,
                "ADD" => OP_ADD,
                "SUB" => OP_SUB,
                "MUL" => OP_MUL,
                "DIV" => OP_DIV,
                _ => unreachable!(),
            };
            Some(encode(opcode, dst, src_type, value))
        }

        "JMP" => {
            if parts.len() < 2 {
                eprintln!("  asm:{}: JMP requires an address", line_num);
                return None;
            }
            let addr = parse_immediate(parts[1])?;
            Some(encode(OP_JMP, 0, SRC_IMM, addr))
        }

        "LOAD" => {
            if parts.len() < 3 { eprintln!("  asm:{}: LOAD requires dst and [addr]", line_num); return None; }
            let dst = reg_index(parts[1].trim_end_matches(','))?;
            let addr_str = parts[2];
            let (addr_type, addr_val) = parse_mem_operand(addr_str)?;
            Some(encode(OP_LOAD, dst, addr_type, addr_val))
        }

        "STORE" => {
            if parts.len() < 3 { eprintln!("  asm:{}: STORE requires [addr] and src", line_num); return None; }
            let addr_str = parts[1].trim_end_matches(',');
            let src = reg_index(parts[2].trim_end_matches(','))?;
            let (addr_type, addr_val) = parse_mem_operand(addr_str)?;
            Some(encode(OP_STORE, src, addr_type, addr_val))
        }

        "INT" => {
            if parts.len() < 2 {
                eprintln!("  asm:{}: INT requires a vector", line_num);
                return None;
            }
            let vector = parse_immediate(parts[1])?;
            Some(encode(OP_INT, 0, SRC_IMM, vector))
        }

        _ => {
            eprintln!("  asm:{}: unknown mnemonic '{}'", line_num, mnemonic);
            None
        }
    }
}

fn parse_mem_operand(s: &str) -> Option<(u8, u64)> {
    let s = s.trim();
    // Format: [0x100] or [R0]
    let inner = s.strip_prefix('[').and_then(|t| t.strip_suffix(']'))?;
    if let Some(idx) = reg_index(inner) {
        Some((SRC_REG, idx as u64)) // SRC_REG = 0 means register addressing
    } else {
        Some((SRC_IMM, parse_immediate(inner)?))
    }
}

fn parse_immediate(s: &str) -> Option<u64> {
    let s = s.trim().trim_start_matches('#');
    if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        u64::from_str_radix(hex, 16).ok()
    } else {
        s.parse::<u64>().ok()
    }
}

/// Assemble a .asm source string into machine code bytes
pub fn assemble(source: &str) -> Result<Vec<u8>, String> {
    let mut code = Vec::new();

    for (i, line) in source.lines().enumerate() {
        if let Some(bytes) = parse_line(line, i + 1) {
            code.extend(bytes);
        }
    }

    if code.is_empty() {
        return Err("no instructions generated".to_string());
    }

    Ok(code)
}

/// Assemble from file path, return (name, code)
pub fn assemble_file(path: &str) -> Result<(String, Vec<u8>), String> {
    let source = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read '{}': {}", path, e))?;

    let name = std::path::Path::new(path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown")
        .to_string();

    let code = assemble(&source)?;
    Ok((name, code))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_assemble_add() {
        let src = "MOV R0, #10\nMOV R1, #20\nADD R2, R0\nADD R2, R1\nHALT\n";
        let code = assemble(src).unwrap();
        assert_eq!(code.len(), 5 * 8); // 5 instructions × 8 bytes

        // Check first instruction: MOV R0, #10
        assert_eq!(code[0], OP_MOV);
        assert_eq!(code[1], 0); // R0
        assert_eq!(code[2], SRC_IMM);
        assert_eq!(code[3], 0); // padding
        assert_eq!(u32::from_le_bytes([code[4], code[5], code[6], code[7]]), 10);
    }

    #[test]
    fn test_comments_and_empty_lines() {
        let src = "; header\nMOV R0, #1\n\n; middle\nHALT\n";
        let code = assemble(src).unwrap();
        assert_eq!(code.len(), 2 * 8);
    }

    #[test]
    fn test_load_store() {
        let src = "MOV R0, #72\nSTORE [0x200], R0\nLOAD R1, [0x200]\nHALT\n";
        let code = assemble(src).unwrap();
        assert_eq!(code.len(), 4 * 8);
        // STORE [0x200], R0 (op=0x07, src=R0, addr_type=IMM, addr_val=0x200)
        assert_eq!(code[8], OP_STORE);
        assert_eq!(code[9], 0); // R0
        assert_eq!(code[10], SRC_IMM);
        assert_eq!(u32::from_le_bytes([code[12],code[13],code[14],code[15]]), 0x200);
        // LOAD R1, [0x200] (op=0x06, dst=R1, addr_type=IMM, addr_val=0x200)
        assert_eq!(code[16], OP_LOAD);
        assert_eq!(code[17], 1); // R1
        assert_eq!(code[18], SRC_IMM);
        assert_eq!(u32::from_le_bytes([code[20],code[21],code[22],code[23]]), 0x200);
    }
}
