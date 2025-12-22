# ssmd: Kalshi Design - Key Management

Keys are first-class citizens. Every environment starts with key definitions. The system makes secret management easy - no manual kubectl or kubeseal operations required.

## Design Principles

1. **Keys first** - Environment definitions start with keys, not infrastructure
2. **Declarative** - Keys defined in YAML, CLI handles encryption/storage
3. **Validated** - All key references validated before deployment
4. **Rotatable** - Keys can be rotated without redeployment
5. **Audited** - All key access and changes logged

## Key Types

| Type | Purpose | Examples |
|------|---------|----------|
| `api_key` | Exchange API credentials | Kalshi API key/secret |
| `transport` | Message broker auth | NATS credentials |
| `storage` | Object storage access | S3 access key/secret |
| `tls` | Certificates | mTLS certs, CA bundles |
| `webhook` | Callback authentication | Agent webhook secrets |

## Environment Keys Definition

Keys are the **first section** in every environment file:

```yaml
# exchanges/environments/kalshi-prod.yaml
name: kalshi-prod
feed: kalshi
schema: trade:v1

keys:
  # Exchange credentials
  kalshi:
    type: api_key
    description: "Kalshi trading API"
    required: true
    fields:
      - api_key
      - api_secret
    rotation_days: 90

  # Infrastructure credentials
  nats:
    type: transport
    description: "NATS messaging"
    required: true
    fields:
      - url
      - username
      - password
      - tls_cert      # optional
      - tls_key       # optional

  storage:
    type: storage
    description: "Object storage (when Garage ready)"
    required: false   # Optional until Brooklyn NAS
    fields:
      - endpoint
      - access_key
      - secret_key
      - region

transport:
  type: nats
  url: $key:nats.url

storage:
  type: s3
  endpoint: $key:storage.endpoint
```

## Key Storage Architecture

```
┌─────────────────────────────────────────────────────────────────────┐
│                         ssmd CLI                                     │
│  ssmd key set kalshi-prod kalshi --api-key xxx --api-secret yyy     │
└───────────────────────────────┬─────────────────────────────────────┘
                                │
                                ▼
┌─────────────────────────────────────────────────────────────────────┐
│                      Key Manager                                     │
│  - Validates key fields against environment spec                    │
│  - Encrypts with Sealed Secrets public key                          │
│  - Stores encrypted values                                          │
└───────────────────────────────┬─────────────────────────────────────┘
                                │
              ┌─────────────────┴─────────────────┐
              ▼                                   ▼
       ┌───────────┐                       ┌───────────┐
       │  Sealed   │                       │   Audit   │
       │  Secret   │                       │    Log    │
       │  (K8s)    │                       │           │
       └───────────┘                       └───────────┘
```

## CLI Commands

```bash
# Initialize keys for a new environment (interactive)
ssmd key init kalshi-prod
# Prompts for each required key defined in environment spec

# Set individual key
ssmd key set kalshi-prod kalshi --api-key xxx --api-secret yyy

# Set from file (for complex keys like TLS certs)
ssmd key set kalshi-prod nats --from-file tls_cert=./cert.pem --from-file tls_key=./key.pem

# Set from environment variables
export KALSHI_API_KEY=xxx
export KALSHI_API_SECRET=yyy
ssmd key set kalshi-prod kalshi --from-env KALSHI_API_KEY,KALSHI_API_SECRET

# List keys (shows metadata, not values)
ssmd key list kalshi-prod
# NAME      TYPE      REQUIRED  STATUS    LAST_ROTATED  EXPIRES
# kalshi    api_key   yes       set       2025-12-14    2026-03-14
# nats      transport yes       set       2025-12-14    -
# storage   storage   no        not_set   -             -

# Verify all required keys are set
ssmd key verify kalshi-prod
# ✓ kalshi: set (expires in 90 days)
# ✓ nats: set
# ○ storage: not set (optional)
# All required keys present.

# Show key metadata (not the value)
ssmd key show kalshi-prod kalshi
# Name: kalshi
# Type: api_key
# Status: set
# Fields: api_key, api_secret
# Last Rotated: 2025-12-14T10:30:00Z
# Rotation Policy: 90 days
# Expires: 2026-03-14T10:30:00Z
# Sealed Secret: ssmd/kalshi-prod-kalshi

# Rotate a key
ssmd key rotate kalshi-prod kalshi --api-key NEW_KEY --api-secret NEW_SECRET

# Delete a key
ssmd key delete kalshi-prod storage

# Export key references (for GitOps, no actual secrets)
ssmd key export kalshi-prod > keys-manifest.yaml
```

## Key Validation

Before any deployment, all key references are validated:

```bash
$ ssmd env apply kalshi.yaml

Validating environment...
  ✓ Keys section present
  ✓ Key 'kalshi' defined (api_key, required)
  ✓ Key 'nats' defined (transport, required)
  ○ Key 'storage' defined (storage, optional)

Checking key status...
  ✓ Key 'kalshi' is set
  ✗ Key 'nats' is NOT SET

Error: Required key 'nats' is not set.
Run: ssmd key set kalshi-prod nats --url xxx --username yyy --password zzz
```

## Key References in Config

Components reference keys by name, never raw values:

```yaml
# Connector config references key by name
connector:
  feed: kalshi
  credentials: $key:kalshi    # Resolved at runtime

# Middleware references keys
middleware:
  transport:
    type: nats
    credentials: $key:nats    # Resolved at runtime
  storage:
    type: s3
    credentials: $key:storage # Optional, only if set
```

## Runtime Key Resolution

Components resolve keys at startup from Kubernetes Sealed Secrets:

```rust
pub struct KeyResolver {
    k8s_client: kube::Client,
    cache: HashMap<String, KeyValue>,
}

impl KeyResolver {
    pub async fn resolve(&self, key_ref: &str) -> Result<KeyValue, KeyError> {
        // Parse reference: "$key:kalshi" -> "kalshi"
        let key_name = key_ref.strip_prefix("$key:").ok_or(KeyError::InvalidRef)?;

        // Check cache
        if let Some(cached) = self.cache.get(key_name) {
            return Ok(cached.clone());
        }

        // Load from Sealed Secret
        let secret_name = format!("{}-{}", self.environment, key_name);
        let secret = self.k8s_client
            .get::<Secret>(&secret_name, &self.namespace)
            .await?;

        let value = KeyValue::from_secret(&secret)?;
        self.cache.insert(key_name.to_string(), value.clone());

        // Log access
        self.audit_log(key_name, "accessed").await;

        Ok(value)
    }
}
```

## Key Expiration Alerts

```yaml
# Prometheus alert for expiring keys
- alert: KeyExpiringSoon
  expr: ssmd_key_expires_in_days < 14
  for: 1h
  labels:
    severity: warning
  annotations:
    summary: "Key {{ $labels.key_name }} expires in {{ $value }} days"
    runbook: "Run: ssmd key rotate {{ $labels.environment }} {{ $labels.key_name }}"

- alert: KeyExpired
  expr: ssmd_key_expires_in_days < 0
  for: 5m
  labels:
    severity: critical
  annotations:
    summary: "Key {{ $labels.key_name }} has EXPIRED"
```

## Workflow: New Environment Setup

```bash
# 1. Create environment file with key definitions
ssmd env create kalshi-prod --feed kalshi --schema trade:v1

# 2. Edit to add key definitions (or use ssmd env add-key)
ssmd env add-key kalshi-prod kalshi --type api_key --fields api_key,api_secret

# 3. Set the actual key values
ssmd key set kalshi-prod kalshi --api-key xxx --api-secret yyy

# 4. Verify all keys set
ssmd key verify kalshi-prod

# 5. Deploy environment (keys validated automatically)
ssmd env apply exchanges/environments/kalshi-prod.yaml
```
