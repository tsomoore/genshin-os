; rwlock.asm — Semaphore mutual exclusion (with real loop!)
; Uses CMP+JNZ for a proper counted loop
MOV R1, #0      ; global sem ID
MOV R3, #100    ; loop counter: 100 iterations (visible in pmon)

; === loop ===
MOV R0, #201    ; sem_wait(R1=0)
INT 0x80

MOV R0, #1      ; print '['
MOV R1, #0x5B
INT 0x80

MOV R1, #0      ; restore sem_id
MOV R0, #202    ; sem_signal(R1=0)
INT 0x80

MOV R0, #1      ; print ']'
MOV R1, #0x5D
INT 0x80

MOV R1, #0      ; restore sem_id
SUB R3, #1      ; counter--
CMP R3, #0      ; done?
JNZ 0x10        ; if not, loop back to MOV R0,#201

HALT
