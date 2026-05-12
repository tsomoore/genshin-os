; clipread.asm — request clipboard → read → release

; 1. Request clipboard device
MOV R0, #208    ; device_open
INT 0x80

; 2. Read clipboard
MOV R1, #32     ; max 32 bytes
MOV R0, #210    ; clipboard_read
INT 0x80        ; data at 0x200, len in R2

; 3. Print result
MOV R0, #2      ; print_str
MOV R1, #0x200
MOV R2, R2
INT 0x80

; 4. Release clipboard device
MOV R0, #209    ; device_close
INT 0x80

HALT
