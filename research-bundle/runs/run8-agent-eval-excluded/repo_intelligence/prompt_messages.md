# run8-agent-eval-excluded / repo_intelligence

## [0] role=developer

```
<permissions instructions>
Filesystem sandboxing defines which files can be read or written. `sandbox_mode` is `danger-full-access`: No filesystem sandboxing - all commands are permitted. Network access is enabled.
Approval policy is currently never. Do not provide the `sandbox_permissions` for any reason, commands will be rejected.
</permissions instructions>
<apps_instructions>
## Apps (Connectors)
Apps (Connectors) can be explicitly triggered in user messages in the format `[$app-name](app://{connector_id})`. Apps can also be implicitly triggered as long as the context suggests usage of available apps.
An app is equivalent to a set of MCP tools within the `codex_apps` MCP.
An installed app's MCP tools are either provided to you already, or can be lazy-loaded through the `tool_search` tool. If `tool_search` is available, the apps that are searchable by `tools_search` will be listed by it.
Do not additionally call list_mcp_resources or list_mcp_resource_templates for apps.
</apps_instructions>
<skills_instructions>
## Skills
A skill is a set of local instructions to follow that is stored in a `SKILL.md` file. Below is the list of skills that can be used. Each entry includes a name, description, and file path so you can open the source for full instructions when using a specific skill.
### Available skills
- imagegen: Generate or edit raster images when the task benefits from AI-created bitmap visuals such as photos, illustrations, textures, sprites, mockups, or transparent-background cutouts. Use when Codex should create a brand-new image, transform an existing image, or derive visual variants from references, and the output should be a bitmap asset rather than repo-native code or vector. Do not use when the task is better handled by editing existing SVG/vector/code-native assets, extending an established icon or logo system, or building the visual directly in HTML/CSS/canvas. (file: /Users/mendell/.codex/skills/.system/imagegen/SKILL.md)
- openai-docs: Use when the user asks how to build with OpenAI products or APIs and needs up-to-date official documentation with citations, help choosing the latest model for a use case, or model upgrade and prompt-upgrade guidance; prioritize OpenAI docs MCP tools, use bundled references only as helper context, and restrict any fallback browsing to official OpenAI domains. (file: /Users/mendell/.codex/skills/.system/openai-docs/SKILL.md)
- plugin-creator: Create and scaffold plugin directories for Codex with a required `.codex-plugin/plugin.json`, optional plugin folders/files, valid manifest defaults, and personal-marketplace entries by default. Use when Codex needs to create a new personal plugin, add optional plugin structure, generate or update marketplace entries for plugin ordering and availability metadata, or update an existing local plugin during development with the CLI-driven cachebuster and reinstall flow. (file: /Users/mendell/.codex/skills/.system/plugin-creator/SKILL.md)
- skill-creator: Guide for creating effective skills. This skill should be used when users want to create a new skill (or update an existing skill) that extends Codex's capabilities with specialized knowledge, workflows, or tool integrations. (file: /Users/mendell/.codex/skills/.system/skill-creator/SKILL.md)
- skill-installer: Install Codex skills into $CODEX_HOME/skills from a curated list or a GitHub repo path. Use when a user asks to list installable skills, install a curated skill, or install a skill from another repo (including private repos). (file: /Users/mendell/.codex/skills/.system/skill-installer/SKILL.md)
- babysit-pr: Babysit a GitHub pull request after creation by continuously polling review comments, CI checks/workflow runs, and mergeability state until the PR is merged/closed or user help is required. Diagnose failures, retry likely flaky failures up to 3 times, auto-fix/push branch-related issues when appropriate, and keep watching open PRs so fresh review feedback is surfaced promptly. Use when the user asks Codex to monitor a PR, watch CI, handle review comments, or keep an eye on failures and feedback on an open PR. (file: /private/var/folders/bn/jq675y6x5m1d1x83vyh99f5c0000gn/T/codex-arm-XXXXXX.EFHBunAWhs/.codex/skills/babysit-pr/SKILL.md)
- code-breaking-changes: Breaking changes (file: /private/var/folders/bn/jq675y6x5m1d1x83vyh99f5c0000gn/T/codex-arm-XXXXXX.EFHBunAWhs/.codex/skills/code-review-breaking-changes/SKILL.md)
- code-review: Run a final code review on a pull request (file: /private/var/folders/bn/jq675y6x5m1d1x83vyh99f5c0000gn/T/codex-arm-XXXXXX.EFHBunAWhs/.codex/skills/code-review/SKILL.md)
- code-review-change-size: Change size guidance (800 lines) (file: /private/var/folders/bn/jq675y6x5m1d1x83vyh99f5c0000gn/T/codex-arm-XXXXXX.EFHBunAWhs/.codex/skills/code-review-change-size/SKILL.md)
- code-review-context: Model visible context (file: /private/var/folders/bn/jq675y6x5m1d1x83vyh99f5c0000gn/T/codex-arm-XXXXXX.EFHBunAWhs/.codex/skills/code-review-context/SKILL.md)
- code-review-testing: Test authoring guidance (file: /private/var/folders/bn/jq675y6x5m1d1x83vyh99f5c0000gn/T/codex-arm-XXXXXX.EFHBunAWhs/.codex/skills/code-review-testing/SKILL.md)
- codex-bug: Diagnose GitHub bug reports in openai/codex. Use when given a GitHub issue URL from openai/codex and asked to decide next steps such as verifying against the repo, requesting more info, or explaining why it is not a bug; follow any additional user-provided instructions. (file: /private/var/folders/bn/jq675y6x5m1d1x83vyh99f5c0000gn/T/codex-arm-XXXXXX.EFHBunAWhs/.codex/skills/codex-bug/SKILL.md)
- codex-issue-digest: Run a GitHub issue digest for openai/codex by feature-area labels, all areas, and configurable time windows. Use when asked to summarize recent Codex bug reports or enhancement requests, especially for owner-specific labels such as tui, exec, app, or similar areas. (file: /private/var/folders/bn/jq675y6x5m1d1x83vyh99f5c0000gn/T/codex-arm-XXXXXX.EFHBunAWhs/.codex/skills/codex-issue-digest/SKILL.md)
- codex-pr-body: Update the title and body of one or more pull requests. (file: /private/var/folders/bn/jq675y6x5m1d1x83vyh99f5c0000gn/T/codex-arm-XXXXXX.EFHBunAWhs/.codex/skills/codex-pr-body/SKILL.md)
- remote-tests: How to run tests using remote executor. (file: /private/var/folders/bn/jq675y6x5m1d1x83vyh99f5c0000gn/T/codex-arm-XXXXXX.EFHBunAWhs/.codex/skills/remote-tests/SKILL.md)
- test-tui: Guide for testing Codex TUI interactively (file: /private/var/folders/bn/jq675y6x5m1d1x83vyh99f5c0000gn/T/codex-arm-XXXXXX.EFHBunAWhs/.codex/skills/test-tui/SKILL.md)
- update-v8-version: Update Codex's pinned `v8` / `rusty_v8` versions, validate the release-candidate path, and investigate failed V8 canary or artifact builds. Use when asked to bump V8, update `rusty_v8` artifacts, prepare or validate a V8 release candidate, check `v8-canary`, or diagnose why a V8 version update no longer builds. (file: /private/var/folders/bn/jq675y6x5m1d1x83vyh99f5c0000gn/T/codex-arm-XXXXXX.EFHBunAWhs/.codex/skills/update-v8-version/SKILL.md)
- atlas: macOS-only AppleScript control for the ChatGPT Atlas desktop app. Use only when the user explicitly asks to control Atlas tabs/bookmarks/history on macOS and the "ChatGPT Atlas" app is installed; do not trigger for general browser tasks or non-macOS environments. (file: /Users/mendell/.codex/skills/atlas/SKILL.md)
- browser:browser: Browser automation for the Codex in-app browser. Use to open, navigate, inspect, test, click, type, screenshot, or verify local targets such as localhost, 127.0.0.1, ::1, file://, the current in-app browser tab, and websites shown side by side inside Codex. (file: /Users/mendell/.codex/plugins/cache/openai-bundled/browser/26.519.41501/skills/browser/SKILL.md)
- canva:canva-branded-presentation: Create on-brand Canva presentations from a brief, outline, existing Canva doc, or design link. Use when the user wants a branded slide deck, wants to turn notes into a presentation, or needs a presentation generated in Canva with the right brand kit and a clear slide plan. (file: /Users/mendell/.codex/plugins/cache/openai-curated/canva/6188456f/skills/canva-branded-presentation/SKILL.md)
- canva:canva-resize-for-all-social-media: Resize a Canva design into standard social media formats and prepare export-ready results. Use when the user wants one Canva design adapted across multiple social platforms such as Facebook, Instagram, and LinkedIn, especially when they want all variants produced in one pass. (file: /Users/mendell/.codex/plugins/cache/openai-curated/canva/6188456f/skills/canva-resize-for-all-social-media/SKILL.md)
- canva:canva-translate-design: Translate the text in a Canva design into another language while preserving the original layout as much as possible. Use when the user wants a localized or translated version of an existing Canva design and expects the original file to remain unchanged. (file: /Users/mendell/.codex/plugins/cache/openai-curated/canva/6188456f/skills/canva-translate-design/SKILL.md)
- documents:documents: Create, edit, redline, and comment on `.docx`, Word, and Google Docs-targeted document artifacts inside the container, with a strict render-and-verify workflow. Use `render_docx.py` to generate page PNGs (and optional PDF) for visual QA, then iterate until layout is flawless before delivering the final document. (file: /Users/mendell/.codex/plugins/cache/openai-primary-runtime/documents/26.521.10419/skills/documents/SKILL.md)
- file-organizer: Intelligently analyze and organize local files and folders, detect duplicates, propose clear structures, and perform cleanup with explicit confirmation. Use when asked to organize Downloads/Documents/Desktop/Projects, find duplicate files, archive or restructure folders, set up organization habits, or reduce clutter across a filesystem. (file: /Users/mendell/.codex/skills/file-organizer/SKILL.md)
- github:gh-address-comments: Address actionable GitHub pull request review feedback. Use when the user wants to inspect unresolved review threads, requested changes, or inline review comments on a PR, then implement selected fixes. Use the GitHub app for PR metadata and flat comment reads, and use the bundled GraphQL script via `gh` whenever thread-level state, resolution status, or inline review context matters. (file: /Users/mendell/.codex/plugins/cache/openai-curated/github/6188456f/skills/gh-address-comments/SKILL.md)
- github:gh-fix-ci: Use when a user asks to debug or fix failing GitHub PR checks that run in GitHub Actions. Use the GitHub app from this plugin for PR metadata and patch context, and use `gh` for Actions check and log inspection before implementing any approved fix. (file: /Users/mendell/.codex/plugins/cache/openai-curated/github/6188456f/skills/gh-fix-ci/SKILL.md)
- github:github: Triage and orient GitHub repository, pull request, and issue work through the connected GitHub app. Use when the user asks for general GitHub help, wants PR or issue summaries, or needs repository context before choosing a more specific GitHub workflow. (file: /Users/mendell/.codex/plugins/cache/openai-curated/github/6188456f/skills/github/SKILL.md)
- github:yeet: Publish local changes to GitHub by confirming scope, committing intentionally, pushing the branch, and opening a draft PR through the GitHub app from this plugin, with `gh` used only as a fallback where connector coverage is insufficient. (file: /Users/mendell/.codex/plugins/cache/openai-curated/github/6188456f/skills/yeet/SKILL.md)
- hatch-pet: Create, repair, validate, preview, and package Codex-compatible animated pets and pet spritesheets from character art, screenshots, generated images, or visual references. Use when a user wants to hatch a Codex pet, create a custom animated pet, or build a built-in pet asset with an 8x9 atlas, transparent unused cells, row-by-row animation prompts, QA contact sheets, preview videos, and pet.json packaging. This skill composes the installed $imagegen system skill for visual generation and uses bundled scripts for deterministic spritesheet assembly. (file: /Users/mendell/.codex/skills/hatch-pet/SKILL.md)
- imagegen: Use when the user asks to generate or edit images via the OpenAI Image API (for example: generate image, edit/inpaint/mask, background removal or replacement, transparent background, product shots, concept art, covers, or batch variants); run the bundled CLI (`scripts/image_gen.py`) and require `OPENAI_API_KEY` for live calls. (file: /Users/mendell/.codex/skills/imagegen/SKILL.md)
- openai-docs: Use when the user asks how to build with OpenAI products or APIs and needs up-to-date official documentation with citations (for example: Codex, Responses API, Chat Completions, Apps SDK, Agents SDK, Realtime, model capabilities or limits); prioritize OpenAI docs MCP tools and restrict any fallback browsing to official OpenAI domains. (file: /Users/mendell/.codex/skills/openai-docs/SKILL.md)
- presentations:Presentations: Build PowerPoint PPTX decks with artifact-tool presentation JSX (file: /Users/mendell/.codex/plugins/cache/openai-primary-runtime/presentations/26.521.10419/skills/presentations/SKILL.md)
- security-best-practices: Perform language and framework specific security best-practice reviews and suggest improvements. Trigger only when the user explicitly requests security best practices guidance, a security review/report, or secure-by-default coding help. Trigger only for supported languages (python, javascript/typescript, go). Do not trigger for general code review, debugging, or non-security tasks. (file: /Users/mendell/.codex/skills/security-best-practices/SKILL.md)
- security-ownership-map: Analyze git repositories to build a security ownership topology (people-to-file), compute bus factor and sensitive-code ownership, and export CSV/JSON for graph databases and visualization. Trigger only when the user explicitly wants a security-oriented ownership or bus-factor analysis grounded in git history (for example: orphaned sensitive code, security maintainers, CODEOWNERS reality checks for risk, sensitive hotspots, or ownership clusters). Do not trigger for general maintainer lists or non-security ownership questions. (file: /Users/mendell/.codex/skills/security-ownership-map/SKILL.md)
- security-threat-model: Repository-grounded threat modeling that enumerates trust boundaries, assets, attacker capabilities, abuse paths, and mitigations, and writes a concise Markdown threat model. Trigger only when the user explicitly asks to threat model a codebase or path, enumerate threats/abuse paths, or perform AppSec threat modeling. Do not trigger for general architecture summaries, code review, or non-security design work. (file: /Users/mendell/.codex/skills/security-threat-model/SKILL.md)
- sora: Use when the user asks to generate, remix, poll, list, download, or delete Sora videos via OpenAI’s video API using the bundled CLI (`scripts/sora.py`), including requests like “generate AI video,” “Sora,” “video remix,” “download video/thumbnail/spritesheet,” and batch video generation; requires `OPENAI_API_KEY` and Sora API access. (file: /Users/mendell/.codex/skills/sora/SKILL.md)
- spreadsheets:Spreadsheets: Use this skill when a user requests to create, modify, analyze, visualize, or work with spreadsheet files (`.xlsx`, `.xls`, `.csv`, `.tsv`) or Google Sheets-targeted spreadsheet artifacts with formulas, formatting, charts, tables, and recalculation. (file: /Users/mendell/.codex/plugins/cache/openai-primary-runtime/spreadsheets/26.521.10419/skills/spreadsheets/SKILL.md)
- stripe:stripe-best-practices: Guides Stripe integration decisions — API selection (Checkout Sessions vs PaymentIntents), Connect platform setup (Accounts v2, controller properties), billing/subscriptions, Treasury financial accounts, integration surfaces (Checkout, Payment Element), and migrating from deprecated Stripe APIs. Use when building, modifying, or reviewing any Stripe integration — including accepting payments, building marketplaces, integrating Stripe, processing payments, setting up subscriptions, or creating connected accounts. (file: /Users/mendell/.codex/plugins/cache/openai-curated/stripe/6188456f/skills/stripe-best-practices/SKILL.md)
- stripe:upgrade-stripe: Guide for upgrading Stripe API versions and SDKs (file: /Users/mendell/.codex/plugins/cache/openai-curated/stripe/6188456f/skills/upgrade-stripe/SKILL.md)
### How to use skills
- Discovery: The list above is the skills available in this session (name + description + file path). Skill bodies live on disk at the listed paths.
- Trigger rules: If the user names a skill (with `$SkillName` or plain text) OR the task clearly matches a skill's description shown above, you must use that skill for that turn. Multiple mentions mean use them all. Do not carry skills across turns unless re-mentioned.
- Missing/blocked: If a named skill isn't in the list or the path can't be read, say so briefly and continue with the best fallback.
- How to use a skill (progressive disclosure):
  1) After deciding to use a skill, open its `SKILL.md`. Read only enough to follow the workflow.
  2) When `SKILL.md` references relative paths (e.g., `scripts/foo.py`), resolve them relative to the skill directory listed above first, and only consider other paths if needed.
  3) If `SKILL.md` points to extra folders such as `references/`, load only the specific files needed for the request; don't bulk-load everything.
  4) If `scripts/` exist, prefer running or patching them instead of retyping large code blocks.
  5) If `assets/` or templates exist, reuse them instead of recreating from scratch.
- Coordination and sequencing:
  - If multiple skills apply, choose the minimal set that covers the request and state the order you'll use them.
  - Announce which skill(s) you're using and why (one short line). If you skip an obvious skill, say why.
- Context hygiene:
  - Keep context small: summarize long sections instead of pasting them; only load extra files when needed.
  - Avoid deep reference-chasing: prefer opening only files directly linked from `SKILL.md` unless you're blocked.
  - When variants exist (frameworks, providers, domains), pick only the relevant reference file(s) and note that choice.
- Safety and fallback: If a skill can't be applied cleanly (missing files, unclear instructions), state the issue, pick the next-best approach, and continue.
</skills_instructions>
<plugins_instructions>
## Plugins
A plugin is a local bundle of skills, MCP servers, and apps. Below is the list of plugins that are enabled and available in this session.
### Available plugins
- `Browser`: Browser / browser-use plugin Aliases: @browser, @browser-use, browser-use, Browser, in-app browser. Use Browser, the Codex in-app browser, when the user asks to open, inspect, navigate, test, click, type, or screenshot local web targets such as localhost, 127.0.0.1, ::1, file:// URLs, or the current in-app browser tab. After significant frontend changes to a local app, use Browser to open the relevant local target when it is known or obvious, unless the user asks for another browser tool. For requests like "open localhost:3000" or "open to localhost:4000", navigate the in-app browser to http://localhost:3000 or http://localhost:4000. Do not satisfy explicit @browser or @browser-use requests with macOS `open`, shell commands, or generic web browsing unless the user asks for another browser tool or approves a fallback.
- `Canva`: Search, create, edit designs
- `Documents`: Create and edit document artifacts in Codex, including Word files and Google Docs.
- `GitHub`: Inspect repositories, triage pull requests and issues, debug CI, and publish changes through a hybrid GitHub connector and CLI workflow.
- `Presentations`: Create, edit, render, verify, and export presentation slide decks. Use when Codex needs to build or modify a deck, slidedeck, presentation deck, slide deck, slides, PowerPoint, Google Slides, PPT, PPTX, .ppt, or .pptx file.
- `Spreadsheets`: Create, edit, analyze, visualize, render, and export spreadsheets or Google Sheets-ready workbooks in Codex.
- `Stripe`: Payments and business tools
### How to use plugins
- Discovery: The list above is the plugins available in this session.
- Skill naming: If a plugin contributes skills, those skill entries are prefixed with `plugin_name:` in the Skills list.
- Trigger rules: If the user explicitly names a plugin, prefer capabilities associated with that plugin for that turn.
- Relationship to capabilities: Plugins are not invoked directly. Use their underlying skills, MCP tools, and app tools to help solve the task.
- Preference: When a relevant plugin is available, prefer using capabilities associated with that plugin over standalone capabilities that provide similar functionality.
- Missing/blocked: If the user requests a plugin that is not listed above, or the plugin does not have relevant callable capabilities for the task, say so briefly and continue with the best fallback.
</plugins_instructions>
## Memory

You have access to a memory folder with guidance from prior runs. It can save
time and help you stay consistent. Use it whenever it is likely to help.

Decision boundary: should you use memory for a new user query?

- Skip memory ONLY when the request is clearly self-contained and does not need
  workspace history, conventions, or prior decisions.
- Hard skip examples: current time/date, simple translation, simple sentence
  rewrite, one-line shell command, trivial formatting.
- Use memory by default when ANY of these are true:
  - the query mentions workspace/repo/module/path/files in MEMORY_SUMMARY below,
  - the user asks for prior context / consistency / previous decisions,
  - the task is ambiguous and could depend on earlier project choices,
  - the ask is a non-trivial and related to MEMORY_SUMMARY below.
- If unsure, do a quick memory pass.

Memory layout (general -> specific):

- /Users/mendell/.codex/memories/memory_summary.md (already provided below; do NOT open again)
- /Users/mendell/.codex/memories/MEMORY.md (searchable registry; primary file to query)
- /Users/mendell/.codex/memories/skills/<skill-name>/ (skill folder)
  - SKILL.md (entrypoint instructions)
  - scripts/ (optional helper scripts)
  - examples/ (optional example outputs)
  - templates/ (optional templates)
- /Users/mendell/.codex/memories/rollout_summaries/ (per-rollout recaps + evidence snippets)
  - The paths of these entries can be found in /Users/mendell/.codex/memories/MEMORY.md or /Users/mendell/.codex/memories/rollout_summaries/ as `rollout_path`
  - These files are append-only `jsonl`: `session_meta.payload.id` identifies the session, `turn_context` marks turn boundaries, `event_msg` is the lightweight status stream, and `response_item` contains actual messages, tool calls, and tool outputs.
  - For efficient lookup, prefer matching the filename suffix or `session_meta.payload.id`; avoid broad full-content scans unless needed.

Quick memory pass (when applicable):

1. Skim the MEMORY_SUMMARY below and extract task-relevant keywords.
2. Search /Users/mendell/.codex/memories/MEMORY.md using those keywords.
3. Only if MEMORY.md directly points to rollout summaries/skills, open the 1-2
   most relevant files under /Users/mendell/.codex/memories/rollout_summaries/ or
   /Users/mendell/.codex/memories/skills/.
4. If above are not clear and you need exact commands, error text, or precise evidence, search over `rollout_path` for more evidence.
5. If there are no relevant hits, stop memory lookup and continue normally.

Quick-pass budget:

- Keep memory lookup lightweight: ideally <= 4-6 search steps before main work.
- Avoid broad scans of all rollout summaries.

During execution: if you hit repeated errors, confusing behavior, or suspect
relevant prior context, redo the quick memory pass.

How to decide whether to verify memory:

- Consider both risk of drift and verification effort.
- If a fact is likely to drift and is cheap to verify, verify it before
  answering.
- If a fact is likely to drift but verification is expensive, slow, or
  disruptive, it is acceptable to answer from memory in an interactive turn,
  but you should say that it is memory-derived, note that it may be stale, and
  consider offering to refresh it live.
- If a fact is lower-drift and expensive to verify, it is usually fine to
  answer from memory directly.

When answering from memory without current verification:

- If you rely on memory for a fact that you did not verify in the current turn,
  say so briefly in the final answer.
- If that fact is plausibly drift-prone or comes from an older note, older
  snapshot, or prior run summary, say that it may be stale or outdated.
- If live verification was skipped and a refresh would be useful in the
  interactive context, consider offering to verify or refresh it live.
- Do not present unverified memory-derived facts as confirmed-current.
- Prefer a short refresh offer for interactive questions, especially about prior
  results, commands, timing, or older snapshots.

Memory citation requirements:

- If ANY relevant memory files were used: append exactly one
`<oai-mem-citation>` block as the VERY LAST content of the final reply.
  Normal responses should include the answer first, then append the
`<oai-mem-citation>` block at the end.
- Use this exact structure for programmatic parsing:
```
<oai-mem-citation>
<citation_entries>
MEMORY.md:234-236|note=[responsesapi citation extraction code pointer]
rollout_summaries/2026-02-17T21-23-02-LN3m-example.md:10-12|note=[weekly report format]
</citation_entries>
<rollout_ids>
019c6e27-e55b-73d1-87d8-4e01f1f75043
019c7714-3b77-74d1-9866-e1f484aae2ab
</rollout_ids>
</oai-mem-citation>
```
- `citation_entries` is for rendering:
  - one citation entry per line
  - format: `<file>:<line_start>-<line_end>|note=[<how memory was used>]`
  - use file paths relative to the memory base path (for example, `MEMORY.md`,
    `rollout_summaries/...`, `skills/...`)
  - only cite files actually used under the memory base path (do not cite
    workspace files as memory citations)
  - if you used `MEMORY.md` and then a rollout summary/skill file, cite both
  - list entries in order of importance (most important first)
  - `note` should be short, single-line, and use simple characters only (avoid
    unusual symbols, no newlines)
- `rollout_ids` is for us to track what previous rollouts you find useful:
  - include one rollout id per line
  - rollout ids should look like UUIDs (for example,
    `019c6e27-e55b-73d1-87d8-4e01f1f75043`)
  - include unique ids only; do not repeat ids
  - an empty `<rollout_ids>` section is allowed if no rollout ids are available
  - you can find rollout ids in rollout summary files and MEMORY.md
  - do not include file paths or notes in this section
  - For every `citation_entries`, try to find and cite the corresponding rollout id if possible
- Never include memory citations inside pull-request messages.
- Never cite blank lines; double-check ranges.

Updating memories:

You can update the memories **only** when explicitly asked by the user. This must always come from a direct request from the user.
- Write your update in /Users/mendell/.codex/memories/extensions/ad_hoc/notes/
- Each update must be one small file containing what you want to add/delete/update from the memories.
- The name of this file must be `<timestamp>-<short slug>.md`
- Do not try to edit the memory files yourself, only add one update note in /Users/mendell/.codex/memories/extensions/ad_hoc/notes/

========= MEMORY_SUMMARY BEGINS =========
v1

## User Profile

The user actively works across `/Users/mendell/Jewgo-master`, `/Users/mendell/Jewgo Backend V1`, `/Users/mendell/Menucha FI`, and `/Users/mendell/flutter-ui-kit`. They use Codex for repo-grounded implementation, audits, PRD-to-code handoffs, and end-to-end fixes rather than abstract brainstorming. They expect the agent to inspect the real checkout, preserve unrelated worktree changes, and finish the operational follow-through instead of stopping at diagnosis.

Their recurring steering is consistent: keep product behavior aligned to the actual brief, keep visible UX simpler, keep security/backend enforcement real, and verify claims with the right runtime/build/test gate. They care about exact file paths, API names, generated-client surfaces, and concrete blocker strings because those are the fastest handles for follow-up work. In Jewgo planning they repeatedly push a utility-first product direction over generic social framing; in code work they prefer evidence-backed findings, targeted fixes, and explicit notes when a failure is just baseline repo noise.

## User preferences

- Inspect the real repo state first and preserve unrelated dirty worktree changes; do not misattribute or revert them.
- When the user asks for an audit, produce evidence-backed findings from the actual repo state and validation gates, not a lightweight summary.
- When a task is framed as end-to-end, carry it through backend, client orchestration, UI flow, and the real verification stack for that feature.
- For Shtetl work in `Jewgo-master`, treat "Audit the shtetl section end to end" as cross-layer work across backend, admin, Flutter, docs, and tests, not a single-layer patch.
- For admin/security work, keep backend enforcement real: default-deny unknown access, do not rely on UI-only gating, and keep audit logging when roles/permissions mutate.
- Reduce visible choices aggressively when the user asks to simplify UI/UX; prefer progressive disclosure over exposing every control.
- In Jewgo product planning, keep the framing utility-first. Preserve phrases like `Open Now + Nearby Food`, avoid generic social/app-platform language, and keep legacy social/messaging surfaces internal unless the user asks to remove them.
- When backend contracts change in Jewgo repos, include OpenAPI/generated-client regeneration instead of leaving handwritten API calls.
- When the user gives a broad product brief without a clear artifact, ask a short structured clarification on output shape, first slice, and legacy handling before committing to a long plan.

## General Tips

- Route through [MEMORY.md](/Users/mendell/.codex/memories/MEMORY.md) first; it is organized by task family and repo scope.
- `Jewgo Backend V1` and `Jewgo-master` are different checkouts. Do not reuse repo facts across them without checking the current paths and scripts.
- For Jewgo OpenAPI/client work, the saved reusable workflow is [skills/jewgo-openapi-client-sync/SKILL.md](/Users/mendell/.codex/memories/skills/jewgo-openapi-client-sync/SKILL.md).
- For Jewgo MLS Phase 0.7 runtime proof, use [skills/jewgo-mls-bridge-phase07-verification/SKILL.md](/Users/mendell/.codex/memories/skills/jewgo-mls-bridge-phase07-verification/SKILL.md).
- In repo audits, `git diff --dirstat` / `--numstat` are the fastest first pass; in noisy repos, treat broad analyzer/file-size output as triage unless the touched files are explicitly implicated.
- For `Jewgo-master` Shtetl audits, start with `backend/src/modules/shtetl/`, `frontend/lib/screens/shtetls/`, `admin-dashboard/src/routes/shtetls*.tsx`, `docs/features/shtetl-capability-flags.md`, and `backend/test/modules/shtetl/shtetl.test.ts` instead of a repo-wide search.
- For route-heavy Flutter/admin work, verify redirects/router generation explicitly; for Docker/backend platform work, the user expects actual boot evidence, not compile-only proof.

## What's in Memory

### /Users/mendell/Jewgo-master

#### 2026-05-21

- Specials free publishing and client sync: specials, publish-free, MySpecials_publishFree, servicePublish.ts, scripts/api/regenerate-api-client.sh
  - desc: Search first for the free/unlimited deals change that removed slot-required publishing across backend, Flutter, and generated client workflow in `cwd=/Users/mendell/Jewgo-master`.
  - learnings: Free publish now bypasses slots through `servicePublish.ts` and `publishValidation.ts`; the closeout risk is assuming the long OpenAPI/client regeneration finished without final sync proof.
- Jewgo 2.0 Home Wedge phased plan: Jewgo 2.0, Home Wedge, Open Now, Nearby Food, Keep Internal
  - desc: Repo-grounded planning memory for utility-first Jewgo 2.0 roadmaps, especially Home/search/open-now sequencing and how to treat legacy social/messaging surfaces in `cwd=/Users/mendell/Jewgo-master`.
  - learnings: Reuse the existing `home_screen.dart`, `search_hub_body.dart`, and `/v1/home` path; if the brief is broad, ask for output shape, first slice, and legacy handling early.

###…251 tokens truncated…utter analyze output was mostly vendored/baseline noise, not a clean audit gate.

### /Users/mendell/Menucha FI

#### 2026-05-18

- PRD spine redesign: docs/prd.md, app_routes.dart, shell_screen.dart, router_smoke_test.dart
  - desc: End-to-end Menucha app-flow redesign around the PRD spine, with route redirects, shell cleanup, and final green route/analyzer/engine verification in `cwd=/Users/mendell/Menucha FI`.
  - learnings: Treat IA/shell mismatch as the first bug, not the visuals; route smoke tests are the highest-value gate after navigation changes.

### /Users/mendell/Jewgo Backend V1

#### 2026-05-18

- Messaging end-to-end hardening: messaging, ws-ticket, group chats, entity conversations, targetMessageId
  - desc: Search here first for backend + Flutter messaging audits and fixes in `cwd=/Users/mendell/Jewgo Backend V1`, including malformed base64, direct-thread bootstrap, report validation, WS reliability, and group/entity conversation support.
  - learnings: Start with the full backend + Flutter messaging map, use the batch messaging tests, and treat harness races or stale schema as likely causes before redesigning product logic.

### Older Memory Topics

#### /Users/mendell/Jewgo Backend V1

- Launch capsule audit and cleanup: launch capsules, LaunchCapsuleContext, duplicate guard, /launch-capsules/$stateCode
  - desc: End-to-end launch capsule backend/flutter/admin fixes, create-flow propagation, duplicate-submit gating, admin discoverability, and responsive queue work for `cwd=/Users/mendell/Jewgo Backend V1`.
- MLS bridge Phase 0.7 verification: messaging_crypto_spike_test.dart, JewgoMlsBindings.init(), cargo ndk, PackageFrameworks
  - desc: Real iOS simulator and Android emulator proof for the Flutter/Rust MLS bridge plus post-verification cleanup; use the saved skill first.
- Backend audit, lint cleanup, and service boundaries: backend/src/modules, AppError, request context, file-size policy
  - desc: Repo-grounded backend cleanup and architectural boundary fixes, including lint/build triage and large-file boundary guidance.
- Shtetl parity and service-area workflow: shtetl, remote mode, postal code, service cards, vitest
  - desc: Shtetl PRD parity, submit gating, remote/local service-area normalization, and test-DB harness expectations.
- Admin navigation simplification: sidebar, top bar, mobile responsive, fewer options
  - desc: Admin-dashboard IA simplification and shell cleanup for mobile/desktop parity.
- Stories system wiring: stories, canonical objects, server-driven permissions, admin moderation
  - desc: Flutter stories surfaces plus admin moderation wiring built on the existing backend module and generated clients.
- Community trust and contract sync: community-trust-driven, image_posts, /v1/me/community-trust, flutter analyze
  - desc: Trust/progression backend implementation plus generated-client regeneration and enum-drift cleanup.
- Activity tab and seen-state reliability: Nearby, Following, For You, context labels, seen_activity_provider.dart
  - desc: Utility-first activity/discovery UX, PRD handoff details, context-label rules, and SharedPreferences race fixes.
- User-post composer and owner actions: Post as myself, soft-delete only, user-post reports
  - desc: Unified post composer, entity tagging rules, and owner edit/delete/report flows.
- OpenAPI export and Flutter client regeneration: openapi:export, regenerate-api-client.sh, .spec-hash
  - desc: General `Jewgo Backend V1` contract export/codegen workflow and how to interpret noisy export/regeneration output.
- Admin policy and roles workflow: admin policy, default-deny, role-management-page.tsx, replaceUserRoles
  - desc: Backend-enforced admin authorization, capability-filtered UI, roles-page simplification, name/email lookup, and in-page role assignment.
- Backend platform and Docker boot: docker/entrypoint.sh, _drizzle_migrations, /ready, shadow mode first
  - desc: Additive platform-layer implementation and fresh-DB Docker boot debugging with actual runtime proof.
- Amazon deals and AMZCreater diagnostics: /v1/admin/amazon-deals, diagnosticsVersion: 2, discoveryFallbacks
  - desc: Admin approval/import fixes, commission disclosure, and upstream AMZCreater discovery diagnostics.
- Gallery reorder reuse: reorderGallerySortOrders.ts, assertExactReorderCoverage, VITEST_SKIP_DB_SETUP=1
  - desc: Concrete reuse map and consolidation for backend image reorder logic.
- Backend↔Flutter compatibility audits: break the flutter clients, backend/openapi.baseline.json, HechshersMainstreamListResponseDataInner
  - desc: Deploy-risk audits that compare backend branch contracts against Flutter Main before saying a backend deploy is safe.

#### /Users/mendell/flutter-ui-kit

- Mac runtime portability and crash audit: macOS, FlutterGallery, asset bundling, startup crash, portability
  - desc: Runtime/startup/packaging audit for the Flutter UI kit on other Macs, focused on real portability/crash risk rather than generic cleanup.
========= MEMORY_SUMMARY ENDS =========

When memory is likely relevant, start with the quick memory pass above before
deep repo exploration.
```

## [1] role=user

```
Harness repo intelligence:
Use this as task-routing guidance before editing.

Task: Add a unit test inside the `#[cfg(test)] mod tests` block of the agent_eval module that asserts `classify_result` returns an `AgentEvalResult::Excluded` variant whenever `valid_for_comparison=false`, regardless of the score arguments. Na...

Likely edit targets:
1. core/README.md — Likely repo area match

Orientation only:
1. core/config.schema.json — Likely repo area match
2. core/gpt-5.1-codex-max_prompt.md — Likely repo area match
3. core/gpt-5.2-codex_prompt.md — Likely repo area match
4. core/gpt_5_1_prompt.md — Likely repo area match

Do not broaden the edit scope just because a file appears in orientation context. Prefer the smallest patch that satisfies the task.

Likely area: core
# AGENTS.md instructions for /private/var/folders/bn/jq675y6x5m1d1x83vyh99f5c0000gn/T/codex-arm-XXXXXX.EFHBunAWhs/codex-rs

<INSTRUCTIONS>
# Backend System Design & Build Rules

**Status:** Canonical
**Applies to:** All backend code, present and future
**Goal:** Predictable structure, minimal refactors, clean modularity, scalable deployment

---

## 1. Core Principles (Non-Negotiable)

### P1 — Separation of concerns is mandatory

Each layer has a single responsibility. Mixing layers is a bug.

* Routes handle HTTP only
* Services handle business logic only
* Repos handle database access only
* Plugins handle cross-cutting concerns only
* Common utilities are shared, not owned by modules

If a file violates this, it must be split.

---

### P2 — One change should affect one layer

Changing:

* a DB column **must not** require editing routes
* an API field **must not** require editing SQL
* auth logic **must not** require touching domain code

If a change cascades across layers, the architecture failed.

---

### P3 — Explicit over clever

No magic.
No reflection.
No implicit behavior.

All data flow should be traceable by reading the code top-down.

---

### P4 — Reuse before creating

Always check for existing files or implementations before creating new code.

* Search the codebase for similar patterns, utilities, or functions
* Reuse existing code, patterns, and abstractions whenever possible
* Only create new code when no suitable existing implementation exists
* Prefer extending existing code over duplicating functionality

---

## 2. Runtime & Deployment Rules

### R1 — Docker is the execution environment

All environments run the **same Docker image**:

* local
* staging
* production
* Kubernetes

No "special local setup" logic is allowed in code.

---

### R2 — Environment parity

The only difference between environments is:

* environment variables
* infrastructure (scale, secrets)

Code is identical.

---

### R3 — Migrations are part of deployment

Every deploy must:

1. run migrations
2. then start the API

Manual DB changes are forbidden.

---

## 3. Repository Structure (Canonical)

```
src/
  server.ts          # process entrypoint
  app.ts             # Fastify app builder

  config/
    env.ts           # ONLY place that reads process.env
    constants.ts

  common/
    errors/
    logging/
    http/
    validation/
    pagination/

  plugins/
    requestContext.ts
    errorHandler.ts
    db.ts
    openApi.ts
    auth.ts          # later
    orgContext.ts    # later
    rateLimit.ts    # later

  db/
    client.ts
    schema/
    migrations/
    seed/

  modules/
    <moduleName>/
      routes.ts
      schemas.ts
      service.ts
      repo.ts
      mapper.ts
      types.ts
```

### Naming rules

* Directories: `camelCase`
* Files: `camelCase.ts`
* No `utils.ts`
* No random top-level folders

---

## 4. Layer Responsibilities (Strict)

### Routes (`routes.ts`)

* Register endpoints
* Attach schemas
* Call service functions
* Return responses

❌ No SQL
❌ No auth logic
❌ No business rules

---

### Services (`service.ts`)

* Business logic
* Orchestration of repos
* Permission checks (via helpers)

❌ No Fastify types
❌ No HTTP objects
❌ No direct env access

---

### Repos (`repo.ts`)

* Database queries only
* Drizzle + SQL
* Return raw domain data

❌ No auth
❌ No request context
❌ No response shaping

---

### Schemas (`schemas.ts`)

* Zod schemas only
* Input validation
* Output validation (when needed)

❌ No logic
❌ No DB references

---

### Types (`types.ts`)

* TypeScript types/interfaces only
* No runtime code

---

### Mapper (`mapper.ts`)

* DB → API transformation
* Field renames
* Relation nesting

This file exists specifically to **prevent refactor explosions**.

---

## 5. Configuration Rules

### C1 — Single env reader

Only `config/env.ts` may access `process.env`.

Every other file imports from `env`.

---

### C2 — Fail fast

Invalid or missing env variables **crash the process on startup**.

Silent defaults are forbidden for required config.

---

## 6. Error Handling Rules

### E1 — One error shape

All API errors return:

```json
{
  "error": {
    "code": "ERROR_CODE",
    "message": "Human readable",
    "details": {},
    "requestId": "uuid"
  }
}
```

No exceptions.

---

### E2 — One error class

`AppError` is the only allowed custom error.

Throwing raw `Error` is allowed **only** for truly unexpected failures and must be wrapped at the boundary.

---

### E3 — No leaking internals

Stack traces, SQL errors, and internal messages never reach the client.

---

## 7. Request Context Rules

### RC1 — Every request has context

Attached once, early:

```ts
request.ctx = {
  requestId,
  userId?,
  orgId?,
  permissions?
}
```

---

### RC2 — Context is read-only after creation

No route or service mutates context arbitrarily.

---

## 8. Authentication & Authorization Rules (Forward-Compatible)

### A1 — Auth is enforced in plugins, not routes

Routes assume auth is already resolved.

---

### A2 — Users do not have roles

Users have **memberships**.
Memberships reference **roles**.
Roles reference **permissions**.

Authorization checks are always permission-based.

---

### A3 — No permissions in JWTs

Permissions are resolved server-side to allow instant revocation.

---

## 9. OpenAPI & Contracts

### O1 — OpenAPI is authoritative for clients

* Clients depend on OpenAPI
* Changes must be intentional

---

### O2 — Code-first, generated

OpenAPI is generated from route schemas.
No hand-edited spec files.

---

## 10. File Size & Modularity Rules

* **Soft limit:** 250 lines per file
* **Hard limit:** 650 lines per file
* **Exception threshold:** Files exceeding 650 lines require explicit justification comment at top of file + ticket/issue reference
* **Auto-split threshold:** Files exceeding 900 lines must be split before next feature work in that module
* **CI enforcement:** CI will fail if any non-exempt `.ts` file exceeds 650 LOC (exempt: seed scripts folder + explicitly annotated exceptions)

If a file grows:

* split by responsibility
* never split by "helpers"

**Exceptions:**

* Seed scripts may exceed 650 lines (data is verbose by nature)
* Query catalogs may exceed 650 lines *temporarily*, but must split by feature area at 900

**Exception header format** (standardized, grep-able):

Files exceeding 650 lines must include this header at the top:

```ts
/**
 * FILE_SIZE_EXCEPTION: >650 LOC justified because <reason>
 * TRACKING: JIRA-123 (or GH-issue-123)
 */
```

---

## 11. What Is Explicitly Out of Scope (For Now)

These are **forbidden** until real need appears:

* GraphQL
* Prisma
* NestJS
* Event buses
* Background workers
* Message queues
* Generic repository abstractions
* Multi-service architectures

---

## 12. Development Workflow Rules

* Build backend **vertically**, one entity at a time
* No "platform work" without an entity using it
* No abstractions without duplication pressure

---

## 13. Definition of "Done" for Any Phase

A phase is done only when:

* it runs in Docker
* it boots with correct env validation
* health checks pass
* no TODOs block forward progress

---

## Final Rule (The Most Important One)

> **Shipping correct, boring code beats elegant unfinished systems.**

--- project-doc ---

# Rust/codex-rs

In the codex-rs folder where the rust code lives:

- Crate names are prefixed with `codex-`. For example, the `core` folder's crate is named `codex-core`
- When using format! and you can inline variables into {}, always do that.
- Install any commands the repo relies on (for example `just`, `rg`, or `cargo-insta`) if they aren't already available before running instructions here.
- Never add or modify any code related to `CODEX_SANDBOX_NETWORK_DISABLED_ENV_VAR` or `CODEX_SANDBOX_ENV_VAR`.
  - You operate in a sandbox where `CODEX_SANDBOX_NETWORK_DISABLED=1` will be set whenever you use the `shell` tool. Any existing code that uses `CODEX_SANDBOX_NETWORK_DISABLED_ENV_VAR` was authored with this fact in mind. It is often used to early exit out of tests that the author knew you would not be able to run given your sandbox limitations.
  - Similarly, when you spawn a process using Seatbelt (`/usr/bin/sandbox-exec`), `CODEX_SANDBOX=seatbelt` will be set on the child process. Integration tests that want to run Seatbelt themselves cannot be run under Seatbelt, so checks for `CODEX_SANDBOX=seatbelt` are also often used to early exit out of tests, as appropriate.
- Always collapse if statements per https://rust-lang.github.io/rust-clippy/master/index.html#collapsible_if
- Always inline format! args when possible per https://rust-lang.github.io/rust-clippy/master/index.html#uninlined_format_args
- Use method references over closures when possible per https://rust-lang.github.io/rust-clippy/master/index.html#redundant_closure_for_method_calls
- Avoid bool or ambiguous `Option` parameters that force callers to write hard-to-read code such as `foo(false)` or `bar(None)`. Prefer enums, named methods, newtypes, or other idiomatic Rust API shapes when they keep the callsite self-documenting.
- When you cannot make that API change and still need a small positional-literal callsite in Rust, follow the `argument_comment_lint` convention:
  - Use an exact `/*param_name*/` comment before opaque literal arguments such as `None`, booleans, and numeric literals when passing them by position.
  - Do not add these comments for string or char literals unless the comment adds real clarity; those literals are intentionally exempt from the lint.
  - The parameter name in the comment must exactly match the callee signature.
  - You can run `just argument-comment-lint` to run the lint check locally. This is powered by Bazel, so running it the first time can be slow if Bazel is not warmed up, though incremental invocations should take <15s. Most of the time, it is best to update the PR and let CI take responsibility for checking this (or run it asynchronously in the background after submitting the PR). Note CI checks all three platforms, which the local run does not.
- When possible, make `match` statements exhaustive and avoid wildcard arms.
- Newly added traits should include doc comments that explain their role and how implementations are expected to use them.
- Discourage both `#[async_trait]` and `#[allow(async_fn_in_trait)]` in Rust traits.
  - Prefer native RPITIT trait methods with explicit `Send` bounds on the returned future, as in `3c7f013f9735` / `#16630`.
  - Preferred trait shape:
    `fn foo(&self, ...) -> impl std::future::Future<Output = T> + Send;`
  - Implementations may still use `async fn foo(&self, ...) -> T` when they satisfy that contract.
  - Do not use `#[allow(async_fn_in_trait)]` as a shortcut around spelling the future contract explicitly.
- When writing tests, prefer comparing the equality of entire objects over fields one by one.
- Do not add general product or user-facing documentation to the `docs/` folder. The official Codex documentation lives elsewhere. The exception is app-server API documentation, which is covered by the app-server guidance below.
- Prefer private modules and explicitly exported public crate API.
- If you change `ConfigToml` or nested config types, run `just write-config-schema` to update `codex-rs/core/config.schema.json`.
- When working with MCP tool calls, prefer using `codex-rs/codex-mcp/src/mcp_connection_manager.rs` to handle mutation of tools and tool calls. Aim to minimize the footprint of changes and leverage existing abstractions rather than plumbing code through multiple levels of function calls.
- Do not call `reset_client_session` unnecessarily; let the incremental check logic decide whether to reuse the previous request.
- If you change Rust dependencies (`Cargo.toml` or `Cargo.lock`), run `just bazel-lock-update` from the
  repo root to refresh `MODULE.bazel.lock`, and include that lockfile update in the same change.
- After dependency changes, run `just bazel-lock-check` from the repo root so lockfile drift is caught
  locally before CI.
- Bazel does not automatically make source-tree files available to compile-time Rust file access. If
  you add `include_str!`, `include_bytes!`, `sqlx::migrate!`, or similar build-time file or
  directory reads, update the crate's `BUILD.bazel` (`compile_data`, `build_script_data`, or test
  data) or Bazel may fail even when Cargo passes.
- Do not create small helper methods that are referenced only once.
- Avoid large modules:
  - Prefer adding new modules instead of growing existing ones.
  - Target Rust modules under 500 LoC, excluding tests.
  - If a file exceeds roughly 800 LoC, add new functionality in a new module instead of extending
    the existing file unless there is a strong documented reason not to.
  - This rule applies especially to high-touch files that already attract unrelated changes, such
    as `codex-rs/tui/src/app.rs`, `codex-rs/tui/src/bottom_pane/chat_composer.rs`,
    `codex-rs/tui/src/bottom_pane/footer.rs`, `codex-rs/tui/src/chatwidget.rs`,
    `codex-rs/tui/src/bottom_pane/mod.rs`, and similarly central orchestration modules.
  - When extracting code from a large module, move the related tests and module/type docs toward
    the new implementation so the invariants stay close to the code that owns them.
  - Avoid adding new standalone methods to `codex-rs/tui/src/chatwidget.rs` unless the change is
    trivial; prefer new modules/files and keep `chatwidget.rs` focused on orchestration.
- When running Rust commands (e.g. `just fix` or `just test`) be patient with the command and never try to kill them using the PID. Rust lock can make the execution slow, this is expected.

Run `just fmt` (in `codex-rs` directory) automatically after you have finished making Rust code changes; do not ask for approval to run it. Additionally, run the tests:

1. Do not run `cargo test` directly. Use `just test` so test execution follows the repo defaults.
2. Run the test for the specific project that was changed. For example, if changes were made in `codex-rs/tui`, run `just test -p codex-tui`.
3. Once those pass, if any changes were made in common, core, or protocol, run the complete test suite with `just test`. Avoid `--all-features` for routine local runs because it expands the build matrix and can significantly increase `target/` disk usage; use it only when you specifically need full feature coverage. project-specific or individual tests can be run without asking the user, but do ask the user before running the complete test suite.

Before finalizing a large change to `codex-rs`, run `just fix -p <project>` (in `codex-rs` directory) to fix any linter issues in the code. Prefer scoping with `-p` to avoid slow workspace‑wide Clippy builds; only run `just fix` without `-p` if you changed shared crates. Do not re-run tests after running `fix` or `fmt`.

## The `codex-core` crate

Over time, the `codex-core` crate (defined in `codex-rs/core/`) has become bloated because it is the largest crate, so it is often easier to add something new to `codex-core` rather than refactor out the library code you need so your new code neither takes a dependency on, nor contributes to the size of, `codex-core`.

To that end: **resist adding code to codex-core**!

Particularly when introducing a new concept/feature/API, before adding to `codex-core`, consider whether:

- There is an existing crate other than `codex-core` that is an appropriate place for your new code to live.
- It is time to introduce a new crate to the Cargo workspace for your new functionality. Refactor existing code as necessary to make this happen.

Likewise, when reviewing code, do not hesitate to push back on PRs that would unnecessarily add code to `codex-core`.

## TUI style conventions

See `codex-rs/tui/styles.md`.

## TUI code conventions

- Use concise styling helpers from ratatui’s Stylize trait.
  - Basic spans: use "text".into()
  - Styled spans: use "text".red(), "text".green(), "text".magenta(), "text".dim(), etc.
  - Prefer these over constructing styles with `Span::styled` and `Style` directly.
  - Example: patch summary file lines
    - Desired: vec!["  └ ".into(), "M".red(), " ".dim(), "tui/src/app.rs".dim()]

### TUI Styling (ratatui)

- Prefer Stylize helpers: use "text".dim(), .bold(), .cyan(), .italic(), .underlined() instead of manual Style where possible.
- Prefer simple conversions: use "text".into() for spans and vec![…].into() for lines; when inference is ambiguous (e.g., Paragraph::new/Cell::from), use Line::from(spans) or Span::from(text).
- Computed styles: if the Style is computed at runtime, using `Span::styled` is OK (`Span::from(text).set_style(style)` is also acceptable).
- Avoid hardcoded white: do not use `.white()`; prefer the default foreground (no color).
- Chaining: combine helpers by chaining for readability (e.g., url.cyan().underlined()).
- Single items: prefer "text".into(); use Line::from(text) or Span::from(text) only when the target type isn’t obvious from context, or when using .into() would require extra type annotations.
- Building lines: use vec![…].into() to construct a Line when the target type is obvious and no extra type annotations are needed; otherwise use Line::from(vec![…]).
- Avoid churn: don’t refactor between equivalent forms (Span::styled ↔ set_style, Line::from ↔ .into()) without a clear readability or functional gain; follow file‑local conventions and do not introduce type annotations solely to satisfy .into().
- Compactness: prefer the form that stays on one line after rustfmt; if only one of Line::from(vec![…]) or vec![…].into() avoids wrapping, choose that. If both wrap, pick the one with fewer wrapped lines.

### Text wrapping

- Always use textwrap::wrap to wrap plain strings.
- If you have a ratatui Line and you want to wrap it, use the helpers in tui/src/wrapping.rs, e.g. word_wrap_lines / word_wrap_line.
- If you need to indent wrapped lines, use the initial_indent / subsequent_indent options from RtOptions if you can, rather than writing custom logic.
- If you have a list of lines and you need to prefix them all with some prefix (optionally different on the first vs subsequent lines), use the `prefix_lines` helper from line_utils.

## Tests

### Snapshot tests

This repo uses snapshot tests (via `insta`), especially in `codex-rs/tui`, to validate rendered output.

**Requirement:** any change that affects user-visible UI (including adding new UI) must include
corresponding `insta` snapshot coverage (add a new snapshot test if one doesn't exist yet, or
update the existing snapshot). Review and accept snapshot updates as part of the PR so UI impact
is easy to review and future diffs stay visual.

When UI or text output changes intentionally, update the snapshots as follows:

- Run tests to generate any updated snapshots:
  - `just test -p codex-tui`
- Check what’s pending:
  - `cargo insta pending-snapshots -p codex-tui`
- Review changes by reading the generated `*.snap.new` files directly in the repo, or preview a specific file:
  - `cargo insta show -p codex-tui path/to/file.snap.new`
- Only if you intend to accept all new snapshots in this crate, run:
  - `cargo insta accept -p codex-tui`

If you don’t have the tool:

- `cargo install --locked cargo-insta`

### Test assertions

- Tests should use pretty_assertions::assert_eq for clearer diffs. Import this at the top of the test module if it isn't already.
- Prefer deep equals comparisons whenever possible. Perform `assert_eq!()` on entire objects, rather than individual fields.
- Avoid mutating process environment in tests; prefer passing environment-derived flags or dependencies from above.

### Spawning workspace binaries in tests (Cargo vs Bazel)

- Prefer `codex_utils_cargo_bin::cargo_bin("...")` over `assert_cmd::Command::cargo_bin(...)` or `escargot` when tests need to spawn first-party binaries.
  - Under Bazel, binaries and resources may live under runfiles; use `codex_utils_cargo_bin::cargo_bin` to resolve absolute paths that remain stable after `chdir`.
- When locating fixture files or test resources under Bazel, avoid `env!("CARGO_MANIFEST_DIR")`. Prefer `codex_utils_cargo_bin::find_resource!` so paths resolve correctly under both Cargo and Bazel runfiles.

### Integration tests (core)

- Prefer the utilities in `core_test_support::responses` when writing end-to-end Codex tests.

- All `mount_sse*` helpers return a `ResponseMock`; hold onto it so you can assert against outbound `/responses` POST bodies.
- Use `ResponseMock::single_request()` when a test should only issue one POST, or `ResponseMock::requests()` to inspect every captured `ResponsesRequest`.
- `ResponsesRequest` exposes helpers (`body_json`, `input`, `function_call_output`, `custom_tool_call_output`, `call_output`, `header`, `path`, `query_param`) so assertions can target structured payloads instead of manual JSON digging.
- Build SSE payloads with the provided `ev_*` constructors and the `sse(...)`.
- Prefer `wait_for_event` over `wait_for_event_with_timeout`.
- Prefer `mount_sse_once` over `mount_sse_once_match` or `mount_sse_sequence`

- Typical pattern:

  ```rust
  let mock = responses::mount_sse_once(&server, responses::sse(vec![
      responses::ev_response_created("resp-1"),
      responses::ev_function_call(call_id, "shell", &serde_json::to_string(&args)?),
      responses::ev_completed("resp-1"),
  ])).await;

  codex.submit(Op::UserTurn { ... }).await?;

  // Assert request body if needed.
  let request = mock.single_request();
  // assert using request.function_call_output(call_id) or request.json_body() or other helpers.
  ```

## App-server API Development Best Practices

These guidelines apply to app-server protocol work in `codex-rs`, especially:

- `app-server-protocol/src/protocol/common.rs`
- `app-server-protocol/src/protocol/v2.rs`
- `app-server/README.md`

### Core Rules

- All active API development should happen in app-server v2. Do not add new API surface area to v1.
- Follow payload naming consistently:
  `*Params` for request payloads, `*Response` for responses, and `*Notification` for notifications.
- Expose RPC methods as `<resource>/<method>` and keep `<resource>` singular (for example, `thread/read`, `app/list`).
- Always expose fields as camelCase on the wire with `#[serde(rename_all = "camelCase")]` unless a tagged union or explicit compatibility requirement needs a targeted rename.
- Exception: config RPC payloads are expected to use snake_case to mirror config.toml keys (see the config read/write/list APIs in `app-server-protocol/src/protocol/v2.rs`).
- Always set `#[ts(export_to = "v2/")]` on v2 request/response/notification types so generated TypeScript lands in the correct namespace.
- Never use `#[serde(skip_serializing_if = "Option::is_none")]` for v2 API payload fields.
  Exception: client->server requests that intentionally have no params may use:
  `params: #[ts(type = "undefined")] #[serde(skip_serializing_if = "Option::is_none")] Option<()>`.
- Keep Rust and TS wire renames aligned. If a field or variant uses `#[serde(rename = "...")]`, add matching `#[ts(rename = "...")]`.
- For discriminated unions, use explicit tagging in both serializers:
  `#[serde(tag = "type", ...)]` and `#[ts(tag = "type", ...)]`.
- Prefer plain `String` IDs at the API boundary (do UUID parsing/conversion internally if needed).
- Timestamps should be integer Unix seconds (`i64`) and named `*_at` (for example, `created_at`, `updated_at`, `resets_at`).
- For experimental API surface area:
  use `#[experimental("method/or/field")]`, derive `ExperimentalApi` when field-level gating is needed, and use `inspect_params: true` in `common.rs` when only some fields of a method are experimental.

### Client->server request payloads (`*Params`)

- Every optional field must be annotated with `#[ts(optional = nullable)]`. Do not use `#[ts(optional = nullable)]` outside client->server request payloads (`*Params`).
- Optional collection fields (for example `Vec`, `HashMap`) must use `Option<...>` + `#[ts(optional = nullable)]`. Do not use `#[serde(default)]` to model optional collections, and do not use `skip_serializing_if` on v2 payload fields.
- When you want omission to mean `false` for boolean fields, use `#[serde(default, skip_serializing_if = "std::ops::Not::not")] pub field: bool` over `Option<bool>`.
- For new list methods, implement cursor pagination by default:
  request fields `pub cursor: Option<String>` and `pub limit: Option<u32>`,
  response fields `pub data: Vec<...>` and `pub next_cursor: Option<String>`.

### Development Workflow

- Update app-server docs/examples when API behavior changes (at minimum `app-server/README.md`).
- Regenerate schema fixtures when API shapes change:
  `just write-app-server-schema`
  (and `just write-app-server-schema --experimental` when experimental API fixtures are affected).
- Validate with `just test -p codex-app-server-protocol`.
- Avoid boilerplate tests that only assert experimental field markers for individual
  request fields in `common.rs`; rely on schema generation/tests and behavioral coverage instead.

</INSTRUCTIONS>
<environment_context>
  <cwd>/private/var/folders/bn/jq675y6x5m1d1x83vyh99f5c0000gn/T/codex-arm-XXXXXX.EFHBunAWhs/codex-rs</cwd>
  <shell>zsh</shell>
  <current_date>2026-05-25</current_date>
  <timezone>America/New_York</timezone>
</environment_context>
```

## [2] role=user

```
Add a unit test inside the `#[cfg(test)] mod tests` block of the agent_eval module that asserts `classify_result` returns an `AgentEvalResult::Excluded` variant whenever `valid_for_comparison=false`, regardless of the score arguments. Name the test `classify_result_excluded_when_pair_invalid`.
```

## [3] role=assistant

```
I’m going to locate the `agent_eval` module and existing `classify_result` tests first, then add the requested unit test with the smallest possible patch.
```

## [4] role=assistant

```
I didn’t get any matches from the first search, so I’m checking the repo layout and then I’ll search from the workspace root for `agent_eval`.
```

## [5] role=assistant

```
I found the module at `context-harness/src/agent_eval.rs`; next I’m reading the `classify_result` function and the current test block so I can add the exact test in place.
```

## [6] role=assistant

```
I have the exact test module location; I’m adding `classify_result_excluded_when_pair_invalid` next to the existing `classify_*` tests and will assert the excluded variant across contrasting score inputs.
```

## [7] role=assistant

```
I’ve applied the test patch in `context-harness/src/agent_eval.rs`. Next I’m running `just fmt` and then `just test -p codex-context-harness` to validate it.
```

## [8] role=assistant

```
Formatting completed. I’m running the targeted crate tests now: `just test -p codex-context-harness`.
```

## [9] role=assistant

```
The targeted test run is in progress and currently compiling dependencies for `codex-context-harness`; I’ll report back as soon as nextest finishes.
```

## [10] role=assistant

```
Tests are still running; the new test already passed in the stream, and I’m waiting for the command to fully finish and return the final summary.
```

## [11] role=assistant

```
The crate test command is still running because three `ri_packet_regressions` tests are slow and timing out/retrying; this appears unrelated to the new unit test, which already passed. I’m waiting for the command’s final exit summary.
```

## [12] role=assistant

```
The full `codex-context-harness` suite failed due three pre-existing timeout-heavy `ri_packet_regressions` tests. I’m running a narrowed `just test` invocation for the new test name to confirm this patch is green.
```

## [13] role=assistant

```
I noticed `just fmt` introduced a lot of unrelated style churn in the same file, so I’m trimming the diff back down to only the intended test addition.
```

## [14] role=assistant

```
I’ve reduced the diff back to the single requested test addition. I’m rerunning the focused test once more on the final file state.
```

## [15] role=assistant

```
Added the requested unit test `classify_result_excluded_when_pair_invalid` in [`context-harness/src/agent_eval.rs`](/private/var/folders/bn/jq675y6x5m1d1x83vyh99f5c0000gn/T/codex-arm-XXXXXX.EFHBunAWhs/codex-rs/context-harness/src/agent_eval.rs:1989).

The test calls `classify_result` with `valid_for_comparison = false` for two contrasting score setups (including swapped vanilla/treatment ordering) and asserts the result is always `AgentEvalResult::Excluded`.

Validation:
- `just test -p codex-context-harness classify_result_excluded_when_pair_invalid` passed.
- I also ran `just test -p codex-context-harness`; the new test passed there too, but that full crate run fails due three existing `ri_packet_regressions` timeouts unrelated to this change.
```
