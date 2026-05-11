; semdemo.asm — semaphore demo
; Creates semaphore, forks, both use sem to coordinate

MOV R0, #200   ; sem_create
INT 0x80       ; sem_id in R1

MOV R0, #100   ; fork
INT 0x80       ; child gets R0=0, parent gets R0=child_pid

; Both parent and child continue here
MOV R0, #201   ; sem_wait(R1=sem_id)
INT 0x80       ; may block if other process holds sem

; Critical section
MOV R0, #1     ; print "in critical section"
MOV R1, #0x43  ; 'C'
INT 0x80

MOV R0, #202   ; sem_signal(R1=sem_id)
INT 0x80       ; release sem

HALT
