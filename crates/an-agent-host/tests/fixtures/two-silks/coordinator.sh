#!/bin/bash
# Domain sequencing stays inside the spool body; the host owns the whole group.
set -eu
base=$1
mode=$2
/bin/bash "$base/producer.sh" "$mode" &
producer=$!
/bin/bash "$base/consumer.sh" "$mode" &
consumer=$!
printf '%s\n' "$$" > coordinator.pid
printf '%s\n' "$producer" > producer.pid
printf '%s\n' "$consumer" > consumer.pid
while [ ! -f producer.ready ] || [ ! -f consumer.ready ]; do /bin/sleep 0.01; done
if [ "$mode" = orphan ]; then
    printf 'candidate from incomplete round\n' > output.txt
    exit 0
fi
wait "$producer"
wait "$consumer"
