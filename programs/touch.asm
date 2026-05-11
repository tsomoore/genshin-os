; touch.asm — create file (syscall 10 + 11)
; Opens file for writing then closes it
MOV R1, #1    ; flags = create
MOV R0, #10   ; open
INT 0x80      ; fd in R1
MOV R0, #11   ; close
INT 0x80
HALT
