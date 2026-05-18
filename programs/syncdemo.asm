; syncdemo.asm — Infinite semaphore demo (never exits, use kill to stop)

MOV R1, #0      ; 0x00: sem_id=0
MOV R0, #201    ; 0x08: sem_wait(0)
INT 0x80        ; 0x10

MOV R0, #1      ; 0x18: print '['
MOV R1, #0x5B   ; 0x20
INT 0x80        ; 0x28

MOV R2, #50     ; 0x30: spin delay
SUB R2, #1      ; 0x38
CMP R2, #0      ; 0x40
JNZ 0x38        ; 0x48

MOV R1, #0      ; 0x50: sem_signal(0)
MOV R0, #202    ; 0x58
INT 0x80        ; 0x60

MOV R0, #1      ; 0x68: print ']'
MOV R1, #0x5D   ; 0x70
INT 0x80        ; 0x78

JMP 0x00        ; 0x80: infinite loop
