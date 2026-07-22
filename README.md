# hm

<p align="center">
  <img src="logo.svg" width="220" alt="hm">
</p>

Minimal thought-capture CLI. Writes to a single markdown file in a git repo.

```
hm                       # open TUI
hm "some thought"        # capture
hm ls                    # list recent entries (with commit hash IDs)
hm log                   # show full commit history of notes file
hm delete <hash>         # delete entry by commit hash
hm search <query>        # search entries (case-insensitive)
hm view <file>           # view a markdown file
hm push                  # push to remote
hm config ls             # show all config values
hm config set <key> <v>  # set a config value
hm init --repo <url>     # first-time setup
hm draft ls              # list drafts
hm draft create "title"  # new encrypted draft, opens $EDITOR
hm draft promote <slug>  # publish a draft
hm draft push            # push blog repo to remote
hm post ls               # list published posts
hm post demote <slug>    # move a published post back to drafts
hm post push             # push blog repo to remote
hm quiz [name]           # quiz yourself on a study source
```

## setup

```bash
brew install age          # encryption for drafts
cargo install --path .
hm init --repo git@github.com:you/your-notes-repo.git
age-keygen -o ~/.config/hm/age.key
```

Creates `~/.local/share/hm` (cloned repo) and `~/.config/hm/config.toml`. See [`config.example.toml`](config.example.toml) for all available options.

## notes demo

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

Thoughts with `?` get a reply from [ollama](https://ollama.com). Set `ollama = true` in config. 

## blog demo

```
% hm config set blog_repo ~/github/my-blog
% hm config set age_key ~/.config/hm/age.key

% hm draft create "you can't subpoena a weight matrix"
Created draft: you-cant-subpoena-a-weight-matrix
Opening in $EDITOR...

% hm draft ls
you-cant-subpoena-a-weight-matrix  2026-07-19  you can't subpoena a weight matrix

% hm draft promote you-cant-subpoena-a-weight-matrix
Published: you-cant-subpoena-a-weight-matrix

% hm post ls
you-cant-subpoena-a-weight-matrix  2026-07-19  you can't subpoena a weight matrix

% hm post push
Pushed → https://github.com/src-kearney/srock.rocks.git
```

Drafts are encrypted at rest with [age](https://age-encryption.org) in the blog repo. Published posts are plaintext. Promote a draft to publish.

## commit log format

Every C~~R~~UD operation is a git commit. `hm push` sends them upstream to your notes repo. Each operation writes a commit with a consistent prefix:

```
capture: questioning bd-2 pedal as neural net activation
edit: semantic search using homomorphic encryption
delete: out of the box thinking observation
```

With `llm = true`, capture commits use a short LLM-generated summary. Otherwise the full thought text is used (truncated at 72 chars).

Filter by operation: `git log --grep="^capture"`, `git log --grep="^edit"`, `git log --grep="^delete"`.
