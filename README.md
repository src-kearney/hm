# hm

<p align="center">
  <img src="logo.svg" width="220" alt="hm">
</p>

Minimal thought-capture CLI. Writes to a single markdown file in a git repo.

```
hm "some thought"     # capture
hm ls                 # list recent entries (with commit hash IDs)
hm delete <hash>      # delete entry by commit hash
hm push               # push to remote
hm tui                # interactive TUI
hm init --repo <url>  # first-time setup
```

## setup

```bash
cargo install --path .
hm init --repo git@github.com:you/your-notes-repo.git
```

Creates `~/.local/share/hm` (cloned repo) and `~/.config/hm/config.toml`.

## demo

```
% hm ls
79b8f00  2026-07-19 14:59  this should have semantic search with homomorphic encyption
df27d25  2026-07-19 14:57  am i an out-of-the-box thinker if i only use out-of-the-box solutions?

% hm new thought
a3f91bc  2026-07-19 15:29 — new thought

% hm ls
4727282  2026-07-19 15:29  new thought
79b8f00  2026-07-19 14:59  this should have semantic search with homomorphic encyption
df27d25  2026-07-19 14:57  am i an out-of-the-box thinker if i only use out-of-the-box solutions?

% hm delete 4727282
Deleted 4727282.

% hm "has anyone ever used a boss bd-2 as a nonlinearity in a neural net?"
4727282  2026-07-19 15:33 — has anyone ever used a boss bd-2 as a nonlinearity in a neural net?
  → No documented examples, but neural nets have been used to model the BD-2 — running it in reverse as an activation would be novel.

% hm push
Pushed → https://github.com/src-kearney/thoughts.git
```

Double quotes `"` are required when the thought contains shell special characters like `?`.

## llm replies

With `llm = true` in `~/.config/hm/config.toml`, thoughts containing `?` trigger a local LLM reply via [Ollama](https://ollama.com). Install Ollama, run `ollama pull <model>`, then set `llm_model` to any pulled model (default: `llama3.2`).

```toml
llm = true
llm_model = "mistral"
```

## commit log format

Every C~~R~~UD operation is a git commit. `hm push` sends them upstream to your notes repo. Each operation writes a commit with a consistent prefix:

```
capture: has anyone e...
edit: this should h...
delete: am i an out-...
```

Filter by operation: `git log --grep="^capture"`, `git log --grep="^edit"`, `git log --grep="^delete"`.
