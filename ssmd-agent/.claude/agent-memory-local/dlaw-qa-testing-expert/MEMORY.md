# QA Testing Expert Memory

## 2026-02-07: Multi-Exchange Agent Tools + DQ Review

### Codebase Test Patterns
- Tests use `Deno.test()` with `assertEquals` from std assert
- Test files mirror src layout: `test/server/routes.test.ts`, `test/cli/secmaster.test.ts`
- Route tests use `createTestRouter()` with mock db and make actual HTTP Request objects
- CLI tests are lightweight: type checks and "does not throw" assertions (no real DB)
- No existing DQ tests at all
- Test runner: `deno test --allow-read --allow-write --allow-net --allow-env test/`
- Agent tools have NO direct test coverage (tools.ts is untested)

### Key Observations
- Route tests only verify auth (401) and 404s - NO happy-path tests with real/mock DB
- `apiRequest()` helper in tools.ts has no mock/stub mechanism
- DQ module uses `Deno.Command` for kubectl/nats - hard to unit test
- `inferCategory()` and `parseWindow()` are pure functions - easily testable
- New pair/condition endpoints already exist in routes.ts (lines 245-301)
- Database functions (listPairs, getPair, listConditions, getCondition) use Drizzle ORM
- Migration 0012 adds pair_snapshots table and namespaces pair_ids with `exchange:` prefix

### Architecture Notes for Testing
- `createRouter()` returns a function `(req: Request) => Promise<Response>` - good for testing
- Auth middleware checks happen in router, handlers get db from RouteContext
- Tools.ts calls HTTP API via `apiRequest()` - tools need either HTTP mock or test server
- DQ compareTrades() is a pure function once given trade arrays - perfect for unit tests

### Effective Co-Panelists
- Database Expert: schema migrations, query correctness
- API Designer: endpoint contracts, error responses
- Data Quality Expert: NATS stream patterns, trade reconciliation logic
