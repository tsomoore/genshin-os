; stat.asm — file info (syscall 17)
; Reads path from 0x100
MOV R0, #17
INT 0x80
HALT
