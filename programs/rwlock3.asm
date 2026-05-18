; rwlock3.asm — 5 Readers + 1 Writer. Role at 0x200. Sem 1=mutex, Sem 2=wrt.

LOAD R3, [0x200]  ; 0x00: role
CMP R3, #0        ; 0x08
JNZ 0x148         ; 0x10: writer

; ═══ READER (0x18) ═══
MOV R3, #3        ; 0x18: iterations
MOV R1, #1        ; 0x20: mutex.wait(1)
MOV R0, #201      ; 0x28
INT 0x80          ; 0x30
LOAD R2, [0x1000] ; 0x38: count++
ADD R2, #1        ; 0x40
STORE [0x1000], R2; 0x48
CMP R2, #1        ; 0x50
JNZ 0x78          ; 0x58: not first
MOV R1, #2        ; 0x60: wrt.wait(2)
MOV R0, #201      ; 0x68
INT 0x80          ; 0x70
MOV R1, #1        ; 0x78: mutex.signal(1)
MOV R0, #202      ; 0x80
INT 0x80          ; 0x88
MOV R0, #1        ; 0x90: print 'R'
MOV R1, #0x52     ; 0x98
INT 0x80          ; 0xA0
MOV R1, #1        ; 0xA8: mutex.wait(1)
MOV R0, #201      ; 0xB0
INT 0x80          ; 0xB8
LOAD R2, [0x1000] ; 0xC0: count--
SUB R2, #1        ; 0xC8
STORE [0x1000], R2; 0xD0
CMP R2, #0        ; 0xD8
JNZ 0x100         ; 0xE0: not last
MOV R1, #2        ; 0xE8: wrt.signal(2)
MOV R0, #202      ; 0xF0
INT 0x80          ; 0xF8
MOV R1, #1        ; 0x100: mutex.signal(1)
MOV R0, #202      ; 0x108
INT 0x80          ; 0x110
SUB R3, #1        ; 0x118: loop
CMP R3, #0        ; 0x120
JNZ 0x20          ; 0x128
MOV R0, #0        ; 0x130: exit(0)
MOV R1, #0        ; 0x138
INT 0x80          ; 0x140

; ═══ WRITER (0x148) ═══
MOV R3, #3        ; 0x148: iterations
MOV R1, #2        ; 0x150: wrt.wait(2)
MOV R0, #201      ; 0x158
INT 0x80          ; 0x160
MOV R0, #1        ; 0x168: print 'W'
MOV R1, #0x57     ; 0x170
INT 0x80          ; 0x178
MOV R2, #5        ; 0x180: delay
SUB R2, #1        ; 0x188
CMP R2, #0        ; 0x190
JNZ 0x188         ; 0x198
MOV R1, #2        ; 0x1A0: wrt.signal(2)
MOV R0, #202      ; 0x1A8
INT 0x80          ; 0x1B0
SUB R3, #1        ; 0x1B8: loop
CMP R3, #0        ; 0x1C0
JNZ 0x150         ; 0x1C8
MOV R0, #0        ; 0x1D0: exit(0)
MOV R1, #0        ; 0x1D8
INT 0x80          ; 0x1E0
