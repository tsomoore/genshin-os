; busy.asm — CPU busy loop (15 MOVs for testing scheduler)
; Each instruction 8 bytes, 15x8=120 bytes, > quantum
MOV R1, #0
MOV R1, #1
MOV R1, #2
MOV R1, #3
MOV R1, #4
MOV R1, #5
MOV R1, #6
MOV R1, #7
MOV R1, #8
MOV R1, #9
MOV R1, #10
MOV R1, #11
MOV R1, #12
MOV R1, #13
MOV R1, #14
HALT
