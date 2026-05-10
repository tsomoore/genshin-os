; loop.asm — simple counter loop (runs ~20 instructions to cross time slice)
; R0 = counter, loops 10 times (each iteration: ADD + SUB = 2 instructions)

MOV R0, #0           ; 0x00: counter = 0
MOV R1, #10          ; 0x08: loop limit = 10

ADD R0, #1           ; 0x10: counter++      <--- loop entry
SUB R1, #1           ; 0x18: limit--
JMP 0x10             ; 0x20: jump back to ADD (infinite if R1 not checked)

; This is an infinite loop for demo — runs forever until killed
; or quantum expires and scheduler switches to another process
