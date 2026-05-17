; rwlock2.asm — Reader-Writer Lock with CMP+JNZ
; Shared page at 0x10000 = reader_count. sem 1=mutex, sem 2=wrt.
; Both run same code as readers (multiple readers allowed).

MOV R0, #0
STORE [0x1000], R0   ; 0x00: reader_count=0
MOV R0, #3
STORE [0x200], R0    ; 0x08: loop=3

; ── LOOP (0x10) ──
MOV R1, #1           ; 0x10 mutex.wait(1)
MOV R0, #201         ; 0x18
INT 0x80             ; 0x20

LOAD R2, [0x1000]    ; 0x28 reader_count++
ADD R2, #1           ; 0x30
STORE [0x1000], R2   ; 0x38

CMP R2, #1           ; 0x40 first reader?
JNZ 0x68             ; 0x48 no→skip wrt.wait
MOV R1, #2           ; 0x50 yes→wrt.wait(2)
MOV R0, #201         ; 0x58
INT 0x80             ; 0x60

MOV R1, #1           ; 0x68 mutex.signal(1)
MOV R0, #202         ; 0x70
INT 0x80             ; 0x78

MOV R0, #1           ; 0x80 print 'R'
MOV R1, #0x52        ; 0x88
INT 0x80             ; 0x90

MOV R1, #1           ; 0x98 mutex.wait(1)
MOV R0, #201         ; 0xA0
INT 0x80             ; 0xA8

LOAD R2, [0x1000]    ; 0xB0 reader_count--
SUB R2, #1           ; 0xB8
STORE [0x1000], R2   ; 0xC0

CMP R2, #0           ; 0xC8 last reader?
JNZ 0xF0             ; 0xD0 no→skip wrt.signal
MOV R1, #2           ; 0xD8 yes→wrt.signal(2)
MOV R0, #202         ; 0xE0
INT 0x80             ; 0xE8

MOV R1, #1           ; 0xF0 mutex.signal(1)
MOV R0, #202         ; 0xF8
INT 0x80             ; 0x100

LOAD R0, [0x200]     ; 0x108 loop--
SUB R0, #1           ; 0x110
STORE [0x200], R0    ; 0x118
CMP R0, #0           ; 0x120 done?
JNZ 0x10             ; 0x128 loop

HALT                 ; 0x130
