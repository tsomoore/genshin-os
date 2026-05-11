; rwlock.asm — Semaphore demo with GLOBAL semaphore (ID 0)
; ===========================================================
; Uses pre-created global semaphore 0 (value=1)
; All processes share it — demonstrates mutual exclusion
;
; dual rwlock  →  2 processes, 1 semaphore, 2 CPUs
; Output: [ and ] alternate, never two [ in a row

MOV R1, #0      ; global semaphore ID
MOV R2, #0      ; init counter

; === loop ===
MOV R0, #201    ; sem_wait(R1=0) — BLOCKS if other proc holds it
INT 0x80

MOV R0, #1      ; print '['
MOV R1, #0x5B
INT 0x80

MOV R1, #0      ; restore sem_id
MOV R0, #202    ; sem_signal(R1=0) — UNBLOCKS waiter
INT 0x80

MOV R0, #1      ; print ']'
MOV R1, #0x5D
INT 0x80

MOV R1, #0      ; restore sem_id
ADD R2, #1
JMP 0x18        ; loop back to sem_wait (MOV R0,#201)

HALT
