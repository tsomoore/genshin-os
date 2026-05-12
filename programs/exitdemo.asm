; exitdemo.asm — demonstrate proper exit with cleanup
; Opens a file, writes data, then exits with code 42

MOV R1, #1      ; flags = create
MOV R0, #10     ; open syscall
INT 0x80        ; fd in R1

MOV R0, #13     ; write syscall
MOV R2, #5      ; write 5 bytes
INT 0x80        ; write data from 0x200

; Exit: R0=0 (halt), R1=42 (exit code)
MOV R1, #42
MOV R0, #0
INT 0x80        ; exit(42)

HALT
