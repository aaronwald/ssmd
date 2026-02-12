#!/bin/sh
set -e
mkdir -p /home/node/.kube
cat > /home/node/.kube/config << EOF
apiVersion: v1
kind: Config
clusters:
- cluster:
    certificate-authority: /var/run/secrets/kubernetes.io/serviceaccount/ca.crt
    server: https://kubernetes.default.svc
  name: default
contexts:
- context:
    cluster: default
    namespace: ssmd
    user: default
  name: default
current-context: default
users:
- name: default
  user:
    tokenFile: /var/run/secrets/kubernetes.io/serviceaccount/token
EOF
exec node dist/worker.js
