# hm — design

`hm` started as a minimal thought-capture CLI. It's becoming a unified personal computing layer: notes, writing, background compute, and eventually the application-layer demo for a heterogeneous inference stack.

## what it is

A terminal tool that treats everything as a git-backed operation. Every thought captured, every draft written, every job fired — committed to a repo, pushable to a remote, auditable forever. The TUI is the home screen. The CLI is the scripting surface.

## layers

### notes

Thoughts go into `hm-YYYY.md` (one file per year), prepended newest-first, formatted as markdown blocks:

```markdown
## 2026-07-19 14:59

has anyone ever used a boss bd-2 as a nonlinearity in a neural net?

---
```

Every capture is a git commit. Commit messages are LLM-generated summaries when Ollama is running, full text otherwise. The notes repo is public — the blog repo (see below) is where private content lives.

### llm

Ollama runs locally. A classifier prompt decides whether a thought warrants a reply. When it does, a reply fires and streams into the terminal while a word-cloud loader (words sampled from your own notes, frequency-weighted) plays in the background.

Long-term: Ollama gets replaced by Remora — the same heterogeneous inference stack used for SETI and bioelectric sim runs through the same backend as your thought replies.

### writing

A separate blog repo (`srock.rocks`) holds blog content. Drafts are encrypted at rest using [age](https://age-encryption.org) (X25519 + ChaCha20-Poly1305) — the public repo on GitHub sees opaque blobs. Published posts are plaintext.

```
blog/
  drafts/
    you-cant-subpoena-a-weight-matrix.json.age   ← encrypted
  published/
    converting-mlir-to-mlir.json                  ← plaintext
  manifest.json                                   ← always plaintext (titles, slugs, dates)
```

```bash
hm write "title"      # new draft → $EDITOR → encrypt → commit
hm draft ls           # list drafts from manifest
hm publish <slug>     # decrypt → move to published/ → commit
```

Every write and publish is a uniform git commit to the blog repo:
```
draft: you can't subpoena a weight matrix
publish: converting mlir to mlir
```

The content format is currently JSON chunks (rendered by brainlog). When brainlog gets an MDX renderer, the format migrates to `.mdx` — prose becomes free-form markdown, custom components (`<Cave />`, `<AnnotatedCodePair />`) drop in inline. The encryption layer is format-agnostic.

Long-term: age gets replaced by HEIR-generated FHE. Drafts become searchable over ciphertext without decryption — bootstrapped on the OrangeCrab FPGA via CIRCT-lowered NTT kernels. The path is: age now → HEIR/CKKS later, same encrypted blob at rest.

### compute

```bash
hm run <cmd>          # spawn daemon, caffeinate on macOS, log to ~/.local/share/hm/jobs/
hm jobs               # list running jobs
hm logs <id>          # tail job output
hm kill <id>          # stop job
```

`hm run seti <file>` fires a de-Doppler pipeline. `hm run levin <config>` fires a gap-junction simulation. Same job API, different payloads. When Remora is the backend, `hm run` dispatches to GPU, FPGA, or CPU depending on what's available.

## through-lines

Every layer maps to a project:

| hm feature | project |
|---|---|
| LLM replies | Remora transformer inference |
| `hm run` jobs | SETI de-Doppler, Levin bioelectric sim |
| Draft encryption | HEIR / FHE compiler exploration |
| FPGA inference | CIRCT → OrangeCrab / Glider |
| Writing format | brainlog / srock.rocks |
| Embeddings + semantic search | Remora embedding pipeline |
| `hm export --book` | LLM-structured anthology of notes (content verbatim, LLM writes only metadata/transitions) |

`hm` is the application layer. Everything else is the stack underneath it.

## hmd

The e-ink device. `hm` running on low-power ARM hardware with a local model (via Remora, via FPGA). Same CLI, same git-backed storage, same job layer — offline-capable, syncs on connect. Passive SETI compute when idle.
