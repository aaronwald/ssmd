# Senior Developer Agent Memory

## 2026-02-07: Operator Generalization Review (Connector Multi-Exchange)

### Task
Reviewed connector_controller.go architecture for generalizing from Kalshi-only to multi-exchange support.

### Key Findings
- **HIGH: Feed config generated inline** — `constructConfigMap()` hardcodes Kalshi WS endpoint, auth env vars, stream names. Should read from feed ConfigMaps instead.
- **HIGH: Hardcoded env var names** — `KALSHI_API_KEY`/`KALSHI_PRIVATE_KEY` injected regardless of exchange. SecretRef allows custom field names but not env var names.
- **HIGH: Kalshi-specific CRD fields at top level** — `Categories`, `GamesOnly`, `CloseWithinHours` should be nested under `filtering` or similar optional struct.
- **MEDIUM: No feed name validation** — `getFeedDefaults()` silently returns nil for missing ConfigMaps.
- **MEDIUM: YAML generation via fmt.Sprintf** — fragile, untestable. Should use struct marshaling.
- **MEDIUM: deploymentNeedsUpdate duplicated** across all 3 controllers.

### Recommendations Given
1. Feed ConfigMap-driven architecture (extend existing `getFeedDefaults` pattern)
2. Restructure CRD: Option B (FilteringConfig nested struct) for backwards compat
3. Three-phase migration: additive → deprecation warnings → remove in v1alpha2
4. Struct-based YAML generation using `sigs.k8s.io/yaml`
5. Extract shared `deploymentNeedsUpdate` to helpers.go
6. Add `FeedConfigValid` condition for early validation
7. Keep ConfigMaps, don't create FeedConfig CRD (static data doesn't need lifecycle)

### Architecture Patterns Observed
- Three controllers (connector, archiver, signal) share identical structure: Reconcile → reconcileDelete → reconcileConfigMap → reconcileDeployment → updateStatus
- Archiver is more generic than connector (multi-source support, no hardcoded exchange names)
- Signal controller is cleanest/simplest — good reference for minimal controller pattern
- `getFeedDefaults()` pattern exists but is underutilized (only reads image/version)
- Feed YAMLs in `exchanges/feeds/` have `defaults:` section with operator-relevant config (image, version, resources, transport)
- Environment YAMLs in `exchanges/environments/` define transport/stream/prefix per deployment context

### Codebase Reference
- Connector controller: `ssmd-operators/internal/controller/connector_controller.go`
- Connector CRD types: `ssmd-operators/api/v1alpha1/connector_types.go`
- Archiver controller (comparison): `ssmd-operators/internal/controller/archiver_controller.go`
- Signal controller (comparison): `ssmd-operators/internal/controller/signal_controller.go`
- Feed YAMLs: `exchanges/feeds/{kalshi,kraken,polymarket}.yaml`
- Env YAMLs: `exchanges/environments/`

### Co-Panelist Notes
- Data Feed expert would be valuable for verifying Rust connector's env var contract
- Operations expert needed for feed ConfigMap provisioning strategy (CLI vs Flux)
