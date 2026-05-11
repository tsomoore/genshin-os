; swaptest.asm — touch pages to trigger page faults

MOV R0, #0x41    ; marker 'A'
STORE [0xa000], R0    ; page 10
STORE [0xb000], R0    ; page 11
STORE [0xc000], R0    ; page 12
STORE [0xd000], R0    ; page 13
STORE [0xe000], R0    ; page 14
STORE [0xf000], R0    ; page 15
STORE [0x10000], R0    ; page 16
STORE [0x11000], R0    ; page 17
STORE [0x12000], R0    ; page 18
STORE [0x13000], R0    ; page 19
STORE [0x14000], R0    ; page 20
STORE [0x15000], R0    ; page 21
STORE [0x16000], R0    ; page 22
STORE [0x17000], R0    ; page 23
STORE [0x18000], R0    ; page 24
STORE [0x19000], R0    ; page 25
STORE [0x1a000], R0    ; page 26
STORE [0x1b000], R0    ; page 27
STORE [0x1c000], R0    ; page 28
STORE [0x1d000], R0    ; page 29

LOAD R1, [0xa000]
MOV R0, #1 ; print R1
INT 0x80
HALT