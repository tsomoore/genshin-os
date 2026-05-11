; exec.asm — exec syscall (R0=101)
; Reads new program name from 0x100
MOV R0, #101
INT 0x80
HALT
