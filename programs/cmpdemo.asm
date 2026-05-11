; cmpdemo.asm — conditional jump: countdown 5→0
MOV R2, #5      ; counter in R2

; === loop (JNZ target) ===
MOV R0, #1      ; print syscall
MOV R1, R2      ; print counter value
INT 0x80

SUB R2, #1      ; counter--
CMP R2, #0      ; compare with 0
JNZ 0x08        ; if R2 != 0, back to MOV R0,#1

HALT
