#!/bin/bash
set -eu
mode=$1
printf ready > consumer.ready
while [ ! -f internal.ready ]; do /bin/sleep 0.01; done
/usr/bin/tr '[:lower:]' '[:upper:]' < internal.ready > output.pending
/bin/mv output.pending output.txt
if [ "$mode" = cancel-after ]; then
    while :; do /bin/sleep 0.05; done
fi
