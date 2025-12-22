# Schema Normalization Design

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Normalize protocol handling and generalize capture location naming in Go and Rust schemas.

**Architecture:** Update shared metadata types in both Go (`internal/types/feed.go`) and Rust (`ssmd-rust/crates/metadata/src/feed.rs`) to use structured protocol objects and more flexible location terminology.

**Tech Stack:** Go, Rust, YAML serialization

---

## Changes

### 1. Protocol Normalization

Replace the free-form `protocol` string in `FeedVersion` with a structured object that separates transport from message protocol.

**Before:**
```yaml
versions:
  - version: v1
    protocol: wss
    endpoint: wss://api.example.com/ws/v2
```

**After:**
```yaml
versions:
  - version: v1
    protocol:
      transport: wss
      message: json
      version: "2.0"
    endpoint: wss://api.example.com/ws/v2
```

#### TransportProtocol Enum

| Value | Description |
|-------|-------------|
| `wss` | WebSocket Secure |
| `https` | HTTPS/REST |
| `multicast` | UDP Multicast |
| `tcp` | Raw TCP |

#### MessageProtocol Enum

| Value | Description |
|-------|-------------|
| `json` | JSON messages |
| `itch` | Nasdaq ITCH (e.g., TotalView) |
| `fix` | FIX Protocol |
| `sbe` | Simple Binary Encoding |
| `protobuf` | Protocol Buffers |

---

### 2. CaptureLocation Generalization

Rename `datacenter` to `site` and add explicit `type` field to handle different deployment scenarios.

**Before:**
```yaml
capture_locations:
  - datacenter: ny5
    provider: equinix
    region: nyc
```

**After:**
```yaml
capture_locations:
  - site: ny5
    type: colo
    provider: equinix
    region: nyc
    clock: null
```

#### SiteType Enum

| Value | Description |
|-------|-------------|
| `cloud` | Cloud provider zones (AWS, GCP, Azure) |
| `colo` | Colocation facilities (Equinix, NY4/NY5) |
| `on_prem` | On-premises, homelab, self-managed |

#### ClockType Enum (Future - not implemented now)

| Value | Description |
|-------|-------------|
| `ptp` | Precision Time Protocol |
| `gps` | GPS-disciplined clock |
| `ntp` | Network Time Protocol |
| `local` | System clock only |

The `clock` field will be defined as optional in the schema but not used until a future implementation phase.

---

## Type Definitions

### Go

```go
// TransportProtocol represents the network transport
type TransportProtocol string

const (
    TransportWSS       TransportProtocol = "wss"
    TransportHTTPS     TransportProtocol = "https"
    TransportMulticast TransportProtocol = "multicast"
    TransportTCP       TransportProtocol = "tcp"
)

// MessageProtocol represents the wire format
type MessageProtocol string

const (
    MessageJSON     MessageProtocol = "json"
    MessageITCH     MessageProtocol = "itch"
    MessageFIX      MessageProtocol = "fix"
    MessageSBE      MessageProtocol = "sbe"
    MessageProtobuf MessageProtocol = "protobuf"
)

// Protocol represents connection and message protocols
type Protocol struct {
    Transport TransportProtocol `yaml:"transport"`
    Message   MessageProtocol   `yaml:"message"`
    Version   string            `yaml:"version,omitempty"`
}

// SiteType represents the type of capture location
type SiteType string

const (
    SiteTypeCloud  SiteType = "cloud"
    SiteTypeColo   SiteType = "colo"
    SiteTypeOnPrem SiteType = "on_prem"
)

// CaptureLocation represents where feed data is captured
type CaptureLocation struct {
    Site     string   `yaml:"site"`
    Type     SiteType `yaml:"type"`
    Provider string   `yaml:"provider,omitempty"`
    Region   string   `yaml:"region,omitempty"`
    Clock    string   `yaml:"clock,omitempty"` // future use
}
```

### Rust

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum TransportProtocol {
    Wss,
    Https,
    Multicast,
    Tcp,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum MessageProtocol {
    Json,
    Itch,
    Fix,
    Sbe,
    Protobuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Protocol {
    pub transport: TransportProtocol,
    pub message: MessageProtocol,
    pub version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SiteType {
    Cloud,
    Colo,
    OnPrem,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaptureLocation {
    pub site: String,
    #[serde(rename = "type")]
    pub site_type: SiteType,
    pub provider: Option<String>,
    pub region: Option<String>,
    pub clock: Option<String>, // future use
}
```

---

## Migration Notes

- Existing YAML files with `protocol: wss` will need migration to the nested format
- Existing YAML files with `datacenter:` will need field renamed to `site:` and `type:` added
- Both Go and Rust should validate the new enum values
- The `clock` field is optional and unused for now

---

## Files to Modify

| File | Changes |
|------|---------|
| `internal/types/feed.go` | Add Protocol struct, update CaptureLocation, add enums |
| `ssmd-rust/crates/metadata/src/feed.rs` | Add Protocol struct, update CaptureLocation, add enums |
| `configs/exchanges/kalshi/feeds/*.yaml` | Migrate to new format (if any exist) |

---

## TODO / Future Work

### Sequenced Stream Handling

Many market data protocols (ITCH, SBE, multicast feeds) use sequence numbers for gap detection and recovery. We need to:

1. **Distinguish sequenced vs unsequenced streams** in the protocol metadata
   - Add `sequenced: bool` to Protocol struct
   - Or infer from message protocol (ITCH is always sequenced, JSON often isn't)

2. **Sequence number checking** at the connector level
   - Track last seen sequence number per stream
   - Detect gaps (missing sequence numbers)
   - Support gap-fill requests where protocol allows
   - Log/alert on sequence gaps

3. **Recovery mechanisms**
   - Request retransmission (if supported)
   - Snapshot + replay for order book recovery
   - Mark data as potentially incomplete when gaps detected

Example schema addition:
```yaml
protocol:
  transport: multicast
  message: itch
  version: "5.0"
  sequenced: true
  sequence_field: "seq_num"  # field name in message
```
