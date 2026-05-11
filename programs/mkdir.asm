; mkdir.asm — create directory (syscall 14)
; Reads path from 0x100
MOV R0, #14
INT 0x80
HALT
