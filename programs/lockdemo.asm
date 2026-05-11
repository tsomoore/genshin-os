; lockdemo.asm — mutex lock demo
; Creates lock, forks, both increment shared counter

MOV R0, #203   ; lock_create
INT 0x80       ; lock_id in R1

MOV R0, #100   ; fork
INT 0x80       ; child gets R0=0

; Both try to acquire lock
MOV R0, #204   ; lock_acquire(R1=lock_id)
INT 0x80       ; may block

; Critical section: increment counter at 0x5000
LOAD R2, [0x5000]
ADD R2, #1
STORE [0x5000], R2

MOV R0, #205   ; lock_release(R1=lock_id)
INT 0x80

; Print counter value
MOV R1, R2
MOV R0, #1
INT 0x80

HALT
