; tree.asm — recursive directory tree (syscall 102)
; Reads starting path from 0x100
MOV R0, #102
INT 0x80
HALT
