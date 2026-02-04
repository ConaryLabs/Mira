# Recording the Demo

## Quick Start

```bash
# 1. Start recording
asciinema rec demo.cast --cols 100 --rows 30

# 2. Run the demo (auto mode simulates typing)
./scripts/demo.sh auto

# 3. Stop recording
# Press Ctrl+D or type 'exit'

# 4. Preview
asciinema play demo.cast

# 5. Upload to asciinema.org (optional)
asciinema upload demo.cast
```

## Manual Recording (more natural)

If you prefer to type commands yourself for a more natural feel:

```bash
asciinema rec demo.cast --cols 100 --rows 30
```

Then follow the commands in `demo.sh`, typing them yourself. This gives more natural pacing and lets you add commentary.

## Converting to GIF/SVG

For embedding in README without requiring asciinema.org:

```bash
# Install agg (asciinema gif generator)
cargo install agg

# Convert to GIF
agg demo.cast demo.gif --cols 100 --rows 30

# Or use svg-term for SVG
npm install -g svg-term-cli
svg-term --in demo.cast --out demo.svg --window
```

## Embedding in README

**Option 1: asciinema.org link**
```markdown
[![Demo](https://asciinema.org/a/<your-id>.svg)](https://asciinema.org/a/<your-id>)
```

**Option 2: GIF in repo**
```markdown
![Demo](demo.gif)
```

## Tips

- Keep terminal size consistent (100x30 works well)
- Clear any sensitive data from `~/.mira/mira.db` before recording
- The demo creates test data that you may want to clean up after
