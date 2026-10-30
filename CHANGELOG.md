# Changelog

## [0.9.1](https://github.com/wn0x00/xbot-cli/compare/v0.9.0...v0.9.1) (2026-10-30)

### Added

- Return ShadowBot Python alongside supported web action results and snapshots. Ordinary DOM actions use semantic XPath; iframe actions use ordered `Selector.xpath.segments` with `web_page.find`. Text output is headed `影刀 Python：`; JSON exposes `data.python`, including dependencies, locator evidence, warnings and validation status. Use `--no-python` to disable generation independently of `--no-snapshot`.
- Add `xbot-cli web xpath <ref...>`: return up to five single-element alternatives, or shared similar-element rules for multiple refs. Reject incompatible element types or contexts; retain match counts and review warnings for ambiguous candidates.
- Generate ordered iframe XPath lists and open-ShadowRoot proposals, labeled as ShadowBot syntax requiring validation in ShadowBot. Explicitly refuse XPath and equivalent Python when the target or an outer context crosses a closed ShadowRoot.
- Preserve the complete snapshot element tree with clearer branch layout, refs, names and states. Filtering and pagination are explicit rather than silently hiding nodes.

### Fixed

- Keep refs bound to the selected browser tab across CLI invocations, including short-lived npm launchers; changing the active tab no longer silently redirects ref actions.
- Discover non-default ShadowBot installations from the running client, registry and known directories. Report the actual backend path, discovery source and targeted repair advice instead of blindly using the default path or only suggesting startup/login.
- Preserve successful web actions when code generation or the follow-up snapshot fails; do not replay an action to recover generated output.

### Upgrade notes

- Reattach with `web active` or open a page before using fresh refs. Browser bindings now live in `%LOCALAPPDATA%\xbot-cli\web-sessions`; independent-preview bindings are not imported.
- Generated code and candidates are proposals, not completed ShadowBot runtime verification (`runtime_verified: false`). iframe lists are Selector segments, not a single XPath for `find_by_xpath`; open ShadowRoot expressions must be tested in ShadowBot. Closed ShadowRoot elements cannot be used.
- Page-keyboard and arbitrary-script operations do not yet emit equivalent Python. Some pure open-ShadowRoot operations use explicitly labeled JavaScript synthetic-event adapters rather than trusted native input.
- Explicit `XBOT_SHELL_CLI` / `XBOT_LAUNCHER` overrides remain authoritative; an invalid override is reported instead of silently falling back.
- The bundled Skill remains v10 with updated Web and diagnostic guidance. See the [Chinese release notes](docs/updates/2026-10-30-web-python-xpath.md) for examples and verification scope.

## [0.9.0](https://github.com/wn0x00/xbot-cli/compare/v0.8.3...v0.9.0) (2026-10-29)

- Replace browser-use-mcp with Chrome Bridge named-pipe transport for all Playwright-style web commands, including navigation, element interaction, snapshots, and read-only evaluation.
- Add CDP trusted input, ARIA refs, on-demand browser engine initialization, multi-browser pipe discovery, bounded reads, and `XBOT_WEB_PIPE` selection.
- Preserve action/snapshot recovery contracts and structured errors, and add isolated named-pipe contract tests alongside real Chrome and ShadowBot Browser verification.
- Update browser documentation and Windows dependencies; Studio MCP behavior is unchanged.

### Upgrade notes

- Browser automation now requires Chrome, Edge, or ShadowBot Browser with a compatible ShadowBot extension and named pipe. Set `XBOT_WEB_PIPE` to select a pipe explicitly; pipe prefixes are not browser product identities.
- The legacy `--name` option is accepted for compatibility but no longer stores selectors in the element library.
- Raw browser compatibility is limited to `web_create`, `web_get_active`, `web_snapshot`, `web_action`, and `web_evaluate_playwright` implemented by Chrome Bridge.

## [0.8.3](https://github.com/wn0x00/xbot-cli/compare/v0.8.2...v0.8.3) (2026-10-23)

- Reorder root help: business commands (app, flow, task, web, win32, tool, wiki, oapi) come first, followed by discovery, diagnostics and self-management (schema, guide, install, status, doctor, version, update, help).
- Simplify the `oapi` summary line to describe the Open API surface: auth, apps, robots, dispatch and logs.

## [0.8.2](https://github.com/wn0x00/xbot-cli/compare/v0.8.1...v0.8.2) (2026-10-23)

- Keep only the `xbot-cli` entry point; the `xbot` shim is no longer created. A stray older native program earlier in PATH (for example a manually copied `xbot-cli.exe`) used to shadow the npm shim and diverge command output between `xbot` and `xbot-cli`.
- Add a `preinstall` cleanup that removes legacy `xbot` shims belonging to this package and quarantines conflicting `xbot-cli` programs outside the npm bin directory to `.bak`, so the command always resolves to the current install. Set `XBOT_CLI_SKIP_CLEANUP=1` to opt out.
- Documentation and built-in examples now consistently use `xbot-cli`.

## [0.8.1](https://github.com/wn0x00/xbot-cli/compare/v0.7.6...v0.8.1) (2026-10-23)

- Add `xbot-cli update`: the npm launcher checks the registry for the latest release and runs the global npm upgrade before the native process starts, since Windows cannot replace a running executable. `xbot-cli update --check` only reports whether a newer version exists.
- The native `update` subcommand keeps the command discoverable in help and schema; invoked on the bare executable it returns `UPDATE_REQUIRES_LAUNCHER` pointing at the npm entry point. The update notifier now recommends `xbot-cli update`.
- Rename the enterprise entry point to `xbot-cli oapi` (`platform` remains a compatibility alias). One-line login `xbot-cli oapi auth <AccessKeyId> <AccessKeySecret>` verifies and stores credentials locally; the token is cached, reused until expiry and refreshed automatically with the stored keys. `oapi auth logout` clears the current authorization; `XBOT_OAPI_*` environment variables take precedence and legacy `XBOT_PLATFORM_*` names keep working.
- Wrap the full Open API with friendly commands: `robot list/groups/detail`, `app list/detail/versions/flows/transfer/records`, `job start/stop/retry/list/status/logs`, `task start/stop`, `user list/create/modify/delete/reset-password` and the complete `queue` group. All 44 registered endpoints remain reachable through `oapi api call`; mutating operations require `--yes` and support offline `--dry-run` previews.

## [0.7.6](https://github.com/wn0x00/xbot-cli/compare/v0.7.5...v0.7.6) (2026-10-23)

- Fix command routing so application runs and diagnostics continue to use the existing client CLI by default. Enterprise `platform` API operations require explicit selection and never become an automatic fallback.
- Update help, schema, README and the bundled Agent Skill to describe `platform` as an optional enterprise entry point; add regression coverage for default routing.
- Deprecate 0.7.5 because of an issue in the newly added Open Platform API execution logic. Upgrade to 0.7.6.

## [0.7.5](https://github.com/wn0x00/xbot-cli/compare/v0.7.4...v0.7.5) (2026-10-23)

> Deprecated: the newly added Open Platform API execution logic had an issue. Upgrade to 0.7.6.

- Add the `platform` command group for the official enterprise Open API, independent of the local ShadowBot client. Bundle 68 documentation references and 44 endpoint contracts covering authentication, accounts, applications, robots, dispatch, logs, queues, files and run records.
- Add schedule history, task results, job status and paginated log queries for investigating dispatch failures. Support synchronous log search and bounded notify-and-poll queries that retain the request ID on timeout.
- Support environment-based API keys or access tokens and private-cloud HTTPS origins. Redact credentials and password fields, encode query/path parameters, preserve server error codes and pagination, and require confirmation for mutating endpoints.
- Add offline API discovery and schemas, a reusable platform troubleshooting guide, seven-day validation for run-record queries and multipart file uploads.
- Verify authentication, dispatch filters, cursor precision, log polling, timeout recovery, redaction, upload encoding and installed package resources with local HTTP fixtures. Live enterprise API permissions and production logs remain unverified.

## [0.7.4](https://github.com/wn0x00/xbot-cli/compare/v0.7.3...v0.7.4) (2026-10-22)

- Add Skill v10 Web and Windows modules, with Agent routing based on the failed call and its actual target. Embed and install all 38 Skill resources and expose the corresponding offline guides.
- Add ref-to-XPath generation using the weighted DOM generator and a hash-pinned, externally supplied ROBULA+ source. Verify that generated paths identify the original node; support segmented iframe/open Shadow paths within the documented limits.
- Add workflow-usage analysis and read-only repair proposals for single elements. Add ordinary-DOM collection contracts, dependent child locator checks, dense-page candidate screening and compatible old/new locator variants.
- Add an experimental Python UIA single-control repair helper and `win32 snapshot --hwnd --pid`. Keep native observation separate from ShadowBot selectors; preserve original assets and reject ambiguity, incomplete observation and unproven identities.
- Document real test results and remaining limits, including same-content node replacement, Windows collection repair and uncertain WeChat send results. These helpers are not an unattended repair service.
- Extend offline helper regressions and package verification to cover the complete installed Skill, current embedded guides and the Windows snapshot discovery contract. ROBULA+ source, UIA dependencies and business runtime data are not bundled.

## [0.7.3](https://github.com/wn0x00/xbot-cli/compare/v0.7.2...v0.7.3) (2026-10-21)

- The npm launcher now reminds interactive users when a newer release is available, printing a one-line upgrade hint to stderr without touching stdout or JSON output.
- The registry check runs at most once every 24 hours with a local cache, reminds at most once per new version, times out silently after one second, and stays off in CI. Set `XBOT_NO_UPDATE_NOTIFIER=1` to disable it completely.

## [0.7.2](https://github.com/wn0x00/xbot-cli/compare/v0.7.1...v0.7.2) (2026-10-20)

- Ship Agent Skill v9 with exploration, development, acceptance and maintenance modes; add task contracts, locator records, business evidence, action risk guidance, bounded waits and a minimal repair/regression workflow.
- Prefer verified XPath in VisualFlow and CodeFlow. Document element-library naming, configuration/business/test flow organization, English snake_case code variables, and compatibility checks for old and new elements that coexist.
- Install all 14 Skill resources, including the new references and descriptive templates. Verify complete content, isolated local/global installs, repeat installation and conflict-before-write behavior; validate the same resources through the packaged Windows executable.
- Browser automation still uses the shared active-page context; this release does not add multi-session isolation or an automatic template executor.

## [0.7.1](https://github.com/wn0x00/xbot-cli/compare/v0.7.0...v0.7.1) (2026-10-19)

- Show the actual backend error when a browser action fails; selector-storage messages no longer hide fill failures. Keep the complete response and call log in JSON while showing a concise error and snapshot guidance in text.
- Add offline `xbot version` and `xbot-cli version` commands, with the same JSON result as `--version`.

## [0.7.0](https://github.com/wn0x00/xbot-cli/compare/v0.6.1...v0.7.0) (2026-10-19)

- Add the `xbot` command and a `web` interface with direct element actions, keyboard input and navigation backed by ShadowBot; retain `xbot-cli` and `browser` aliases.
- Show page snapshots after successful web actions, preserve action success when a snapshot fails, and allow saving snapshot text without overwriting files.
- Fix browser commands reporting `MCP_BAD_RESPONSE` after a successful action when the service returns a numeric JSON-RPC ID such as `1.0` instead of `1`. JSON and SSE responses accept equivalent numeric IDs while rejecting unrelated or string IDs.
- Publish from manually pushed version tags, with a helper to synchronize npm and Rust versions.

### Upgrade notes

- Install `@guanzhu.me/xbot-cli@0.7.0` to receive these changes; `0.6.1` does not contain the response-ID fix or the `xbot web` interface.
- Existing `xbot-cli browser` command names remain supported. Open and navigation URLs now accept HTTP(S), bare domains (normalized to HTTPS), and `about:blank`; other schemes are rejected locally.
- High-level open, attach, navigation and element/keyboard commands fetch a snapshot after success. Use `--no-snapshot` when only the action result is needed; raw tool calls keep their existing behavior.

## [0.6.1](https://github.com/wn0x00/xbot-cli/compare/v0.6.0...v0.6.1) (2026-10-19)


### Bug Fixes

* **release:** automate trusted npm and GitHub publishing ([bd12480](https://github.com/wn0x00/xbot-cli/commit/bd12480b7d1325ca8658c2cf42735a790a40b98c))
