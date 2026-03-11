#!/usr/bin/env bash
set -euo pipefail

# Harman integration tests via port-forward to cluster Postgres.
# Creates a harman_test database if it doesn't exist, then runs
# crash_tests and group_fill_tests with --test-threads=1.
#
# Usage:
#   make harman-integration-test
#   # Or directly:
#   ./scripts/harman-integration-test.sh

NAMESPACE="ssmd"
PG_POD="ssmd-postgres-0"
LOCAL_PORT=5433
DB_NAME="harman_test"
DB_USER="ssmd"

# Get password from K8s secret
echo "Fetching database credentials..."
DB_PASS=$(kubectl get secret -n "$NAMESPACE" ssmd-harman-ws-db -o jsonpath='{.data.password}' | base64 -d)

# Start port-forward in background
echo "Starting port-forward to $PG_POD..."
kubectl port-forward -n "$NAMESPACE" "$PG_POD" "$LOCAL_PORT":5432 &
PF_PID=$!
trap "kill $PF_PID 2>/dev/null || true" EXIT

# Wait for port-forward to be ready
sleep 2

# Create test database if it doesn't exist
echo "Ensuring $DB_NAME database exists..."
kubectl exec -n "$NAMESPACE" "$PG_POD" -- psql -U "$DB_USER" -d postgres \
    -c "SELECT 1 FROM pg_database WHERE datname = '$DB_NAME'" | grep -q 1 || \
    kubectl exec -n "$NAMESPACE" "$PG_POD" -- psql -U "$DB_USER" -d postgres \
    -c "CREATE DATABASE $DB_NAME OWNER $DB_USER"

DATABASE_URL="postgresql://${DB_USER}:${DB_PASS}@localhost:${LOCAL_PORT}/${DB_NAME}?sslmode=disable"
export DATABASE_URL

echo "Running harman integration tests..."
echo "  DATABASE_URL=postgresql://${DB_USER}:***@localhost:${LOCAL_PORT}/${DB_NAME}"

cd ssmd-rust

# Run crash tests
echo ""
echo "=== crash_tests ==="
cargo test -p ssmd-harman --test crash_tests -- --ignored --test-threads=1

# Run group fill tests
echo ""
echo "=== group_fill_tests ==="
cargo test -p ssmd-harman --test group_fill_tests -- --ignored --test-threads=1

echo ""
echo "All integration tests passed!"
