# Security policy

## Supported versions

Security fixes target the latest released version and `main`.

## Reporting a vulnerability

Please report suspected vulnerabilities privately through GitHub Security Advisories:

https://github.com/aanari/muxboard/security/advisories/new

If that is unavailable, open a minimal issue asking for a private contact path without including exploit details.

## Scope

Muxboard is local-first. It talks to the local `tmux` server selected by your environment or CLI flags, reads local config/state, and may inspect local pane output to summarize status. It should not require network access to run.

Useful reports include command injection paths, unsafe file writes, accidental disclosure of pane content, terminal escape handling problems, and tmux plugin behavior that crosses the user's intended session or host boundary.
