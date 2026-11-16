#!/bin/sh
# test provider: key = seed XOR 0x12345678 (the loom-uds-sim bench algorithm)
read s
printf '%08X\n' $(( 0x$s ^ 0x12345678 ))
