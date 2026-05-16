; memtest.asm — Memory allocation & zombie demo
; Allocates pages, verifies them, then exits with code 42.
; Watch in pmon: MEM column shows frame count, then 0f after exit.
;
; STORE to a new page → page fault → frame allocated → mapped.
; After exit(42), all frames freed → process becomes Zombie → reaped.

MOV R0, #99
STORE [0x0000], R0    ; page 0 (code page, already mapped)

MOV R0, #100
STORE [0x1000], R0    ; page 1 → triggers page fault → alloc frame

MOV R0, #200
STORE [0x2000], R0    ; page 2 → alloc frame

MOV R0, #300
STORE [0x3000], R0    ; page 3 → alloc frame

MOV R0, #400
STORE [0x4000], R0    ; page 4 → alloc frame

MOV R0, #500
STORE [0x5000], R0    ; page 5 → alloc frame

; Verify: read back values
LOAD R1, [0x1000]     ; should be 100
LOAD R2, [0x2000]     ; should be 200
LOAD R3, [0x3000]     ; should be 300

; At this point, 6 frames allocated (pages 0-5).
; pmon MEM column shows "6f" for this PID.

; Wait briefly so pmon observer can see the memory
MOV R0, #0
MOV R0, #0
MOV R0, #0

; exit(42) — all 6 frames freed → Zombie → reaped
; pmon shows: state Zombie, MEM: 0f
MOV R0, #0
MOV R1, #42
INT 0x80
