; clipwrite.asm — write "HELLO" to clipboard via DeviceService
MOV R0, #0x48   ; 'H'
STORE [0x200], R0
MOV R0, #0x45   ; 'E'
STORE [0x201], R0
MOV R0, #0x4C   ; 'L'
STORE [0x202], R0
STORE [0x203], R0
MOV R0, #0x4F   ; 'O'
STORE [0x204], R0

MOV R0, #211    ; clipboard_write syscall
MOV R2, #5      ; 5 bytes
INT 0x80        ; → DeviceService.ClipboardSet

HALT
