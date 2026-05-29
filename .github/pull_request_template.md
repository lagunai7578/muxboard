## What changed?

Describe the user-visible change or maintenance fix.

## Why?

Name the journey or risk this improves.

## Verification

- [ ] `cargo fmt --check`
- [ ] `cargo clippy --all-targets --all-features -- -D warnings`
- [ ] `just ci`
- [ ] `just ux`, if UI or copy changed
- [ ] `just test-live`, if tmux behavior changed

## Notes

Call out anything intentionally deferred or risky.
