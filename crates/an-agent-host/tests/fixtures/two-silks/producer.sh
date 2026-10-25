#!/bin/bash
set -eu
mode=$1
printf ready > producer.ready
if [ "$mode" = fail ]; then exit 7; fi
if [ "$mode" = cancel-before ] || [ "$mode" = orphan ] || [ "$mode" = timeout ]; then
    while :; do /bin/sleep 0.05; done
fi
printf 'precise internal handoff\n' > internal.pending
/bin/mv internal.pending internal.ready
if [ "$mode" = cancel-after ]; then
    while :; do /bin/sleep 0.05; done
fi
