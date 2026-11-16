#!/bin/sh
# exec: the sleeper IS the direct child, so the timeout kill ends it (no orphan holding the pipe)
exec /bin/sleep 5
