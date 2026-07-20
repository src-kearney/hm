# hm

<p align="center">
  <img src="logo.svg" width="220" alt="hm">
</p>

Minimal thought-capture CLI. Writes to a single markdown file in a git repo.

```
hm "some thought"        # capture
hm ls                    # list recent entries (with commit hash IDs)
hm delete <hash>         # delete entry by commit hash
hm search <query>        # search entries (case-insensitive)
hm view <file>           # view a markdown file
hm push                  # push to remote
hm tui                   # interactive TUI
hm config ls             # show all config values
hm config set <key> <v>  # set a config value
hm init --repo <url>     # first-time setup
```

## setup

```bash
cargo install --path .
hm init --repo git@github.com:you/your-notes-repo.git
```

Creates `~/.local/share/hm` (cloned repo) and `~/.config/hm/config.toml`. See [`config.example.toml`](config.example.toml) for all available options.

## demo

```
% hm ls
79b8f00  2026-07-19 14:59  this should have semantic search with homomorphic encyption
df27d25  2026-07-19 14:57  am i an out-of-the-box thinker if i only use out-of-the-box solutions?

% hm just scribbling random note here
a3f91bc  2026-07-19 15:29 — just scribbling random note here

% hm ls
4727282  2026-07-19 15:29  just scribbling random note here
79b8f00  2026-07-19 14:59  this should have semantic search with homomorphic encyption
df27d25  2026-07-19 14:57  am i an out-of-the-box thinker if i only use out-of-the-box solutions?

% hm delete 4727282
Deleted 4727282.

% hm "has anyone ever used a boss bd-2 as a nonlinearity in a neural net?"
4727282  2026-07-19 15:33 — has anyone ever used a boss bd-2 as a nonlinearity in a neural net?
  → No documented examples, but neural nets have been used to model the BD-2 — running it in reverse as an activation would be novel.

% hm search "bd-2"
4727282  2026-07-19 15:33  has anyone ever used a boss bd-2 as a nonlinearity in a neural net?

% hm push
Pushed → https://github.com/src-kearney/thoughts.git
```

Double quotes `"` are required when the thought contains shell special characters like `?`.

## llm replies

With `llm = true` in `~/.config/hm/config.toml`, captured thoughts can trigger a local LLM reply via [Ollama](https://ollama.com). Install Ollama, run `ollama pull <model>`, then set `llm_model` to any pulled model (default: `llama3.2`).

```toml
llm = true
llm_model = "mistral"       # any model from ollama.com/library
llm_trigger = "heuristic"   # heuristic | classifier | always | off
```

`llm_trigger` controls when a reply fires:

| value | behavior |
|---|---|
| `heuristic` | reply only if thought contains `?` (default) |
| `classifier` | ask the LLM whether a reply is warranted before replying |
| `always` | reply to every captured thought |
| `off` | never reply (equivalent to `llm = false`) |

When `llm_trigger = "classifier"`, the prompt sent to the LLM defaults to:

```
Does this thought warrant a brief reply? The answer is most likely no — only say 'yes' for direct questions or thoughts that clearly invite a response. Answer only 'yes' or 'no': {thought}
```

Override it with `llm_classifier_prompt` in config. Use `{thought}` as the placeholder for the captured text.

## commit log format

Every C~~R~~UD operation is a git commit. `hm push` sends them upstream to your notes repo. Each operation writes a commit with a consistent prefix:

```
capture: has anyone e...
edit: this should h...
delete: am i an out-...
```

Filter by operation: `git log --grep="^capture"`, `git log --grep="^edit"`, `git log --grep="^delete"`.
