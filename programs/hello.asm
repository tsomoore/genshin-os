; hello.asm — print "Hello" via STORE + PRINT_STR syscall

MOV R0, #72
STORE [0x200], R0    ; 'H'
MOV R0, #101
STORE [0x201], R0    ; 'e'
MOV R0, #108
STORE [0x202], R0    ; 'l'
STORE [0x203], R0    ; 'l'
MOV R0, #111
STORE [0x204], R0    ; 'o'

MOV R0, #2           ; syscall PRINT_STR
MOV R1, #0x200
MOV R2, #5
INT 0x80

MOV R0, #0
INT 0x80
