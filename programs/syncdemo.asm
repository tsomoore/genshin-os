; syncdemo.asm — Semaphore mutual exclusion demo (5 iterations)
MOV R3, #5      ; 0x00: counter

MOV R1, #0      ; 0x08: sem_id=0
MOV R0, #201    ; 0x10: sem_wait(0)
INT 0x80        ; 0x18

MOV R0, #1      ; 0x20: print '['
MOV R1, #0x5B   ; 0x28
INT 0x80        ; 0x30

MOV R2, #10     ; 0x38: spin delay
SUB R2, #1      ; 0x40
CMP R2, #0      ; 0x48
JNZ 0x40        ; 0x50

MOV R1, #0      ; 0x58: sem_signal(0)
MOV R0, #202    ; 0x60
INT 0x80        ; 0x68

MOV R0, #1      ; 0x70: print ']'
MOV R1, #0x5D   ; 0x78
INT 0x80        ; 0x80

SUB R3, #1      ; 0x88: loop
CMP R3, #0      ; 0x90
JNZ 0x08        ; 0x98

MOV R0, #0      ; 0xA0: exit(0) — releases semaphore
MOV R1, #0      ; 0xA8
INT 0x80        ; 0xB0
