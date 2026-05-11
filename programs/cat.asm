; cat.asm — read file (syscall 10 + 12 + 11)
; Opens file, reads 16 bytes, prints, closes
MOV R0, #10   ; open (path at 0x100)
INT 0x80      ; fd in R1
MOV R0, #12   ; read
MOV R2, #0x10 ; 16 bytes
INT 0x80      ; prints data
MOV R0, #11   ; close
INT 0x80
HALT
