; rm.asm — delete file (syscall 16)
; Reads path from 0x100
MOV R0, #16
INT 0x80
HALT
