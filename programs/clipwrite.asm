; clipwrite.asm — request clipboard → write → release
; Demonstrates device open/write/close lifecycle

; 1. Request clipboard device
MOV R0, #208    ; device_open
INT 0x80        ; → "[DEVICE] pid=N requests clipboard"

; 2. Write data to 0x200
MOV R0, #0x48   ; 'H'
STORE [0x200], R0
MOV R0, #0x45   ; 'E'
STORE [0x201], R0
MOV R0, #0x4C   ; 'L'
STORE [0x202], R0
STORE [0x203], R0
MOV R0, #0x4F   ; 'O'
STORE [0x204], R0

; 3. Write to clipboard via DeviceService
MOV R0, #211    ; clipboard_write
MOV R2, #5      ; 5 bytes
INT 0x80

; 4. Release clipboard device
MOV R0, #209    ; device_close
INT 0x80        ; → "[DEVICE] pid=N releases clipboard"

HALT
