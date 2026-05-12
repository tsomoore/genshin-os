; clipread.asm — read clipboard via DeviceService
MOV R1, #32     ; max 32 bytes
MOV R0, #210    ; clipboard_read syscall
INT 0x80        ; → DeviceService.ClipboardGet → data at 0x200

MOV R0, #2      ; print_str syscall
MOV R1, #0x200  ; buffer
MOV R2, R2      ; use length from clipboard_read
INT 0x80

HALT
