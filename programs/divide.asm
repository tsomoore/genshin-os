; divide.asm — 除法演示
; 计算 R2 = 100 / 7 = 14 (整数除法)
MOV R0, #100
MOV R1, #7
DIV R0, R1
MOV R2, R0
HALT
