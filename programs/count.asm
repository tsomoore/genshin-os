; count.asm — 计数器演示
; R0 从 0 数到 5, 每次加 1
; R1 作为循环计数器从 5 倒数到 0
MOV R0, #0
MOV R1, #5
ADD R0, #1
SUB R1, #1
ADD R0, #1
SUB R1, #1
ADD R0, #1
SUB R1, #1
ADD R0, #1
SUB R1, #1
ADD R0, #1
SUB R1, #1
HALT
