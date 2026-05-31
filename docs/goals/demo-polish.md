Make muxboard's GitHub documentation, demo media, and launch presentation feel world-class: obvious in the first screen, safe to try, visually credible, and mechanically reproducible.

Follow AGENTS.md. Work as Product, Design, Engineering, QA, and Release in one long bounded loop. Continue sanding recursively until you have either a reviewable improvement with evidence or a concrete blocker. Improve one public surface at a time; do not sprawl into product scope.

Context:
- Muxboard is a tmux-first command center for AI agent fleets.
- V1 has no VCS, git, hg, branch, PR, or worktree awareness.
- The public promise is local-first tmux control: scan panes, see what needs you, inspect output, continue or reply, broadcast safely, and jump back to tmux.
- Public docs and media must never expose real pane data, private paths, secrets, chat transcripts, customer data, or the user's live tmux server.

Audit these surfaces before editing:
- README first screen, install path, trust copy, screenshots, and quick-start flow.
- docs/index.html metadata, links, hero copy, social image, and Pages assumptions.
- docs/demo.md, scripts/demo-session, just demo recipes, GIF, MP4, cast, and PNG export paths.
- docs/social-preview.svg, docs/social-preview.png, docs/muxboard-demo.svg, generated previews, and any future launch images.
- docs/tmux-plugin.md, docs/release.md, issue templates, PR template, security docs, and GitHub repo metadata preflight.
- Architecture guards around public surface, saved goals, scripts, release gates, and V1 scope.

Recursive loop:
1. Product: read the surface like a new visitor. Can they tell in seconds what muxboard is, why it exists, how to try it safely, and what it will not touch?
2. Design: inspect rendered screenshots or generated media, not just source. Is the hierarchy calm, legible, sparse, and credible at GitHub card, README, and landing-page sizes?
3. Engineering: make the demo and media path reproducible. Prefer scripts, just recipes, and guardrails over manual notes.
4. QA: press the advertised path. Run the synthetic demo where practical, export static assets, inspect images, and verify every key claim that can be checked locally.
5. Release: check GitHub metadata, canonical links, release docs, package metadata, and broken external routes. Do not point users at unverified Pages or unpublished assets.

Useful improvements:
- tighten README and landing-page copy so the first screen sells the product without happy talk,
- make safe demo setup mindless with start, attach, smoke, record, GIF, MP4, assets, and stop paths,
- improve screenshots/social previews until they render correctly through common tooling,
- add or strengthen guards so broken public links, stale product language, unsafe demo claims, or missing media tooling fail loudly,
- update saved instructions so future agents can repeat the same review loop without re-learning context.

Validation, progressively:
- `bash -n scripts/demo-session`
- `just demo-check`
- `just demo-assets`
- `just public-assets`
- `just demo-smoke` when tmux behavior or visible demo output changed
- `cargo fmt --check`
- targeted architecture guards for public surface, demo SVGs, release gates, and saved goals
- `just github-preflight` when GitHub metadata or public links changed
- `just ci` before closing if practical
- `git diff --check`

Stop conditions:
- Stop after a coherent reviewable diff with exact evidence.
- Stop if the repo is in a confusing or risky state.
- Do not commit, tag, publish, force-push, upload recordings, install broad tooling, change owner-level Pages or DNS settings, or add VCS awareness.
