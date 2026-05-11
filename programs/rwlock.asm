
; rwlock.asm — 3 iterations per process, then HALT
MOV R1, #0      ; global sem ID
; --- Iteration 1 ---
MOV R0, #201    ; sem_wait(R1=0)
INT 0x80
MOV R0, #1      ; print "["
MOV R1, #0x5B
INT 0x80
MOV R1, #0      ; restore sem_id
MOV R0, #202    ; sem_signal(R1=0)
INT 0x80
MOV R0, #1      ; print "]"
MOV R1, #0x5D
INT 0x80
MOV R1, #0      ; restore sem_id
; --- Iteration 2 ---
MOV R0, #201    ; sem_wait(R1=0)
INT 0x80
MOV R0, #1      ; print "["
MOV R1, #0x5B
INT 0x80
MOV R1, #0      ; restore sem_id
MOV R0, #202    ; sem_signal(R1=0)
INT 0x80
MOV R0, #1      ; print "]"
MOV R1, #0x5D
INT 0x80
MOV R1, #0      ; restore sem_id
; --- Iteration 3 ---
MOV R0, #201    ; sem_wait(R1=0)
INT 0x80
MOV R0, #1      ; print "["
MOV R1, #0x5B
INT 0x80
MOV R1, #0      ; restore sem_id
MOV R0, #202    ; sem_signal(R1=0)
INT 0x80
MOV R0, #1      ; print "]"
MOV R1, #0x5D
INT 0x80
MOV R1, #0      ; restore sem_id

HALT