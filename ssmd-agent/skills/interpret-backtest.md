---
name: interpret-backtest
description: How to analyze backtest results
---

# Interpreting Backtest Results

## Key Metrics

- **fires**: Number of times signal triggered
- **errors**: Runtime errors in signal code
- **fireTimes**: When signals fired (check clustering)
- **samplePayloads**: Example payloads (verify data looks right)

## Fire Count Guidelines

| Count | Interpretation | Action |
|-------|----------------|--------|
| 0 | Condition never met | Loosen threshold |
| 1-10 | Rare events | May be appropriate for alerts |
| 10-100 | Moderate frequency | Good for daily monitoring |
| 100-500 | Frequent | Consider if this is too noisy |
| 500+ | Very frequent | Likely needs tighter conditions |

## Common Issues

- **fires: 0, errors: 0**: Threshold too strict, or data doesn't have expected pattern
- **fires: 0, errors: [...]**: Bug in signal code, check error messages
- **Clustered fireTimes**: Signal fires rapidly then stops - may need cooldown
- **All same payload values**: Signal may not be updating state correctly
