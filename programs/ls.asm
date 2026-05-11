; ls.asm — list directory (syscall 18)
; Reads path from 0x100 (written by exec)
MOV R0, #18
INT 0x80
HALT
