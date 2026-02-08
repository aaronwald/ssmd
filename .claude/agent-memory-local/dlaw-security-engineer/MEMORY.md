# Security Engineer Memory

## Codebase Patterns

### ssmd Connector Security Model
- Connectors use `sanitize_symbol()` pattern to strip NATS-special chars (`.`, `>`, `*`) before building subjects
- Channel names matched against hardcoded strings, not user input - safe pattern
- WebSocket message size limited via `tokio-tungstenite` `WebSocketConfig`
- mpsc channels are bounded (1000) for backpressure
- Crash-restart pattern: `std::process::exit(1)` on WS disconnect, K8s handles restart

### K8s Security Posture
- ssmd namespace uses default-deny NetworkPolicies
- Container security best practice: `readOnlyRootFilesystem`, `allowPrivilegeEscalation: false`, `capabilities.drop: ALL`
- Pod-level: `runAsUser: 1000` used but `runAsNonRoot: true` sometimes missing (check each deployment)
- Services: cache, cdc, data-ts, postgres, redis, notifier have NetworkPolicies; new services need them too

### Secrets Pattern
- Credentials resolved from env vars, never hardcoded
- Public feeds (Kraken) use `auth_method: none`
- Kalshi uses RSA key-based auth via `KalshiCredentials`
- ConfigMaps contain only public config (NATS service DNS, feed metadata)

## Common Findings
- Missing NetworkPolicy on new deployments (check every time)
- Missing `runAsNonRoot: true` at pod securityContext level
- Error message previews could leak data if pattern reused for authenticated feeds

## Review Checklist for New Connectors
1. NetworkPolicy exists with least-privilege egress
2. `runAsNonRoot: true` in pod securityContext
3. Symbol/ticker sanitization before NATS subject construction
4. WebSocket message size limits configured
5. Bounded channels for backpressure
6. No hardcoded credentials
7. TLS enforced for external WebSocket connections
8. Error messages don't leak sensitive data

## Session Log
- 2026-02-06: Reviewed Kraken connector (Rust WS + NATS writer + K8s deployment + DB migration). No HIGH findings. MEDIUM: missing NetworkPolicy, missing runAsNonRoot. Code quality is good.
- 2026-02-06: Reviewed Polymarket connector (WS + Gamma REST discovery + NATS writer). No HIGH findings. MEDIUM: unbounded pagination DoS risk, error message preview leaks, no Gamma API response size limit. LOW: mpsc buffer larger than other connectors (2000 vs 1000). Overall good security posture, consistent with Kraken patterns.
