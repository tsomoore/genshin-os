; schedemo.asm — Pure scheduling demo (CPU-bound infinite loop)
; Multiple instances show round-robin time slice switching.

MOV R0, #0      ; 0x00: counter = 0
MOV R1, #255    ; 0x08: max value

ADD R0, #1      ; 0x10: counter++
CMP R0, R1      ; 0x18: compare
JNZ 0x10        ; 0x20: loop

MOV R0, #0      ; 0x28: reset
JMP 0x10        ; 0x30: back to ADD
