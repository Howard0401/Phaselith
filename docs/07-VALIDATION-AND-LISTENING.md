# 07. Validation and Listening

## Why Validation Matters

Phaselith can produce impressive first-impression effects.

That is both an advantage and a risk.

The project must therefore distinguish between:

1. first-wow reactions
2. stable listening improvement
3. bug-driven or level-driven false positives

## Technical Validation

Run and maintain:

1. unit tests
2. integration tests
3. engine wiring tests
4. runtime safety checks

Minimum command set:

```bash
cargo test -p asce-dsp-core -p asce-wasm-bridge
```

## Listening Validation Categories

Maintain at least four groups of reference material:

1. `obvious wins`
   - clips where Phaselith clearly helps
2. `subtle material`
   - material where improvement is smaller but still meaningful
3. `clean references`
   - already-good material where Phaselith should stay disciplined
4. `risk material`
   - clips likely to expose dryness, false width, over-focus, or loss of musicality

## Listening Questions

For every major change, ask:

1. Does the center image get stronger or weaker?
2. Does the source move forward naturally or become fake?
3. Is the result clearer because masking dropped, or only because tone shifted?
4. Does ambience still breathe?
5. Does the result stay enjoyable over longer listening?

## Anti-False-Positive Rules

1. Match output level before drawing conclusions.
2. Re-test after bug fixes.
3. Use hard-panned material to detect accidental mono behavior.
4. Use laptop speakers as well as headphones and better speakers.
5. Separate "more attractive" from "more truthful."

## Regression Rule

Any change that damages these core effects should be treated as a regression:

1. source appears in front of the listener
2. center image is locked and stable
3. haze is reduced without obvious glare
4. harshness does not return
5. the presentation still feels premium
