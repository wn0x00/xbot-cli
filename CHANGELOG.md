# Changelog

## [0.7.5](https://github.com/wn0x00/xbot-cli/compare/v0.7.4...v0.7.5) (2026-10-23)

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
