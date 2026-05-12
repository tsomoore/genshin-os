; testexit.asm — allocate pages, then exit() to free them
; Demonstrates: page allocation → exit cleanup → memory freed

; --- Touch 5 pages to force MemoryService allocations ---
MOV R0, #0x41    ; marker byte A
STORE [0xa000], R0    ; touch page 10
STORE [0xb000], R0    ; touch page 11
STORE [0xc000], R0    ; touch page 12
STORE [0xd000], R0    ; touch page 13
STORE [0xe000], R0    ; touch page 14

; --- Read back to verify ---
LOAD R1, [0xa000]  ; read first page
MOV R0, #1       ; print R1 (should be 65 = A)
INT 0x80

; --- Proper exit with cleanup ---
MOV R1, #42      ; exit code
MOV R0, #0       ; exit syscall
INT 0x80         ; exit(42) — frees all 5 pages

HALT              ; never reached