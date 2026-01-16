# Kalshi API Data Model

## Hierarchy

```
Category
  └── Tags (groupings within category)
        └── Series (e.g., "NBA Games", "EPL Games")
              └── Events (e.g., "OKC vs HOU Jan 16")
                    └── Markets (e.g., "OKC to win", "Total points O/U")
```

## What is a Series?

A **series** represents a template for recurring events that follow the same format and rules.

Examples:
- "Monthly Jobs Report" - recurring economic events
- "Weekly Initial Jobless Claims" - recurring employment data
- "Daily Weather in NYC" - recurring weather events
- "Professional Basketball Game" (`KXNBAGAME`) - recurring NBA games

The `/series` endpoint allows browsing and discovering available series templates by category. Each series generates multiple events over time.

## Key Endpoints

### Discovery Endpoints

| Endpoint | Purpose | Response Time |
|----------|---------|---------------|
| `GET /search/tags_by_categories` | Get all tags grouped by category | ~200ms |
| `GET /search/filters_by_sport` | Get sports, competitions, and scopes | ~200ms |
| `GET /series` | List series (supports category filter) | ~200ms per page |

### Series Endpoint Filters

`GET /series` supports these query parameters:

| Parameter | Type | Description |
|-----------|------|-------------|
| `category` | string | Filter series by category |
| `tags` | string | Filter series by tags |
| `include_product_metadata` | boolean | Include internal product metadata (default: false) |
| `include_volume` | boolean | Include total volume traded across all events in series (default: false) |

### Data Endpoints

| Endpoint | Purpose |
|----------|---------|
| `GET /series?category={cat}` | Get series for a category |
| `GET /series?category={cat}&tags={tag}` | Get series for a category filtered by tag |
| `GET /events?series_ticker={ticker}` | Get events for a series |
| `GET /markets?series_ticker={ticker}` | Get markets for a series |

## Categories We Care About

| Category | Series Count | Notes |
|----------|--------------|-------|
| Economics | 405 | Fed, inflation, employment |
| Elections | 537 | US and international |
| Entertainment | 2,181 | Awards, movies, music |
| Financials | 170 | S&P, Nasdaq, forex |
| Politics | 2,689 | Trump, Congress, SCOTUS |
| Sports | 1,061 | Games, futures, awards |

## Tags by Category

```json
{
  "Economics": ["Fed", "Oil and energy", "Growth", "Inflation", "Employment", "Housing", "Mortgages"],
  "Elections": ["International elections"],
  "Entertainment": ["Awards", "Movies", "Music", "Music charts", "Television", "Video games", "Rotten Tomatoes", "Live Music"],
  "Financials": ["S&P", "Daily", "Nasdaq", "EUR/USD", "USD/JPY", "Treasuries", "WTI"],
  "Politics": ["US Elections", "Primaries", "Trump", "Foreign Elections", "International", "House", "Congress", "SCOTUS & courts", "Local", "Recurring"],
  "Sports": ["Soccer", "Basketball", "Football", "Baseball", "Hockey", "Golf", "Esports", "Tennis", "UFC", "MMA", "Cricket", "Motorsport", "Chess", "Table Tennis"]
}
```

## Sports Filters Structure

Sports has additional filtering by **competition** and **scope**:

### Competitions by Sport

| Sport | Key Competitions |
|-------|------------------|
| Basketball | Pro Basketball (M), College Basketball (M/W), Euroleague, Unrivaled |
| Football | Pro Football, College Football, College Football Playoffs |
| Soccer | EPL, La Liga, Bundesliga, Serie A, Ligue 1, UCL, FA Cup |
| Hockey | Pro Hockey, AHL, College Hockey, KHL |
| Baseball | Pro Baseball |
| Tennis | ATP tournaments, WTA tournaments, Grand Slams |
| Golf | PGA tournaments, TGL |
| MMA | UFC |
| Esports | CS2, League of Legends, Valorant, Call of Duty |

### Scopes (Market Types)

| Scope | Description |
|-------|-------------|
| **Games** | Individual game outcomes (winner, spread, total) |
| Futures | Season-long outcomes (champion, MVP, win totals) |
| Awards | Individual awards (MVP, ROY, etc.) |
| 1st Half Winner/Spread/Total | First half props |
| Divisions | Division winners |
| To Advance | Playoff advancement |
| Events | Special events |

## Recommended Sync Strategy

### For Non-Sports Categories

Sync all series → events → markets for:
- Economics
- Elections
- Entertainment
- Financials
- Politics

### For Sports

Filter to **Games** scope only to get time-sensitive game markets:

| Competition | Why Include |
|-------------|-------------|
| Pro Basketball (M) | NBA games |
| Pro Football | NFL games |
| Pro Hockey | NHL games |
| EPL, La Liga, Serie A, Bundesliga, Ligue 1 | Top soccer leagues |
| UCL | Champions League |

Exclude futures, awards, draft picks, etc. - these don't need real-time tracking.

## Series → Events → Markets Flow

```
1. Fetch series for category
   GET /series?category=Sports

2. For each series we care about, fetch events
   GET /events?series_ticker=KXNBAGAME

3. For each event, fetch markets (or fetch by series)
   GET /markets?event_ticker=KXNBAGAME-26JAN15OKCHOU
   OR
   GET /markets?series_ticker=KXNBAGAME
```

## Performance Observations

- Series endpoint returns all results in one page (no pagination needed for <3000 series)
- Rate limit: ~6 requests per second at 100ms delay, then 429
- Safe rate: 1 request per second (1000ms delay)
- Full series fetch for all categories: ~2 seconds total

## Sports Series by Tag

Using `GET /series?category=Sports&tags={tag}` (~40-100ms per request):

| Tag | Total Series | Game Series |
|-----|--------------|-------------|
| Basketball | 143 | 18 |
| Football | 232 | 8 |
| Hockey | 38 | 6 |
| Baseball | 75 | 3 |
| Soccer | 185 | 43 |
| Tennis | 71 | 11 |
| Golf | 30 | 2 |
| Esports | 87 | 10 |

## Sports Game Series Tickers

Fetched from `GET /series?category=Sports&tags={tag}` filtered for game-related series.

### US Pro Sports

| Ticker | Title |
|--------|-------|
| `KXNBAGAME` | Professional Basketball Game |
| `KXNFLGAME` | Professional Football Game |
| `KXNHLGAME` | NHL Game |
| `KXMLBGAME` | Professional Baseball Game |
| `KXWNBAGAME` | Professional Women's Basketball Game |

### US College Sports

| Ticker | Title |
|--------|-------|
| `KXNCAAMBGAME` | Men's College Basketball Men's Game |
| `KXNCAAWBGAME` | College Basketball Women's Game |
| `KXNCAAFGAME` | College Football Game |
| `KXNCAAHOCKEYGAME` | College Hockey Game |

### European Soccer

| Ticker | Title |
|--------|-------|
| `KXEPLGAME` | English Premier League Game |
| `KXLALIGAGAME` | La Liga Game |
| `KXSERIEAGAME` | Serie A Game |
| `KXBUNDESLIGAGAME` | Bundesliga Game |
| `KXLIGUE1GAME` | Ligue 1 Game |
| `KXUCLGAME` | UEFA Champions League Game |
| `KXUELGAME` | UEFA Europa League Game |
| `KXFACUPGAME` | FA Cup Game |
| `KXEREDIVISIEGAME` | Eredivisie Game |
| `KXLIGAPORTUGALGAME` | Liga Portugal Game |

### Other Notable

| Ticker | Title |
|--------|-------|
| `KXUNRIVALEDGAME` | Unrivaled Basketball Game |
| `KXEUROLEAGUEGAME` | Euroleague Game |
| `KXMLSGAME` | Major League Soccer Game |
| `KXLIGAMXGAME` | Liga MX Game |
| `KXTGLMATCH` | TGL Golf Match |

### Esports

| Ticker | Title |
|--------|-------|
| `KXLOLGAME` | League of Legends Game |
| `KXCS2GAME` | Counter-Strike 2 Game |
| `KXVALORANTGAME` | Valorant game winner |
| `KXCODGAME` | Call of Duty Games |
| `KXDOTA2GAME` | Dota 2 Game |

### Tennis

| Ticker | Title |
|--------|-------|
| `KXATPGAME` | ATP Tennis Winner |
| `KXATPMATCH` | ATP Tennis Match |
| `KXWTAGAME` | WTA Tennis Winner |
| `KXWTAMATCH` | WTA Tennis Match |

## Recommended Series to Track

### Tier 1 (US Major Sports)

```
KXNBAGAME, KXNFLGAME, KXNHLGAME, KXMLBGAME
```

### Tier 2 (College + Women's)

```
KXNCAAMBGAME, KXNCAAWBGAME, KXNCAAFGAME, KXWNBAGAME, KXUNRIVALEDGAME
```

### Tier 3 (Soccer)

```
KXEPLGAME, KXLALIGAGAME, KXSERIEAGAME, KXBUNDESLIGAGAME, KXLIGUE1GAME, KXUCLGAME
```

## Series-Based Sync Strategy

### Three Queries Per Series

| Query | Filter | Purpose |
|-------|--------|---------|
| 1. Open | `series_ticker={ticker}&status=open` | Markets to subscribe to |
| 2. Closed | `series_ticker={ticker}&status=closed&min_close_ts={24h ago}&max_close_ts={now}` | Markets to unsubscribe from |
| 3. Settled | `series_ticker={ticker}&status=settled&min_settled_ts={24h ago}` | Record final results |

### Example Results (Jan 16, 2026)

| Series | Open | Closed (24h) | Settled (24h) | Total Time |
|--------|------|--------------|---------------|------------|
| KXNBAGAME | 46 | 0 | 16 | ~600ms |
| KXNFLGAME | 8 | 0 | 0 | ~500ms |
| KXNHLGAME | 58 | 0 | 8 | ~580ms |
| KXNCAAMBGAME | 390 | 8 | 122 | ~2.3s |
| KXEPLGAME | 30 | 0 | 0 | ~500ms |

### Benefits Over Time-Based Sync

| Approach | Markets Returned | Time |
|----------|------------------|------|
| `close_within_hours=48` (all categories) | 3600+ | Minutes, rate limited |
| `series_ticker=KXNBAGAME&status=open` | 46 | 166ms |

Series-based sync is:
- **Targeted**: Only fetches markets we care about
- **Fast**: Single page for most series
- **Predictable**: No surprise volume spikes

### Sync Flow

```
For each series_ticker in config:
  1. GET /markets?series_ticker={ticker}&status=open
     → Upsert to DB, add to subscription list

  2. GET /markets?series_ticker={ticker}&status=closed&min_close_ts={24h_ago}&max_close_ts={now}
     → Update status in DB, remove from subscription list

  3. GET /markets?series_ticker={ticker}&status=settled&min_settled_ts={24h_ago}
     → Update status and result in DB
```

### Recommended Series for Sports Connector

```yaml
# Tier 1: US Pro Sports
- KXNBAGAME      # NBA (~50 open)
- KXNFLGAME     # NFL (~8 open during season)
- KXNHLGAME     # NHL (~60 open)
- KXMLBGAME     # MLB (seasonal)

# Tier 2: College
- KXNCAAMBGAME  # College Basketball (~400 open)
- KXNCAAFGAME   # College Football (seasonal)

# Tier 3: Soccer
- KXEPLGAME     # EPL (~30 open)
- KXLALIGAGAME  # La Liga
- KXSERIEAGAME  # Serie A
- KXBUNDESLIGAGAME # Bundesliga
- KXUCLGAME     # Champions League
```

## Next Steps

1. Update secmaster sync to use series-based queries
2. Add series ticker configuration to connector CRD
3. Implement per-series subscription management
