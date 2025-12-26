# ssmd: Kalshi Design - Agent Integration

AI agents (Claude, custom bots) interact with ssmd through structured APIs. The system is designed to be agent-friendly: queryable, explainable, and actionable.

## Integration Points

```
┌─────────────────────────────────────────────────────────────────────┐
│                           AI AGENT                                   │
│  (Claude Code, Custom Bot, Notebook)                                │
└───────────┬──────────────────┬──────────────────┬───────────────────┘
            │                  │                  │
            ▼                  ▼                  ▼
     ┌─────────────┐   ┌─────────────┐   ┌─────────────┐
     │  MCP Server │   │   REST API  │   │  WebSocket  │
     │  (tools)    │   │  (queries)  │   │  (stream)   │
     └──────┬──────┘   └──────┬──────┘   └──────┬──────┘
            │                 │                 │
            └────────────────┼─────────────────┘
                             │
                      ┌──────▼──────┐
                      │   Gateway   │
                      └─────────────┘
```

## MCP Server (ssmd-mcp)

Model Context Protocol server exposes ssmd as tools for Claude:

```go
// ssmd-mcp implements MCP server protocol
type SSMDServer struct {
    gateway  *GatewayClient
    secmaster *SecmasterClient
}

// Tools exposed to Claude
var Tools = []mcp.Tool{
    {
        Name:        "ssmd_list_markets",
        Description: "List available markets with optional filters",
        InputSchema: schema.Object{
            "feed":     schema.String{Description: "Filter by feed (kalshi, polymarket)"},
            "status":   schema.String{Description: "Filter by status (active, expired)"},
            "category": schema.String{Description: "Filter by category"},
        },
    },
    {
        Name:        "ssmd_get_market",
        Description: "Get details for a specific market including current price",
        InputSchema: schema.Object{
            "ticker": schema.String{Required: true, Description: "Market ticker"},
        },
    },
    {
        Name:        "ssmd_get_trades",
        Description: "Get recent trades for a market",
        InputSchema: schema.Object{
            "ticker": schema.String{Required: true},
            "limit":  schema.Integer{Default: 100, Max: 1000},
            "since":  schema.String{Description: "ISO timestamp"},
        },
    },
    {
        Name:        "ssmd_get_orderbook",
        Description: "Get current orderbook for a market",
        InputSchema: schema.Object{
            "ticker": schema.String{Required: true},
            "depth":  schema.Integer{Default: 10, Max: 50},
        },
    },
    {
        Name:        "ssmd_query_historical",
        Description: "Query historical data for backtesting",
        InputSchema: schema.Object{
            "ticker":     schema.String{Required: true},
            "start_date": schema.String{Required: true, Description: "YYYY-MM-DD"},
            "end_date":   schema.String{Required: true, Description: "YYYY-MM-DD"},
            "interval":   schema.String{Default: "1m", Description: "1m, 5m, 1h, 1d"},
        },
    },
    {
        Name:        "ssmd_report_issue",
        Description: "Report a data quality issue for investigation",
        InputSchema: schema.Object{
            "ticker":      schema.String{Required: true},
            "issue_type":  schema.String{Required: true, Enum: []string{"missing_data", "incorrect_price", "duplicate", "other"}},
            "description": schema.String{Required: true},
            "timestamp":   schema.String{Description: "When the issue occurred"},
            "evidence":    schema.String{Description: "Supporting data or observations"},
        },
    },
    {
        Name:        "ssmd_system_status",
        Description: "Get current system health and data coverage",
        InputSchema: schema.Object{},
    },
    {
        Name:        "ssmd_data_inventory",
        Description: "Check what data is available for a date range",
        InputSchema: schema.Object{
            "feed":       schema.String{Required: true},
            "start_date": schema.String{Required: true},
            "end_date":   schema.String{Required: true},
        },
    },
}
```

## Agent-Friendly Responses

Responses include context that helps agents understand and act:

```json
{
  "ticker": "INXD-25-B4000",
  "title": "Will S&P 500 close above 4000 on Dec 31, 2025?",
  "feed": "kalshi",
  "status": "active",
  "current_price": {
    "yes": 0.45,
    "no": 0.55,
    "last_trade": 0.45,
    "last_trade_time": "2025-12-14T10:30:00Z"
  },
  "orderbook_summary": {
    "best_bid": 0.44,
    "best_ask": 0.46,
    "spread": 0.02,
    "bid_depth": 1500,
    "ask_depth": 2000
  },
  "contract": {
    "expiration": "2025-12-31T23:59:59Z",
    "settlement": "2026-01-01T12:00:00Z",
    "days_to_expiry": 17
  },
  "data_quality": {
    "status": "healthy",
    "last_update": "2025-12-14T10:30:01Z",
    "gaps_today": 0
  },
  "_links": {
    "trades": "/v1/markets/INXD-25-B4000/trades",
    "orderbook": "/v1/markets/INXD-25-B4000/orderbook",
    "historical": "/v1/markets/INXD-25-B4000/history"
  },
  "_hints": {
    "price_interpretation": "0.45 yes price implies 45% probability of S&P > 4000",
    "suggested_actions": [
      "Use ssmd_get_trades to see recent activity",
      "Use ssmd_query_historical for trend analysis"
    ]
  }
}
```

## Agent Feedback Loop

Agents can report data quality issues that feed back into the system:

```rust
pub struct AgentFeedback {
    pub id: String,
    pub agent_id: String,           // MCP client identifier
    pub ticker: String,
    pub issue_type: IssueType,      // missing_data, incorrect_price, duplicate, other
    pub description: String,
    pub timestamp: Option<DateTime<Utc>>,
    pub evidence: Option<serde_json::Value>,
    pub status: FeedbackStatus,     // open, investigating, resolved, invalid
    pub created_at: DateTime<Utc>,
}
```

Feedback is stored in the journal for processing:

```
Subject: {env}.agent.feedback

{
  "id": "fb-123",
  "agent_id": "claude-code-abc",
  "ticker": "INXD-25-B4000",
  "issue_type": "missing_data",
  "description": "No trades between 14:30-14:35",
  "timestamp": "2025-12-14T14:30:00Z",
  "created_at": "2025-12-14T15:00:00Z"
}
```

### Feedback Workflow

```
Agent reports issue
       │
       ▼
┌─────────────────┐
│ Publish to      │
│ feedback journal│
└────────┬────────┘
         │
         ▼
┌─────────────────┐     ┌─────────────────┐
│ Auto-triage:    │────▶│ Link to existing│
│ duplicate?      │ yes │ issue           │
└────────┬────────┘     └─────────────────┘
         │ no
         ▼
┌─────────────────┐     ┌─────────────────┐
│ Auto-validate:  │────▶│ Mark invalid,   │
│ data exists?    │ no  │ notify agent    │
└────────┬────────┘     └─────────────────┘
         │ yes
         ▼
┌─────────────────┐
│ Create Linear   │
│ issue (if high  │
│ priority)       │
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│ Investigate &   │
│ resolve         │
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│ Update agent    │
│ via webhook     │
└─────────────────┘
```

## Natural Language Queries

Gateway supports natural language queries that get translated to structured queries:

```json
// Agent sends
{
  "action": "query",
  "natural_language": "What prediction markets about Bitcoin are trading today?"
}

// Gateway translates to
{
  "action": "list_markets",
  "filters": {
    "category": "crypto",
    "underlying": "BTC",
    "status": "active"
  }
}

// And returns
{
  "interpretation": "Searching for active markets related to Bitcoin",
  "results": [...],
  "suggestions": [
    "To narrow down: 'Bitcoin price markets expiring this week'",
    "For specific market: 'ssmd_get_market KXBTC-25DEC31'"
  ]
}
```

## Rate Limiting for Agents

Agents have separate rate limits from real-time streaming:

```yaml
rate_limits:
  agents:
    requests_per_minute: 60
    requests_per_hour: 1000
    burst: 10

  # Higher limits for feedback (we want bug reports)
  feedback:
    requests_per_minute: 10
    requests_per_hour: 100
```

## Claude Code Integration

Example Claude Code session:

```
Human: What's the current price of the S&P 4000 prediction market on Kalshi?

Claude: I'll check the current market data.

[Calls ssmd_get_market with ticker pattern matching "S&P 4000"]

The S&P 500 above 4000 market (INXD-25-B4000) is currently trading at:
- Yes: $0.45 (45% implied probability)
- No: $0.55
- Spread: $0.02

The market expires on Dec 31, 2025 (17 days). Last trade was 2 minutes ago.
```

## CLI for Feedback Management

```bash
# List agent feedback
ssmd feedback list
# ID       AGENT          TICKER          TYPE          STATUS
# fb-123   claude-code    INXD-25-B4000   missing_data  open
# fb-124   bot-alpha      BTCUSD          incorrect     investigating

# Show feedback details
ssmd feedback show fb-123

# Update feedback status
ssmd feedback resolve fb-123 --resolution "Gap confirmed, connector reconnected"

# Mark as invalid
ssmd feedback invalid fb-124 --reason "Data verified correct"
```
