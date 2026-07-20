# future features

## x + bluesky integration

Post any thought directly to X or Bluesky via their APIs.

```
hm tweet 4727282
hm bsky 4727282
```

X requires OAuth 2.0 via a Twitter developer app. Bluesky uses ATP with email + app password. Tokens stored in `~/.config/hm/social.toml`.

---

## local llm responses

On each capture, a local LLM runs a quick classification to decide if the thought is worth responding to. Above some threshold it fires a reply printed inline.

```
% hm "has anyone ever used a boss bd-2 as a nonlinearity in a neural net"
2026-07-19 15:33 — has anyone ever used a boss bd-2 as a nonlinearity in a neural net
  → actually yes — Engel et al. used guitar pedal nonlinearities in neural audio synthesis (2019)
```

Model runs locally to start.

---

## llm-generated commit summaries

Instead of truncating the thought text for the commit message, a local LLM generates a short semantic summary.

```
capture: novel use of analog distortion circuits as learned activations
```

Richer git log, better grep signal.

---

## remora apple silicon target

Compile and run inference via Remora on Apple Silicon using a Metal backend. Replaces the Ollama subprocess with a direct call to Remora — same StableHLO pipeline, native GPU execution on M-series chips.

---

## obsidian plugin

Sync the notes repo into an Obsidian vault. A community plugin watches `hm.md` for changes and surfaces new entries as Obsidian notes, backlinks intact.

---

## semantic search with homomorphic encryption

`hm search <query>` runs semantic similarity over thought embeddings without decrypting them the notes repo stays encrypted at rest and the search computation happens over ciphertext.

```
% hm search "guitar pedals"
4727282  2026-07-19 15:33  has anyone ever used a boss bd-2 as a nonlinearity in a neural net?
```

Requires some OpenFHE/HEIR tinkering and a local embedding model. Enables private remote / cloud-based search.