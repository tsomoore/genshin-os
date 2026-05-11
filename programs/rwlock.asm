; rwlock.asm — Semaphore demo with global semaphore (ID 0)
; Output: [ and ] alternate — mutual exclusion proof

MOV R1, #0      ; global sem ID

; === loop (JMP target here) ===
MOV R0, #201    ; 0x10 sem_wait(R1=0) — BLOCKS
INT 0x80        ; 0x18

MOV R0, #1      ; 0x20 print '['
MOV R1, #0x5B   ; 0x28
INT 0x80        ; 0x30

MOV R1, #0      ; 0x38 restore sem_id
MOV R0, #202    ; 0x40 sem_signal(R1=0) — UNBLOCKS
INT 0x80        ; 0x48

MOV R0, #1      ; 0x50 print ']'
MOV R1, #0x5D   ; 0x58
INT 0x80        ; 0x60

MOV R1, #0      ; 0x68 restore sem_id
JMP 0x08        ; 0x70 loop → MOV R0, #201

HALT            ; 0x78
