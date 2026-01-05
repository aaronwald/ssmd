#!/bin/bash
# scripts/generate-k8s.sh
# Generates Kubernetes manifests from feed configurations
set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

echo "Generating Kubernetes manifests from feeds..."
deno run --allow-read --allow-write \
  "$SCRIPT_DIR/feed-to-configmap.ts" \
  "$REPO_ROOT/exchanges/feeds" \
  "$REPO_ROOT/generated/k8s"

echo ""
echo "Done. Copy generated/k8s/*.yaml to varlab/clusters/homelab/apps/ssmd/generated/"
