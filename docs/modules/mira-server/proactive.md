# proactive

Proactive intelligence engine for anticipating developer needs through pattern recognition. Detects behavioral patterns and generates interventions before the user asks.

## Key Types

- `EventType` - Tracked developer events
- `PatternType` - Recognized behavioral patterns
- `InterventionType` - Types of proactive suggestions
- `UserResponse` - Feedback with effectiveness multipliers
- `ProactiveConfig` - User/project preferences for proactive behavior

## Sub-modules

| Module | Purpose |
|--------|---------|
| `behavior` | Behavioral event tracking |
| `patterns` | Pattern recognition and matching |
| `predictor` | Prediction engine |
| `interventions` | Intervention generation and delivery |
| `feedback` | User feedback processing and learning |

## Key Export

`get_proactive_config()` - Loads user/project preferences for proactive analysis.
