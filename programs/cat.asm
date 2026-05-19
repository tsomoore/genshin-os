; cat.asm — read file and print (syscall 10 + 12 + 11)
; Result written to 0x200 by kernel, size in R2
MOV R0, #10     ; open (path at 0x100)
INT 0x80        ; fd in R1
MOV R3, R1      ; save fd to R3 before overwriting R1
MOV R0, #12     ; read(fd, size=4096) → data at 0x200, size in R2
MOV R2, #4096
INT 0x80        ; R1 is still fd here
MOV R1, #0x200  ; address of data (R1 overwritten!)
MOV R0, #2      ; print_str(addr, size)
INT 0x80        ; R2 already has size from read
MOV R1, R3      ; restore fd from R3
MOV R0, #11     ; close
INT 0x80
HALT
