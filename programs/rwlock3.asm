; rwlock3.asm — Reader-priority RW lock. sem1=mutex sem2=wrt sem3=rd

LOAD R3, [0x200]  ; 0x00: role
CMP R3, #0        ; 0x08
JNZ 0x178         ; 0x10: writer

; ═══ READER ═══
MOV R3, #3        ; 0x18
MOV R1, #3        ; 0x20: rd.wait(3)
MOV R0, #201      ; 0x28
INT 0x80          ; 0x30
MOV R1, #1        ; 0x38: mutex.wait(1)
MOV R0, #201      ; 0x40
INT 0x80          ; 0x48
LOAD R2, [0x1000] ; 0x50: count++
ADD R2, #1        ; 0x58
STORE [0x1000], R2; 0x60
CMP R2, #1        ; 0x68
JNZ 0x90          ; 0x70
MOV R1, #2        ; 0x78: wrt.wait(2)
MOV R0, #201      ; 0x80
INT 0x80          ; 0x88
MOV R1, #1        ; 0x90: mutex.signal(1)
MOV R0, #202      ; 0x98
INT 0x80          ; 0xA0
MOV R0, #1        ; 0xA8: print 'R'
MOV R1, #0x52     ; 0xB0
INT 0x80          ; 0xB8
MOV R1, #1        ; 0xC0: mutex.wait(1)
MOV R0, #201      ; 0xC8
INT 0x80          ; 0xD0
LOAD R2, [0x1000] ; 0xD8: count--
SUB R2, #1        ; 0xE0
STORE [0x1000], R2; 0xE8
CMP R2, #0        ; 0xF0
JNZ 0x118         ; 0xF8
MOV R1, #2        ; 0x100: wrt.signal(2)
MOV R0, #202      ; 0x108
INT 0x80          ; 0x110
MOV R1, #1        ; 0x118: mutex.signal(1)
MOV R0, #202      ; 0x120
INT 0x80          ; 0x128
MOV R1, #3        ; 0x130: rd.signal(3)
MOV R0, #202      ; 0x138
INT 0x80          ; 0x140
SUB R3, #1        ; 0x148: loop
CMP R3, #0        ; 0x150
JNZ 0x20          ; 0x158
MOV R0, #0        ; 0x160: exit
MOV R1, #0        ; 0x168
INT 0x80          ; 0x170

; ═══ WRITER ═══
MOV R3, #3        ; 0x178
MOV R1, #3        ; 0x180: rd.wait(3)
MOV R0, #201      ; 0x188
INT 0x80          ; 0x190
MOV R1, #2        ; 0x198: wrt.wait(2)
MOV R0, #201      ; 0x1A0
INT 0x80          ; 0x1A8
MOV R0, #1        ; 0x1B0: print 'W'
MOV R1, #0x57     ; 0x1B8
INT 0x80          ; 0x1C0
MOV R2, #5        ; 0x1C8: delay
SUB R2, #1        ; 0x1D0
CMP R2, #0        ; 0x1D8
JNZ 0x1D0         ; 0x1E0
MOV R1, #2        ; 0x1E8: wrt.signal(2)
MOV R0, #202      ; 0x1F0
INT 0x80          ; 0x1F8
MOV R1, #3        ; 0x200: rd.signal(3)
MOV R0, #202      ; 0x208
INT 0x80          ; 0x210
SUB R3, #1        ; 0x218: loop
CMP R3, #0        ; 0x220
JNZ 0x180         ; 0x228
MOV R0, #0        ; 0x230: exit
MOV R1, #0        ; 0x238
INT 0x80          ; 0x240
