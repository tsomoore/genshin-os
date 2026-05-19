; syncdemo.asm — Semaphore mutual exclusion demo (sem_wait/sem_signal)
; Semaphore 0 is pre-created as a binary semaphore (initial value = 1)
; sem_wait blocks if count=0; sem_signal transfers ownership to waiter

MOV R1, #0      ; 0x00: sem_id = 0
MOV R0, #201    ; 0x08: sem_wait(0) — P operation
INT 0x80        ; 0x10: enter critical section

MOV R0, #1      ; 0x18: print '['
MOV R1, #0x5B   ; 0x20: character '[' = 91
INT 0x80        ; 0x28

MOV R2, #50     ; 0x30: spin delay (50 iterations)
SUB R2, #1      ; 0x38
CMP R2, #0      ; 0x40
JNZ 0x38        ; 0x48

MOV R1, #0      ; 0x50: sem_signal(0) — V operation
MOV R0, #202    ; 0x58: exit critical section, wake waiter
INT 0x80        ; 0x60

MOV R0, #1      ; 0x68: print ']'
MOV R1, #0x5D   ; 0x70: character ']' = 93
INT 0x80        ; 0x78

JMP 0x00        ; 0x80: infinite loop
