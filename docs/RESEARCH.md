# Research record

Two read-only studies from 11 November 2026, done before building anything. Neither ran a model or
used the API key. Numbers are the cited authors' own unless marked independent. `†` marks what
could not be confirmed from current official documentation.

## 1. Memory, and agents that make their own skills

### The question

Does a harness that "evolves" need to run many projects before it gets better, and is there a
form that works out of the box?

### Answer

"Evolving" covers three different things.

- **Knowledge written or tuned before shipping** (built-in skills, a prompt tuned on a benchmark)
  works in the first session. The developer pays for the runs.
- **Memory learned on the user's machine** helps only from the second related session in the same
  project, typically after 1 to 40 similar tasks. When the model writes its own memories or
  skills the measured effect is often zero or negative, and worst on small models.
- **Harnesses that rewrite their own code** need dozens of benchmark iterations, weeks and
  thousands of dollars in a lab, and have been caught gaming their objective.

### Evidence

| System | What is stored and when | Helps in session 1? | Reported effect and caveats |
|---|---|---|---|
| Reflexion [1] | A reflection after a failed trial, injected on retry of the same task | No | HumanEval 91.0 vs 80.1 (GPT-4). MBPP got worse (77.1 vs 80.1): self-written tests gave false passes. No gain on a weak model |
| ExpeL [2] | Insights and successful trajectories from a training half | No | HotpotQA 39 vs 28. Needs roughly 50 training tasks |
| Voyager [3] | Code skills, kept only after a critic confirms success | No | 3.3× more items in Minecraft. With GPT-3.5 writing the code: 5.7× fewer |
| Agent Workflow Memory [4] | Recipes induced from trajectories judged successful | No | WebArena 35.5 vs 23.5. Misjudged successes become wrong workflows |
| Dynamic Cheatsheet [5] | A cheatsheet the model rewrites after each query | No | Game of 24: 10% to 99% (GPT-4o). GPT-4o-mini on AIME: 16.7 to 13.3 |
| ACE [6] | Bullets with helpful/harmful counters, updated by deltas | No | AppWorld 42.4 to 59.4. Without reliable feedback it hurt (70.7 to 67.3) |
| MemGPT / Letta [7][8][9] | Memory blocks in the prompt, edited by tool call | Only after an exploration run | Vendor-reported; sleep-time results are on maths |
| AGENTS.md / CLAUDE.md [12] | A project file, always loaded | Yes, if a person wrote it | Independent: developer-written +2.4% (not significant); model-generated −0.5% to −2% at +20–23% cost |
| Claude Code skills [13][15] | Name and description in context, body on use | Yes | Independent (SkillsBench): curated skills 33.9 to 50.5; self-generated −8.1 to −11.5; two or three focused skills beat four or more |
| Cursor [17][18] | Rules by glob or description | Yes for rules | Auto-generated Memories were removed in version 2.1 |
| Cline memory bank [19] | Six files the model rewrites, all read every task | No | No measurements; a token cost every session |
| Darwin Gödel Machine [22], SICA [24] | The agent rewrites its own code against a benchmark | Lab only | SWE-bench 20% to 50% over 80 iterations and two weeks; SICA about $7,000. Faked a test log |
| GEPA [26] | Prompt evolution on a training set | Lab only | 1,839 to 7,051 rollouts per benchmark |
| Live-SWE-agent [25] | Helper scripts written during the task, nothing kept | Yes | Helps a strong model (77.4 vs 74.2); GPT-5-nano fell from 44 to 14 |

Two more findings: agents copy retrieved experiences, so stored mistakes propagate [27]; and no
reliable multi-session coding result exists yet [28][29].

### What Presage should build, by expected value

1. **Built-in playbooks, selected by the harness.** Six to ten hand-written playbooks of about 200
   words (debug a failing test, add a feature, refactor, start from nothing, UI design and
   anti-slop, per-ecosystem conventions), compiled into the binary. Selected from task keywords
   and workspace markers, at most two per session; SkillsBench names skills the model never found
   as a failure cause, hence harness-side selection (an inference). Appended to the first user
   message in `Agent::opening`, never the system prompt, so the cached prefix stays byte-identical;
   300 to 500 tokens once. New `playbooks.rs`, a `playbooks` flag in `state.rs`, a `no-playbooks`
   bench variant. A playbook that does not pay in the benchmark is deleted.
2. **Verified project facts**, extending the reflex memory in `lessons.rs`, with no model call:
   the check command that passed and gated `finish`, passing build and lint commands, required
   setup. Stored per project beside `lessons.jsonl`. At most eight `known here:` lines in the
   first message; better, the harness acts on them (preflight and auto-watch the remembered test
   command when the task names none). Kept honest by counters, demotion when a remembered command
   fails, invalidation when a manifest changes, and a `presage lessons` list/forget command.
3. **Human-written notes only.** `AGENTS.md`, `CLAUDE.md` and `.presage.md` are already read
   (`prompt.rs`, capped at 12,000 characters). Add `presage remember "<text>"`.
4. **Offline tuning by the developer** of the prompt and the playbooks, against held-out tasks.
   The blocker is task count, not money: seven tasks would overfit.

Not building: model-written reflections, skills or auto-memory; runtime tool creation; embedding
or vector memory; editable memory blocks in the system prompt (every edit busts the cached
prefix); a memory bank read every session; a self-rewriting harness; sleep-time compute.

### A benchmark that can show learning

`presage bench --series`: one synthetic project with four to six ordered tasks that share a quirk
(for example tests that only run through `make check` after `make gen`), run in sequence in the
same workspace and data directory. Arms: warm (memory kept), cold (wiped between sessions), drift
(the quirk changes at session four, to price staleness). Report solved, false-done, turns and cost
per position: position one must match across arms and the gain should appear from position two.
Include one task where memory is irrelevant and warm must be no worse. At least ten repeats.

## 2. Other harnesses

### Running them against Logfare

Logfare documents Chat Completions, Responses and Anthropic Messages endpoints, so every candidate
is reachable in principle.

| # | Harness | Route | Turn or cost cap | Metrics from | Verdict |
|---|---|---|---|---|---|
| 1 | mini-swe-agent 2.x | chat completions (LiteLLM) | `agent.step_limit`, `agent.cost_limit` | trajectory `info.model_stats` | Cheapest and fairest; the usual baseline |
| 2 | opencode | chat completions, custom provider | agent `steps` (2.x) | `--format json`†, `opencode stats --json` | Good; pin a major version |
| 3 | Qwen Code | chat completions† | `--max-session-turns`, `--max-wall-time` | `--output-format json` | The only one with all caps built in |
| 4 | Goose | chat completions (`OPENAI_HOST`) | `--max-turns` | `--output-format json` | Easy; token fields undocumented |
| 5 | OpenHands | chat completions (LiteLLM) | SDK only | SDK metrics | CLI called unmaintained; needs a driver |
| 6 | pi | chat completions | none | `--mode json` | Lean prompt; cap externally |
| 7 | Codex CLI | Responses only | none | `--json` usage | Compatibility over Logfare unverified |
| 8 | Claude Code | Anthropic Messages only | `--max-turns`, `--max-budget-usd` | `--output-format json` | Anthropic says non-Claude models through gateways are unsupported |
| 9 | aider | chat completions | none | `--analytics-log`† | A one-shot editor, not an agent loop |
| 10 | SWE-agent | chat completions (LiteLLM) | `per_instance_call_limit` | trajectory files | Maintenance only; superseded by mini |

```
# 1  uv tool install mini-swe-agent
cd $WS && OPENAI_API_KEY=$KEY MSWEA_COST_TRACKING=ignore_errors mini -y --exit-immediately -m openai/$MODEL -t "$TASK" \
  -o $OUT/traj.json -c mini.yaml -c model.model_kwargs.api_base=https://logfare.ai/v1 -c agent.step_limit=30 -c agent.cost_limit=0
# 2  npm i -g @opencode/cli        (1.x: opencode-ai)
cd $WS && LOGFARE_API_KEY=$KEY opencode run --standalone --auto --format json --model logfare/$MODEL "$TASK"
# 3  qwen -p "$TASK" --yolo --output-format json --max-session-turns 30 --max-wall-time 10m -m $MODEL
# 4  cd $WS && GOOSE_DISABLE_KEYRING=1 GOOSE_PROVIDER=openai GOOSE_MODEL=$MODEL OPENAI_HOST=https://logfare.ai OPENAI_API_KEY=$KEY \
  goose run --no-session --max-turns 30 --output-format json -t "$TASK"
# 5  uv tool install openhands --python 3.12
cd $WS && LLM_MODEL=openai/$MODEL LLM_API_KEY=$KEY LLM_BASE_URL=https://logfare.ai/v1 openhands --headless --json --override-with-envs -t "$TASK"
# 7  npm install -g @openai/codex   (config.toml: model_provider with base_url and wire_api="responses")
cd $WS && LOGFARE_API_KEY=$KEY codex exec --json --full-auto --skip-git-repo-check --ephemeral "$TASK"
# 9  cd $WS && OPENAI_API_BASE=https://logfare.ai/v1 OPENAI_API_KEY=$KEY aider --model openai/$MODEL --message "$TASK" --yes-always --no-auto-commits
```

None of these commands has been run. Inferred rather than documented: mini's `api_base` as a
`-c key=value`; opencode's provider wiring for 2.x; Qwen Code's install command and endpoint
variables; the exact aider flag combination.

Integration: an external variant in `crates/presage-cli/src/bench/mod.rs`. `attempt()` already
materialises the workspace and calls `verify()`; replace only the `harness.start(req)` block with
a spawned command (working directory the workspace, killed at `time_budget`), fill `Stats` from an
adapter and price every harness with Presage's own `prices_for()`. The fairest meter is a logging
reverse proxy in front of Logfare that counts requests and usage identically for all.

### Fairness protocol

- Equal across harnesses: exact model id and endpoint; reasoning effort (Presage sends one and
  ratchets from `high`, others send provider defaults; fix one level or report both, since one
  study found thinking on or off moved mini-swe-agent by 11 points); provider-default temperature;
  the task text verbatim; the same fresh copy and the same `verify`; a 30-request cap and 600 s.
- A turn is one completed model request. Title-generation and compaction calls count in tokens.
- 7 tasks × 5 runs cannot support a solve-rate claim. "What Does a Harness Buy?" found that a
  harness swap and a plain rerun each flipped 13% of tasks. Use paired tasks, McNemar or bootstrap
  intervals, at least 100 task instances per cell for solve rate, and report a same-cell rerun
  beside any gap. Turn and cost claims need fewer.
- Record harness version and config, first-request preamble tokens, requests, fresh, cached,
  output and reasoning tokens, billed and uncached cost at one price list, wall time, outcome
  class (solved, false-done, budget, crash, provider error), retries.
- Keep each harness's shipped system prompt. Disclose that Presage repairs malformed tool calls
  and retries eight times. Use a fresh HOME per run. Interleave harnesses in time.
- Home-field risk: all seven easy tasks name the check or the file. Add tasks that do not.

### Mechanisms worth adopting

1. **Reject and revert broken edits** (SWE-agent: 18.0% with its lint guard, 15.0% without; a
   GPT-4-era figure). Presage writes the broken file and reports a surprise. `tools/edit.rs`,
   `tools/write.rs`; ablate as `no-edit-guard`.
2. **Red-then-green gate** (Agentless; mini's reproduce, fix, test prompt). A check counts only if
   it was seen failing first, or was written before the first edit when the task has no tests.
   `watches.rs` and the finish validation in `script.rs`; the metric is `false_done`.
3. **Verified retries** (Large Language Monkeys: 15.9% to 56% with 250 samples, only with an
   automatic verifier). On budget or an unverified finish, restart from a clean copy up to k
   times. Report cost per solved task. Useless while the solve rate is 100%.
4. **Architect and editor** (aider: 85.0% vs 79.7% solo; a 2024 result). GLM-5.3 plans once,
   DeepSeek Flash executes under the gate. A model switch forfeits the prefix cache.
5. **Repo map** (aider: graph-ranked symbols in about 1k tokens). `bootstrap.rs` lists top-level
   entries only. Needs a multi-file task to show anything.
6. **Mechanical finish review**: at `finish`, check that files the task forbids editing are
   untouched and no scratch files remain. Source cited from memory, unverified.
7. **Preamble budget.** Measured preambles: 829 tokens (mini), 7,025 (opencode), 16,581 (Claude
   Code), up to 3× cost on the same model. Presage's fixed prefix was 1,193 estimated tokens.

Not worth adopting: LLM summarisation of history (it cost 3.9 to 4.9 points in one study, and
plain masking did as well in another; Presage's decay already masks), general subagents, planning
tools.

## Sources

Memory and skills: [1] arxiv.org/abs/2303.11366 · [2] 2308.10144 · [3] 2305.16291 · [4] 2409.07429
· [5] 2504.07952 · [6] 2510.04618 · [7] 2310.08560 · [8] 2504.13171 · [9] letta.com/blog/skill-learning
· [12] 2602.11988 · [13] 2602.12670 · [15] code.claude.com/docs/en/skills · [17] cursor.com/docs/context/rules
· [18] forum.cursor.com/t/cant-clear-memories/148254/2 · [19] docs.cline.bot/prompting/cline-memory-bank
· [22] 2505.22954 · [24] 2504.15228 · [25] 2511.13646 · [26] 2507.19457 · [27] 2505.16067
· [28] 2507.00014 · [29] frontiermemory.com/blog/v1 (bare numbers are arXiv ids).

Harnesses: mini-swe-agent.com/latest/usage/mini/ · swe-agent.com/latest/usage/cl_tutorial/ ·
aider.chat/docs/scripting.html · aider.chat/docs/repomap.html · aider.chat/2024/09/26/architect.html
· docs.openhands.dev/openhands/usage/cli/headless · opencode.ai/docs/cli/commands/ ·
github.com/openai/codex · goose-docs.ai/docs/guides/goose-cli-commands ·
code.claude.com/docs/en/llm-gateway · qwenlm.github.io/qwen-code-docs/en/users/features/headless/
· pi.dev/docs/latest/cli · docs.harborframework.com/agents/pre-integrated-agents · logfare.ai/docs.
Papers (arXiv): 2405.15793 (SWE-agent) · 2407.01489 (Agentless) · 2407.21787 (Large Language
Monkeys) · 2508.21433 (Complexity Trap) · 2609.32459 (Beyond the Model) · 2610.04433 (What Does a
Harness Buy?) · 2610.32131 (Pairing Matters) · 2607.22585 (Scaffold Effect).
