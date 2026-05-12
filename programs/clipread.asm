; clipread.asm — read clipboard via DeviceService  
; Result written to 0x200, length in R2, then printed

MOV R1, #256    ; max 256 bytes
MOV R0, #210    ; clipboard_read syscall
INT 0x80        ; data at 0x200, len in R2

; Print the clipboard contents
MOV R0, #2      ; print_str syscall
MOV R1, #0x200  ; buffer address
MOV R2, R2      ; length (from clipboard_read)
INT 0x80

HALT
