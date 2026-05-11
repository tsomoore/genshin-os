; swaptest.asm — verify page fault handling works
; Touches 20 pages, reads back first and last

MOV R0, #0x41    ; marker 'A'
STORE [0xa000], R0
STORE [0xb000], R0
STORE [0xc000], R0
STORE [0xd000], R0
STORE [0xe000], R0
STORE [0xf000], R0
STORE [0x10000], R0
STORE [0x11000], R0
STORE [0x12000], R0
STORE [0x13000], R0
STORE [0x14000], R0
STORE [0x15000], R0
STORE [0x16000], R0
STORE [0x17000], R0
STORE [0x18000], R0
STORE [0x19000], R0
STORE [0x1a000], R0
STORE [0x1b000], R0
STORE [0x1c000], R0
STORE [0x1d000], R0

LOAD R1, [0xa000]
MOV R0, #1 ; print R1
INT 0x80

LOAD R1, [0x1d000]
MOV R0, #1 ; print R1
INT 0x80

HALT