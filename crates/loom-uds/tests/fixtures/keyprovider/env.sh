#!/bin/sh
# env must be cleared except LOOM_UDS_LEVEL (sh itself may set a default PATH, so HOME/USER are the probe)
if [ -n "$HOME" ] || [ -n "$USER" ]; then echo ZZ; else printf "%s%s\n" "$LOOM_UDS_LEVEL" "$LOOM_UDS_LEVEL"; fi
