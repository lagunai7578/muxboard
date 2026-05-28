# Theme audit

Muxboard should feel native in a terminal without becoming a theme engine. The current direction is strong: keep a small semantic theme layer, provide tasteful presets, make System Colors the answer for users who already theme WezTerm, Ghostty, iTerm, Alacritty, or tmux, and reject widget-level color configuration until there is a real product need.

## Sources inspected

- Ratatui ecosystem:
  - `ratatui-themes`: preset palettes with semantic colors for common states. <https://docs.rs/ratatui-themes/latest/ratatui_themes/>
  - `ratatui-themekit`: a richer semantic-slot crate with Terminal Native, No Color, widget builders, and NO_COLOR support. <https://docs.rs/ratatui-themekit/latest/ratatui_themekit/>
  - `tca-ratatui`: Base24 YAML themes for Ratatui. <https://docs.rs/tca-ratatui/latest/tca_ratatui/>
  - `tui-theme`: no current crate with that exact name appeared in `cargo search`.
  - `tui-theme-builder`: the closest current match, a derive-based Ratatui theme deserializer with sparse docs; useful as prior art, not a product dependency. <https://docs.rs/tui-theme-builder/0.2.2/tui_theme_builder/>
  - GitUI: active Ratatui app with partial RON theme overrides, terminal-color caveats, and external syntax themes. <https://raw.githubusercontent.com/gitui-org/gitui/master/THEMES.md>
  - crates-tui: Ratatui reference app that documents Base16 theme examples and config. <https://raw.githubusercontent.com/ratatui/crates-tui/main/README.md>
- Terminal applications:
  - Helix: theme names, palette references, light/dark config, custom theme files. <https://docs.helix-editor.com/master/themes.html>
  - Yazi: broad component-level theme surface and flavors. <https://yazi-rs.github.io/docs/configuration/theme/>
  - Yazi flavors: prebuilt themes plus user overrides layered on top. <https://yazi-rs.github.io/docs/flavors/overview>
  - Zellij: built-in themes, separate theme files, component roles. <https://zellij.dev/documentation/themes>
  - Zellij theme list: practical names such as `ansi`, `catppuccin-latte`, `gruvbox-dark`, `tokyo-night`. <https://zellij.dev/documentation/theme-list>
  - Lazygit: XDG config plus targeted color overrides rather than full palette engines. <https://lazygit.dev/docs/configuration/>
  - Starship: style strings are terminal-dependent and intentionally light. <https://starship.rs/config/>
  - Bat: previewable themes, `ansi`/`base16` terminal-aware themes, dark/light environment options, and `.tmTheme` support for custom syntax themes. <https://github.com/sharkdp/bat>
  - Delta: showable themes, dark/light filtering, named colors and RGB hex. <https://dandavison.github.io/delta/full---help-output.html>
  - Catppuccin ports: strong evidence that users expect theme names to work across many terminal apps. <https://catppuccin.com/ports/>
  - Tokyo Night ports: same cross-tool expectation for terminal palettes. <https://tokyonight.org/>
  - Gruvbox: light/dark modes, contrast options, and a focus on distinguishable colors that stay pleasant. <https://github.com/morhetz/gruvbox>
  - Nord: numbered colors designed to be usable as terminal color schemes and UI palettes. <https://www.nordtheme.com/docs/colors-and-palettes/>
  - Rosé Pine: role-based color names such as base, surface, text, subtle, love, gold, pine, foam, and iris. <https://rosepinetheme.com/palette/ingredients>
  - OpenCode: exposes this as the `system` theme, which adapts to the terminal color scheme, uses ANSI colors, preserves terminal defaults with `none`, and derives grays from the terminal background. <https://opencode.ai/docs/themes/>
  - Local WezTerm config: confirms that many users already curate a precise terminal ANSI palette, so System Colors should be first-class without reading private dotfiles.
- Standards:
  - NO_COLOR: environment convention for suppressing color in terminal programs. <https://no-color.org/>

## Comparison matrix

| Pattern | Strong OSS behavior | Muxboard decision |
| --- | --- | --- |
| Semantic tokens | `ratatui-themes`, `ratatui-themekit`, Helix, Zellij, and Rosé Pine organize colors by role instead of raw widget paint. | Keep 10 semantic slots and guard against raw `Color::` spread outside the theme boundary. |
| ANSI/native colors | GitUI, Bat, Starship, Yazi, OpenCode, and Ratatui colors all treat named ANSI colors as terminal-dependent. OpenCode exposes this as `system`; `ratatui-themekit` has Terminal Native. | Keep the stored `TerminalNative` value, label it System Colors, and accept aliases `system`, `terminal`, and `ansi`; do not inspect terminal config files. |
| Truecolor | Helix, Yazi, Delta, Bat, GitUI, and palette ports use `#RRGGBB`; Helix also accepts `#RGB`. | Accept `#RGB` and `#RRGGBB`, but keep truecolor behind semantic overrides and presets. |
| Overrides | GitUI and Yazi allow partial overrides; Starship favors small style strings. | Allow partial semantic overrides only. No widget-by-widget theme sprawl. |
| Theme names | Zellij, Bat, Ratatui crates, and palette ports use kebab-case names such as `tokyo-night`, `catppuccin-mocha`, and `gruvbox-dark`. | Accept PascalCase, kebab-case, snake_case, spaced names, and obvious aliases. |
| Fallbacks | Helix and Yazi support light/dark fallback, while Bat and Delta expose explicit dark/light controls. | Use explicit presets; avoid light/dark auto-detection because tmux and SSH make it unreliable. |
| NO_COLOR and dumb terminals | `ratatui-themekit` and no-color.org make NO_COLOR explicit; terminal apps often fall back to ANSI or plain styling. | Disable color when `NO_COLOR` or `TERM=dumb` applies and preserve shape cues with reverse, bold, underline, and ASCII borders. |
| Docs and tests | Bat and Delta offer preview commands; Zellij has theme testers; Ratatui apps rely on examples. | Use renderer/X-ray tests as the theme preview and keep README copy short. |

## Patterns worth copying

- Semantic slots beat widget-level color settings. Ratatui theme crates and terminal apps converge on roles such as text, muted text, border, selection, warning, error, and surface.
- Terminal-native themes matter. `terminal`, `ansi`, or equivalent presets are common because many users already tune their terminal palette.
- Theme names should be forgiving. Users try `tokyo-night`, `tokyo night`, `catppuccin`, `terminal`, and `no-color`; muxboard should accept the obvious spellings.
- Partial overrides are enough for V1. Lazygit and Starship show that most users only need to nudge a few colors.
- Previewability matters, but tests are muxboard's preview. Bat and Delta make theme choices visible with preview commands; muxboard's equivalent should be renderer tests and screenshots, not another in-app settings surface.
- Guardrails should inspect rendered cells. Theme correctness is visual, so X-ray renderer tests are more valuable than only parsing config.
- Accessibility needs shape cues. Mono, NO_COLOR, and dumb terminals must keep selection, alerts, and send-list rows distinguishable without relying on color.

## Patterns rejected for V1

- Full external theme files. Helix, Yazi, and Zellij need large theme surfaces because they style editors, file managers, plugin panes, and syntax previews. Muxboard has one focused dashboard and should not expose that complexity yet.
- Auto-reading terminal or dotfile configs. System Colors delegates palette control to the terminal without coupling muxboard to WezTerm, Ghostty, iTerm, Alacritty, or tmux file formats.
- Permanent runtime theme surface. The first-run picker is allowed because it removes setup friction; the main dashboard should not grow a standing settings pane.
- Pulling a dependency just for palette constants. `ratatui-themes` and `ratatui-themekit` validate muxboard's design, but muxboard currently needs fewer slots, fewer presets, and stricter product-specific renderer tests.
- Light/dark auto-detection. Helix supports terminal dark/light detection, but muxboard cannot depend on that support being present over SSH or inside tmux. Explicit presets are more predictable.
- Syntax-theme concepts. Bat, Delta, and Helix solve source-code highlighting. Muxboard is a control surface, so syntax theme files and Syntect integration would add weight without helping the fleet dashboard.

## Deferred until evidence

- More palette flavors. Catppuccin Frappe/Macchiato, Rosé Pine Moon/Dawn, Solarized, Dracula, Kanagawa, Everforest, and Base16 imports are plausible, but V1 should avoid a long picker-shaped list until users ask.
- Theme previews. If theme switching becomes common, a CLI preview or docs screenshot is better than adding a permanent picker to the main command surface.
- Light/dark auto mode. This can come back when muxboard has a reliable terminal capability story across tmux, SSH, and local terminals.
- User theme files. If overrides become too cramped, prefer one small semantic config file before adopting a full Helix/Yazi/Zellij-style theme format.

## Current muxboard design

- Stable slots: `text`, `muted`, `accent`, `success`, `warning`, `danger`, `surface`, `border`, `selected_fg`, `selected_bg`.
- 11 presets: `Calm`, `Contrast`, `Mono`, `TerminalNative`, `CatppuccinLatte`, `CatppuccinMocha`, `TokyoNight`, `GruvboxDark`, `GruvboxLight`, `Nord`, `RosePine`.
- Named truecolor presets are semantic mappings into muxboard slots, not full upstream theme ports.
- Friendly aliases: lowercase, kebab-case, snake_case, and spaced names work. Examples: `light`, `dark`, `system`, `system colors`, `terminal`, `ansi`, `no-color`, `catppuccin`, `tokyo night`, `gruvbox`, `rose-pine`, `rosé pine`.
- Color overrides accept ANSI names, friendly aliases like `purple`, 0-255 indexed colors, `#RGB`, and `#RRGGBB`.
- First run opens a small picker with System Colors highlighted; generated config also uses `TerminalNative` so dotfiles follow the terminal palette by default.
- Legacy `theme_preset` remains supported, while `ui_settings.theme.preset` takes precedence when present.

## Gaps closed by this audit

- Added forgiving preset aliases, including common ids from Ratatui theming crates and Zellij-style theme names.
- Added `#RGB` shorthand because Helix and CSS-style theme conventions make that expectation reasonable.
- Made the default `TerminalNative` after the local WezTerm audit exposed the risk of muxboard fighting a user's existing terminal palette.
- Locked exact truecolor palette tokens for named presets in `cargo test theme`.
- Added actionable bad-preset errors, matching the existing bad-color behavior.
- Added first-run and explicit `--theme-picker` onboarding without adding permanent dashboard chrome.
- Documented why muxboard stays intentionally small instead of adopting a generic theme package.

## Guardrails

- `cargo test theme` covers parsing, aliases, overrides, invalid colors, invalid presets, exact truecolor palette tokens, TerminalNative, Mono, NO_COLOR, dumb terminals, selection, alerts, targets, focused borders, and scrollbars.
- `cargo test onboarding` covers first-run theme onboarding, dotfile persistence, and the picker action contract.
- `cargo test --lib usability_theme` renders all presets and inspects exact cells for key visual states.
- `tests/architecture_guards.rs` prevents raw `Color::` usage from spreading outside the theme boundary.
- README and `config.example.json` keep the quiet user-facing config path discoverable without adding footer clutter.
