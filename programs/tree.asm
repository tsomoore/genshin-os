; tree.asm — recursive directory tree (syscall 102)
; Result written to 0x200 by kernel, size in R2
MOV R0, #102    ; tree → result at 0x200, size in R2
INT 0x80
MOV R1, #0x200  ; address of result string
MOV R0, #2      ; print_str(addr, size)
INT 0x80
HALT
