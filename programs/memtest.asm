; memtest.asm — Memory allocation + zombie demo
; Allocates 8 pages, spins in delay loop, then exit(42).
; Demo: run memtest → pmon → MEM: 8f → Zombie → 0f → gone

MOV R0, #99
STORE [0x0000], R0    ; page 0 (code, from exec)

MOV R0, #100
STORE [0x1000], R0    ; page 1

MOV R0, #200
STORE [0x2000], R0    ; page 2

MOV R0, #300
STORE [0x3000], R0    ; page 3

MOV R0, #400
STORE [0x4000], R0    ; page 4

MOV R0, #500
STORE [0x5000], R0    ; page 5

MOV R0, #600
STORE [0x6000], R0    ; page 6

MOV R0, #700
STORE [0x7000], R0    ; page 7

; Delay: 100 * 50 iterations so pmon observer can see peak memory
MOV R3, #100          ; 0x40: outer
MOV R2, #50           ; 0x48: inner
SUB R2, #1            ; 0x50
CMP R2, #0            ; 0x58
JNZ 0x50              ; 0x60
SUB R3, #1            ; 0x68
CMP R3, #0            ; 0x70
JNZ 0x48              ; 0x78

; exit(42)
MOV R0, #0            ; 0x80
MOV R1, #42           ; 0x88
INT 0x80              ; 0x90
