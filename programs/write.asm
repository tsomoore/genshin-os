; write.asm — write to file (syscall 10 + 13 + 11)
; Opens file, writes data from 0x200, closes
MOV R1, #1    ; flags = create
MOV R0, #10   ; open (path at 0x100)
INT 0x80      ; fd in R1
MOV R0, #13   ; write
MOV R2, #255  ; max 255 bytes (data at 0x200)
INT 0x80      ; write data
MOV R0, #11   ; close
INT 0x80
HALT
