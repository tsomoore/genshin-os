; rwlock3.asm — 5 Readers + 1 Writer demo
; Shared page at 0x10000 = reader_count
; sem 1 = mutex, sem 2 = wrt
; Role byte at 0x200: 0=reader, 1=writer (set by spawn handler)
; NOP fill to align: each instruction = 8 bytes

NOP               ; 0x00: placeholder

; === LOAD ROLE ===
LOAD R3, [0x200]  ; 0x08: R3 = role (0=reader, 1=writer)
CMP R3, #0        ; 0x10: is reader?
JNZ WRITER_CODE    ; 0x18: no → jump to writer

; ═══════════════════════════════════════
; READER CODE
; ═══════════════════════════════════════
MOV R3, #3         ; 0x20: 3 iterations

READER_LOOP:
; mutex.wait(1)
MOV R1, #1         ; 0x28
MOV R0, #201       ; 0x30
INT 0x80           ; 0x38

; reader_count++ (shared memory)
LOAD R2, [0x1000]  ; 0x40
ADD R2, #1         ; 0x48
STORE [0x1000], R2 ; 0x50

; if count==1: wrt.wait(2)
CMP R2, #1         ; 0x58
JNZ R_SKIP_WRT     ; 0x60
MOV R1, #2         ; 0x68
MOV R0, #201       ; 0x70
INT 0x80           ; 0x78

R_SKIP_WRT:
; mutex.signal(1)
MOV R1, #1         ; 0x80
MOV R0, #202       ; 0x88
INT 0x80           ; 0x90

; ── READING ──
MOV R0, #1         ; 0x98: print 'R'
MOV R1, #0x52      ; 0xA0
INT 0x80           ; 0xA8

; ── READER EXIT ──
; mutex.wait(1)
MOV R1, #1         ; 0xB0
MOV R0, #201       ; 0xB8
INT 0x80           ; 0xC0

; reader_count--
LOAD R2, [0x1000]  ; 0xC8
SUB R2, #1         ; 0xD0
STORE [0x1000], R2 ; 0xD8

; if count==0: wrt.signal(2)
CMP R2, #0         ; 0xE0
JNZ R_SKIP_SIG     ; 0xE8
MOV R1, #2         ; 0xF0
MOV R0, #202       ; 0xF8
INT 0x80           ; 0x100

R_SKIP_SIG:
; mutex.signal(1)
MOV R1, #1         ; 0x108
MOV R0, #202       ; 0x110
INT 0x80           ; 0x118

; loop
SUB R3, #1         ; 0x120
CMP R3, #0         ; 0x128
JNZ 0x28           ; 0x130: loop to READER_LOOP

MOV R0, #0         ; 0x138: exit(0)
MOV R1, #0         ; 0x140
INT 0x80           ; 0x148

; ═══════════════════════════════════════
; WRITER CODE
; ═══════════════════════════════════════
WRITER_CODE:       ; 0x150
MOV R3, #3         ; 0x150: 3 iterations

WRITER_LOOP:
; wrt.wait(2)
MOV R1, #2         ; 0x158
MOV R0, #201       ; 0x160
INT 0x80           ; 0x168

; ── WRITING ──
MOV R0, #1         ; 0x170: print 'W'
MOV R1, #0x57      ; 0x178
INT 0x80           ; 0x180

; spin delay
MOV R2, #5         ; 0x188
W_DELAY:
SUB R2, #1         ; 0x190
CMP R2, #0         ; 0x198
JNZ 0x190          ; 0x1A0

; wrt.signal(2)
MOV R1, #2         ; 0x1A8
MOV R0, #202       ; 0x1B0
INT 0x80           ; 0x1B8

; loop
SUB R3, #1         ; 0x1C0
CMP R3, #0         ; 0x1C8
JNZ 0x158          ; 0x1D0

MOV R0, #0         ; 0x1D8: exit(0)
MOV R1, #0         ; 0x1E0
INT 0x80           ; 0x1E8
