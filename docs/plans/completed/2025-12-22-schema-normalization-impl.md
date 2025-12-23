# Schema Normalization Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Update Go and Rust schemas to use structured Protocol objects and generalized CaptureLocation.

**Architecture:** Add new enum types and structs to both Go and Rust, update existing types to use them, update tests and validation.

**Tech Stack:** Go, Rust, serde, YAML

---

### Task 1: Add Protocol types to Go

**Files:**
- Modify: `internal/types/feed.go`

**Step 1: Add TransportProtocol and MessageProtocol enums**

Add after the existing AuthMethod constants (around line 36):

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
```

**Step 2: Update FeedVersion to use Protocol struct**

Change the `Protocol` field in FeedVersion from `string` to `Protocol`:

```go
// FeedVersion represents a version of feed configuration
type FeedVersion struct {
	Version                 string            `yaml:"version"`
	EffectiveFrom           string            `yaml:"effective_from"`
	EffectiveTo             string            `yaml:"effective_to,omitempty"`
	Protocol                Protocol          `yaml:"protocol"`
	Endpoint                string            `yaml:"endpoint"`
	AuthMethod              AuthMethod        `yaml:"auth_method,omitempty"`
	RateLimitPerSecond      int               `yaml:"rate_limit_per_second,omitempty"`
	MaxSymbolsPerConnection int               `yaml:"max_symbols_per_connection,omitempty"`
	SupportsOrderbook       bool              `yaml:"supports_orderbook,omitempty"`
	SupportsTrades          bool              `yaml:"supports_trades,omitempty"`
	SupportsHistorical      bool              `yaml:"supports_historical,omitempty"`
	ParserConfig            map[string]string `yaml:"parser_config,omitempty"`
}
```

**Step 3: Run Go tests**

Run: `go test ./internal/types/...`
Expected: Tests may fail due to schema change - we'll fix in Task 3

**Step 4: Commit**

```bash
git add internal/types/feed.go
git commit -m "feat(go): add Protocol struct with transport and message enums"
```

---

### Task 2: Update CaptureLocation in Go

**Files:**
- Modify: `internal/types/feed.go`

**Step 1: Add SiteType enum**

Add after the Protocol struct:

```go
// SiteType represents the type of capture location
type SiteType string

const (
	SiteTypeCloud  SiteType = "cloud"
	SiteTypeColo   SiteType = "colo"
	SiteTypeOnPrem SiteType = "on_prem"
)
```

**Step 2: Update CaptureLocation struct**

Replace the existing CaptureLocation struct:

```go
// CaptureLocation represents where feed data is captured
type CaptureLocation struct {
	Site     string   `yaml:"site"`
	Type     SiteType `yaml:"type"`
	Provider string   `yaml:"provider,omitempty"`
	Region   string   `yaml:"region,omitempty"`
	Clock    string   `yaml:"clock,omitempty"` // future: ptp, gps, ntp, local
}
```

**Step 3: Commit**

```bash
git add internal/types/feed.go
git commit -m "feat(go): update CaptureLocation with site and type fields"
```

---

### Task 3: Update Go validation

**Files:**
- Modify: `internal/types/feed.go`

**Step 1: Add Protocol validation in Validate()**

Update the Validate() function to check Protocol fields. Add after endpoint validation (around line 137):

```go
// Validate protocol
if v.Protocol.Transport == "" {
	return fmt.Errorf("version %s: protocol.transport is required", v.Version)
}
switch v.Protocol.Transport {
case TransportWSS, TransportHTTPS, TransportMulticast, TransportTCP:
	// valid
default:
	return fmt.Errorf("version %s: invalid protocol.transport: %s", v.Version, v.Protocol.Transport)
}
if v.Protocol.Message == "" {
	return fmt.Errorf("version %s: protocol.message is required", v.Version)
}
switch v.Protocol.Message {
case MessageJSON, MessageITCH, MessageFIX, MessageSBE, MessageProtobuf:
	// valid
default:
	return fmt.Errorf("version %s: invalid protocol.message: %s", v.Version, v.Protocol.Message)
}
```

**Step 2: Run Go tests**

Run: `go test ./internal/types/...`
Expected: May still fail - test fixtures need updating

**Step 3: Commit**

```bash
git add internal/types/feed.go
git commit -m "feat(go): add validation for Protocol and CaptureLocation"
```

---

### Task 4: Update Go tests

**Files:**
- Modify: `internal/types/feed_test.go`

**Step 1: Check if feed_test.go exists and update test fixtures**

Run: `ls -la internal/types/feed_test.go`

If exists, update test YAML fixtures to use new schema format:

```yaml
versions:
  - version: v1
    effective_from: "2025-01-01"
    protocol:
      transport: wss
      message: json
    endpoint: wss://example.com/ws
```

**Step 2: Run Go tests**

Run: `go test ./internal/types/... -v`
Expected: PASS

**Step 3: Commit**

```bash
git add internal/types/
git commit -m "test(go): update feed tests for new schema"
```

---

### Task 5: Add Protocol types to Rust

**Files:**
- Modify: `ssmd-rust/crates/metadata/src/feed.rs`

**Step 1: Add TransportProtocol and MessageProtocol enums**

Add after AuthMethod enum (around line 32):

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
```

**Step 2: Update FeedVersion to use Protocol struct**

Change the `protocol` field from `String` to `Protocol`:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeedVersion {
    pub version: String,
    pub effective_from: String,
    pub effective_to: Option<String>,
    pub protocol: Protocol,
    pub endpoint: String,
    pub auth_method: Option<AuthMethod>,
    pub rate_limit_per_second: Option<i32>,
    pub max_symbols_per_connection: Option<i32>,
    pub supports_orderbook: Option<bool>,
    pub supports_trades: Option<bool>,
    pub supports_historical: Option<bool>,
    pub parser_config: Option<HashMap<String, String>>,
}
```

**Step 3: Commit**

```bash
git add ssmd-rust/crates/metadata/src/feed.rs
git commit -m "feat(rust): add Protocol struct with transport and message enums"
```

---

### Task 6: Update CaptureLocation in Rust

**Files:**
- Modify: `ssmd-rust/crates/metadata/src/feed.rs`

**Step 1: Add SiteType enum**

Add after the Protocol struct:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SiteType {
    Cloud,
    Colo,
    OnPrem,
}
```

**Step 2: Update CaptureLocation struct**

Replace the existing CaptureLocation struct:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaptureLocation {
    pub site: String,
    #[serde(rename = "type")]
    pub site_type: SiteType,
    pub provider: Option<String>,
    pub region: Option<String>,
    pub clock: Option<String>, // future: ptp, gps, ntp, local
}
```

**Step 3: Commit**

```bash
git add ssmd-rust/crates/metadata/src/feed.rs
git commit -m "feat(rust): update CaptureLocation with site and type fields"
```

---

### Task 7: Update Rust tests

**Files:**
- Modify: `ssmd-rust/crates/metadata/src/feed.rs`

**Step 1: Update test fixtures in feed.rs**

Update the test YAML in `test_load_feed()`:

```rust
#[test]
fn test_load_feed() {
    let mut file = tempfile::NamedTempFile::new().unwrap();
    writeln!(
        file,
        r#"
name: kalshi
display_name: Kalshi Exchange
type: websocket
status: active
versions:
  - version: v1
    effective_from: "2025-12-22"
    protocol:
      transport: wss
      message: json
    endpoint: wss://api.kalshi.com/trade-api/ws/v2
    auth_method: api_key
"#
    )
    .unwrap();

    let feed = Feed::load(file.path()).unwrap();
    assert_eq!(feed.name, "kalshi");
    assert_eq!(feed.feed_type, FeedType::Websocket);
    assert!(feed.get_latest_version().is_some());
}
```

**Step 2: Update test_version_for_date() test**

Update the FeedVersion construction:

```rust
FeedVersion {
    version: "v1".to_string(),
    effective_from: "2025-01-01".to_string(),
    effective_to: Some("2025-06-30".to_string()),
    protocol: Protocol {
        transport: TransportProtocol::Wss,
        message: MessageProtocol::Json,
        version: None,
    },
    endpoint: "wss://v1".to_string(),
    auth_method: None,
    rate_limit_per_second: None,
    max_symbols_per_connection: None,
    supports_orderbook: None,
    supports_trades: None,
    supports_historical: None,
    parser_config: None,
},
```

**Step 3: Run Rust tests**

Run: `source ~/.cargo/env && cd ssmd-rust && cargo test`
Expected: PASS

**Step 4: Commit**

```bash
git add ssmd-rust/crates/metadata/src/feed.rs
git commit -m "test(rust): update feed tests for new schema"
```

---

### Task 8: Update Rust metadata lib.rs exports

**Files:**
- Modify: `ssmd-rust/crates/metadata/src/lib.rs`

**Step 1: Add new types to exports**

Update the feed exports:

```rust
pub use feed::{
    AuthMethod, Calendar, CaptureLocation, Feed, FeedStatus, FeedType, FeedVersion,
    MessageProtocol, Protocol, SiteType, TransportProtocol,
};
```

**Step 2: Run cargo build**

Run: `source ~/.cargo/env && cd ssmd-rust && cargo build`
Expected: PASS

**Step 3: Commit**

```bash
git add ssmd-rust/crates/metadata/src/lib.rs
git commit -m "feat(rust): export new Protocol and SiteType types"
```

---

### Task 9: Run full test suite

**Step 1: Run Go tests**

Run: `go test ./...`
Expected: PASS

**Step 2: Run Rust tests**

Run: `source ~/.cargo/env && cd ssmd-rust && cargo test --all`
Expected: PASS

**Step 3: Run Rust clippy**

Run: `source ~/.cargo/env && cd ssmd-rust && cargo clippy --all`
Expected: No warnings

---

### Task 10: Final commit and push

**Step 1: Push to PR**

```bash
git push
```

**Step 2: Verify PR updated**

Check: https://github.com/aaronwald/ssmd/pull/8
