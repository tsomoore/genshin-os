; cmpdemo.asm — CMP + JZ + JNZ showcase for minigdb
; Step through with 's' to see flags change and branches taken.

MOV R0, #5      ; 0x00: counter = 5
MOV R1, #3      ; 0x08: threshold = 3

SUB R0, #1      ; 0x10: counter-- (4)
CMP R0, #0      ; 0x18: counter == 0?
JZ  0x58        ; 0x20: if yes → exit

CMP R0, R1      ; 0x28: counter vs 3
JNZ 0x50        ; 0x30: if counter != 3 → skip print

MOV R0, #1      ; 0x38: print_int (counter == 3)
MOV R1, #99     ; 0x40
INT 0x80        ; 0x48

JMP 0x10        ; 0x50: loop

MOV R0, #0      ; 0x58: exit(0)
MOV R1, #0      ; 0x60
INT 0x80        ; 0x68
