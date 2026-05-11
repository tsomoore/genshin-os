; rwlock.asm — Semaphore mutual exclusion demo
; ============================================
; 4 processes share 1 semaphore (value=1)
; Each: wait → print '[' → signal → print ']'
; With 2 CPUs, you see interleaved [ and ]
;
; Layout (8 bytes per instruction):
;   Setup:     0x00 - 0x28
;   Init R2:   0x30
;   sem_wait:  0x38 ← loop target
;   print '[': 0x48 - 0x58
;   sem_signal:0x60 - 0x68
;   print ']': 0x70 - 0x80
;   ADD+JMP:   0x88 - 0x98

MOV R0, #200    ; 0x00 sem_create (value=1)
INT 0x80        ; 0x08 R1 = sem_id

MOV R0, #100    ; 0x10 fork #1 → 2 procs
INT 0x80        ; 0x18

MOV R0, #100    ; 0x20 fork #2 → 4 procs
INT 0x80        ; 0x28

MOV R2, #0      ; 0x30 init counter (once)

; === Loop starts here ===
MOV R0, #201    ; 0x38 sem_wait — BLOCKS
INT 0x80        ; 0x40

MOV R0, #1      ; 0x48 print '['
MOV R1, #0x5B   ; 0x50
INT 0x80        ; 0x58

MOV R0, #202    ; 0x60 sem_signal — UNBLOCKS
INT 0x80        ; 0x68

MOV R0, #1      ; 0x70 print ']'
MOV R1, #0x5D   ; 0x78
INT 0x80        ; 0x80

ADD R2, #1      ; 0x88 iter++
JMP 0x38        ; 0x90 loop

HALT            ; 0x98 (never reached)
