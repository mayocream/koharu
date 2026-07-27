#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 2 ]]; then
  echo "usage: $0 POD_HOST_ID PUBLIC_KEY_FILE" >&2
  exit 2
fi

pod_host_id=$1
public_key_file=$2
encoded=$(base64 -w0 <"$public_key_file")

(
  sleep 5
  printf 'stty -echo\n'
  sleep 1
  printf 'mkdir -p /root/.ssh && chmod 700 /root/.ssh\n'
  printf "echo '%s' | base64 -d > /root/.ssh/authorized_keys\n" "$encoded"
  printf 'chmod 600 /root/.ssh/authorized_keys\n'
  printf 'service ssh restart || /etc/init.d/ssh restart || true\n'
  printf 'echo SSH_REPAIR_DONE; wc -c /root/.ssh/authorized_keys\n'
  sleep 4
  printf 'exit\n'
) | ssh -tt \
  -o BatchMode=yes \
  -o ConnectTimeout=20 \
  -o ServerAliveInterval=10 \
  -o ServerAliveCountMax=3 \
  -o StrictHostKeyChecking=accept-new \
  -i /c/Users/Mayo/.ssh/id_ed25519 \
  "${pod_host_id}@ssh.runpod.io"
