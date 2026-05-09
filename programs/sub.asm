; sub.asm — 减法演示
; 计算 R2 = 50 - 30 = 20, R3 = 0 - 20 = -20
MOV R0, #50
MOV R1, #30
SUB R2, R0
SUB R3, R2
HALT
