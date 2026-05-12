; clipwrite.asm — write data to clipboard via DeviceService
; Reads data from 0x200 written by exec

MOV R0, #211    ; clipboard_write syscall
MOV R2, #32     ; write up to 32 bytes
INT 0x80        ; sends ClipboardSet to DeviceService

HALT
