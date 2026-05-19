; syncdemo.asm — Mutex mutual exclusion demo (lock_acquire/release)
; Lock 0 is the global mutex. Only holder can release.

MOV R1, #0      ; 0x00: lock_id = 0
MOV R0, #204    ; 0x08: lock_acquire(0)
INT 0x80        ; 0x10

MOV R0, #1      ; 0x18: print '['
MOV R1, #0x5B   ; 0x20
INT 0x80        ; 0x28

MOV R2, #30     ; 0x30: spin delay
SUB R2, #1      ; 0x38
CMP R2, #0      ; 0x40
JNZ 0x38        ; 0x48

MOV R1, #0      ; 0x50: lock_release(0)
MOV R0, #205    ; 0x58
INT 0x80        ; 0x60

MOV R0, #1      ; 0x68: print ']'
MOV R1, #0x5D   ; 0x70
INT 0x80        ; 0x78

JMP 0x00        ; 0x80: infinite loop
