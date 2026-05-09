; hello.asm — store "Hello" chars to memory and print via syscall
; Writes 'H','e','l','l','o' to addresses 0x200-0x204
; Then calls PRINT_STR syscall

STORE [0x200], #72   ; 'H' = 0x48 = 72
MOV R0, #73
STORE [0x201], R0    ; 'e'
MOV R0, #108
STORE [0x202], R0    ; 'l'
STORE [0x203], R0    ; 'l'
MOV R0, #111
STORE [0x204], R0    ; 'o'

MOV R0, #2           ; syscall PRINT_STR
MOV R1, #0x200       ; address
MOV R2, #5           ; length
INT 0x80

MOV R0, #0
INT 0x80
