; Page fault test — access unmapped address 0x5000
LOAD R0, [0x5000]   ; Read from unmapped page → PageFault
MOV R1, R0          ; Use the loaded value
MOV R0, #0
INT 0x80            ; HALT
