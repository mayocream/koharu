#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 2 ]]; then
  echo "usage: $0 POD_HOST_ID COMMAND_FILE" >&2
  exit 2
fi

pod_host_id=$1
command_file=$2
payload=$(base64 -w0 <"$command_file")

(
  sleep 5
  printf 'stty -echo\n'
  sleep 1
  printf "echo '%s' | base64 -d | bash\n" "$payload"
  sleep 3
  printf 'exit\n'
) | ssh -tt \
  -o BatchMode=yes \
  -o ConnectTimeout=20 \
  -o ServerAliveInterval=10 \
  -o ServerAliveCountMax=3 \
  -o StrictHostKeyChecking=accept-new \
  -i /c/Users/Mayo/.ssh/id_ed25519 \
  "${pod_host_id}@ssh.runpod.io"
