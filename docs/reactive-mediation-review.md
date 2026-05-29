# Reactive Mediation — reviewer's guide

A map for reviewing the reactive-mediation PR. The feature is **default-off**
and **read-only**; this guide is structured so you can confirm "it can't hurt
anyone who hasn't opted in, and even opted-in it can't run side effects"
quickly.

## What it is (one line)

A default-off tool-output compaction layer for coding agents: when enabled,
eligible `rg` searches and large file reads return compact, line-numbered
evidence first, while exact-repeat bypass preserves raw tool access.

## Diff map

| Area | Paths | Nature |
| --- | --- | --- |
| **Pure library** (most of the diff) | `search-proxy/`, `large-read-proxy/` crates | self-contained (serde/shlex only); string→decision + slicing. Unit-tested in isolation. |
| **Hot-path integration** | `core/src/tools/handlers/shell.rs`, `…/unified_exec/exec_command.rs` | two `if let Some(out) = intercept_*` calls before normal exec |
| **New handlers** | `core/src/tools/handlers/{search_proxy,large_read_proxy}.rs` | the gate + escape-hatch + telemetry glue |
| **Session state** | `core/src/state/service.rs`, `session/session.rs` | per-session intercept registries + `ProxyTelemetry` |
| **Observability** | `core/src/tasks/mod.rs` (turn-end summary), `cli/src/doctor.rs` (status row) | stderr summary + `codex doctor` row |
| **Flags / schema / docs** | `features/src/lib.rs`, `core/config.schema.json`, `docs/reactive-mediation.md` | two default-off flags |

## Hot-path review (the only files that run for non-opted-in users)

`shell.rs` and `exec_command.rs` each gained exactly this shape, after the
existing `apply_patch` interception and before normal command execution:

```rust
if let Some(output) = intercept_search_proxy(&hook_command, cwd, session).await? {
    return Ok(output);
}
if let Some(output) = intercept_large_read_proxy(&hook_command, cwd, session).await? {
    return Ok(output);
}
```

Each `intercept_*` is `if !session.enabled(Feature::…) { return Ok(None); }`
as its **first statement** — so with the feature off, the only added cost is
two enum checks returning `Ok(None)`. Test: `search_proxy_is_inert_when_feature_disabled`,
`large_read_proxy_is_inert_when_feature_disabled`.

## Safety model

- **Read-only.** The proxies never mutate the workspace. Search Proxy runs
  its *own* bounded `rg`; Large Read Proxy reads the target file. Neither
  executes the model's command string.
- **Conservative by default.** Every path that isn't a clean, eligible,
  beneficial substitution returns `Ok(None)` and the model's original
  command runs verbatim. The failure-mode contract is enumerated in a doc
  comment at the top of each handler.
- **Why side-effect commands can't be intercepted.** Search Proxy only
  classifies a command as eligible when, after wrapper-stripping, it is a
  bare `rg` with recognized flags and **no unquoted shell metacharacter**
  (`; | & > < $( backtick`). `rm`, `git push`, redirects, pipes, and command
  substitution all fail this and pass through. The one compound allowance is
  a leading `pwd &&` (a no-op prefix); `cd <dir> &&` and any trailing chain
  still pass through. Even on a match, the proxy runs a *fresh read-only
  `rg`* from the parsed query — it does not run the model's string. Test:
  `side_effect_commands_are_never_intercepted`, plus the classifier
  pass-through suite.
- **Escape hatch.** Repeating the exact same command returns raw output
  (the substitution is recorded per-session; a repeat bypasses). Test:
  `search_proxy_repeat_command_bypasses_to_raw`,
  `first_read_substitutable_then_repeat_bypasses`.
- **Timeout.** The internal `rg` runs under a 5s wall-clock cap with a
  concurrent stdout drain (no pipe-deadlock) and kill-on-deadline; on
  timeout it passes through, so a pathological tree never blocks the turn.
  Test: `run_with_timeout_kills_a_slow_child`, `runner_timeout_passes_through`.
- **No panics / unwraps** in proxy code; binary and missing files pass
  through. Test: `large_read_proxy_passes_through_{binary,missing}_file`.

## Test matrix

| Concern | Test(s) | Crate |
| --- | --- | --- |
| feature OFF → no behavior change | `*_is_inert_when_feature_disabled` | core |
| pipes / redirects / `;` / `&&` / `$()` pass through | `*_is_shell_metacharacter`, `command_substitution_*` | search-proxy |
| side-effect cmds never intercepted | `side_effect_commands_are_never_intercepted` | search-proxy |
| `cd &&` passes through; only `pwd &&` stripped | `cd_prefix_still_passes_through`, `pwd_*` | search-proxy |
| unknown rg flags pass through | `unknown_{long,short}_flag_passes_through` | search-proxy |
| repeat-bypass | `search_proxy_repeat_command_bypasses_to_raw`, `first_read_substitutable_then_repeat_bypasses` | core / large-read-proxy |
| rg timeout → pass through | `run_with_timeout_kills_a_slow_child`, `runner_timeout_passes_through` | search-proxy |
| binary / missing file pass through | `large_read_proxy_passes_through_{binary,missing}_file` | core |
| owner ranking / confidence (no confident-wrong) | `confidence_*`, ranking benchmark | search-proxy |
| telemetry summary states | `proxy_telemetry_tests::*` | core |

Run: `cargo test -p codex-search-proxy -p codex-large-read-proxy -p codex-features`
and `cargo test -p codex-core -- proxy_telemetry reactive search_proxy large_read_proxy`.

## Known limitations

Default-off because validation is narrow (small fixed suite of search/read
tasks); cost was measured, not quality; LRP standalone is often inert (value
is in composition). See `docs/reactive-mediation.md` → "Known limitations".
