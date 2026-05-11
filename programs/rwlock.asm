; rwlock.asm — Semaphore mutual exclusion demo
; 10 iterations — each prints [ on enter, ] on exit
MOV R1, #0      ; global sem ID
; --- iter 1 ---
MOV R0, #201    ; sem_wait(R1=0)
INT 0x80
MOV R0, #1      ; print "["
MOV R1, #0x5B
INT 0x80
MOV R1, #0
MOV R0, #202    ; sem_signal(R1=0)
INT 0x80
MOV R0, #1      ; print "]"
MOV R1, #0x5D
INT 0x80
MOV R1, #0
; --- iter 2 ---
MOV R0, #201    ; sem_wait(R1=0)
INT 0x80
MOV R0, #1      ; print "["
MOV R1, #0x5B
INT 0x80
MOV R1, #0
MOV R0, #202    ; sem_signal(R1=0)
INT 0x80
MOV R0, #1      ; print "]"
MOV R1, #0x5D
INT 0x80
MOV R1, #0
; --- iter 3 ---
MOV R0, #201    ; sem_wait(R1=0)
INT 0x80
MOV R0, #1      ; print "["
MOV R1, #0x5B
INT 0x80
MOV R1, #0
MOV R0, #202    ; sem_signal(R1=0)
INT 0x80
MOV R0, #1      ; print "]"
MOV R1, #0x5D
INT 0x80
MOV R1, #0
; --- iter 4 ---
MOV R0, #201    ; sem_wait(R1=0)
INT 0x80
MOV R0, #1      ; print "["
MOV R1, #0x5B
INT 0x80
MOV R1, #0
MOV R0, #202    ; sem_signal(R1=0)
INT 0x80
MOV R0, #1      ; print "]"
MOV R1, #0x5D
INT 0x80
MOV R1, #0
; --- iter 5 ---
MOV R0, #201    ; sem_wait(R1=0)
INT 0x80
MOV R0, #1      ; print "["
MOV R1, #0x5B
INT 0x80
MOV R1, #0
MOV R0, #202    ; sem_signal(R1=0)
INT 0x80
MOV R0, #1      ; print "]"
MOV R1, #0x5D
INT 0x80
MOV R1, #0
; --- iter 6 ---
MOV R0, #201    ; sem_wait(R1=0)
INT 0x80
MOV R0, #1      ; print "["
MOV R1, #0x5B
INT 0x80
MOV R1, #0
MOV R0, #202    ; sem_signal(R1=0)
INT 0x80
MOV R0, #1      ; print "]"
MOV R1, #0x5D
INT 0x80
MOV R1, #0
; --- iter 7 ---
MOV R0, #201    ; sem_wait(R1=0)
INT 0x80
MOV R0, #1      ; print "["
MOV R1, #0x5B
INT 0x80
MOV R1, #0
MOV R0, #202    ; sem_signal(R1=0)
INT 0x80
MOV R0, #1      ; print "]"
MOV R1, #0x5D
INT 0x80
MOV R1, #0
; --- iter 8 ---
MOV R0, #201    ; sem_wait(R1=0)
INT 0x80
MOV R0, #1      ; print "["
MOV R1, #0x5B
INT 0x80
MOV R1, #0
MOV R0, #202    ; sem_signal(R1=0)
INT 0x80
MOV R0, #1      ; print "]"
MOV R1, #0x5D
INT 0x80
MOV R1, #0
; --- iter 9 ---
MOV R0, #201    ; sem_wait(R1=0)
INT 0x80
MOV R0, #1      ; print "["
MOV R1, #0x5B
INT 0x80
MOV R1, #0
MOV R0, #202    ; sem_signal(R1=0)
INT 0x80
MOV R0, #1      ; print "]"
MOV R1, #0x5D
INT 0x80
MOV R1, #0
; --- iter 10 ---
MOV R0, #201    ; sem_wait(R1=0)
INT 0x80
MOV R0, #1      ; print "["
MOV R1, #0x5B
INT 0x80
MOV R1, #0
MOV R0, #202    ; sem_signal(R1=0)
INT 0x80
MOV R0, #1      ; print "]"
MOV R1, #0x5D
INT 0x80
MOV R1, #0
HALT