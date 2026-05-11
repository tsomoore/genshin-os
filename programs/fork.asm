; fork.asm — trigger fork syscall (R0=100)
; Runs on CPU, triggers INT, ProcessService handles fork
MOV R0, #100
INT 0x80
; After fork: parent R0 = child_pid, child R0 = 0
; Both parent and child continue here
HALT
