# Recording a demo

## Desktop app

```bash
cargo run -p atlas-tauri
# Browse… or paste a repository path, then Open
# Index if needed
# Search for a symbol, select it, and explore Callers, Callees and Impact
# Toggle العربية to show the RTL UI
```

For semantic search, run `atlas model download` once (or use the download
button in the semantic panel), build the embeddings, then switch the search
mode to **Semantic** and enter a natural-language query.

A captured screenshot is available at `docs/atlas-desktop.png`; the Arabic UI
is at `docs/atlas-desktop-ar.png`. Semantic search is shown in
`docs/atlas-semantic.png` (English) and `docs/atlas-semantic-ar.png` (Arabic).

## CLI

To record a CLI demo GIF/animation:

## Using asciinema
```bash
asciinema rec demo.cast
# run: atlas index <repo>, atlas report, atlas search <query>,
#      atlas embed, atlas search --semantic <query>, atlas callers <sym>,
#      atlas impact <sym>
asciinema play demo.cast
# convert with agg: agg demo.cast docs/demo.gif
```

## Using VHS
Create a `demo.tape` and run:
```bash
vhs demo.tape
```

Place the resulting GIF at `docs/demo.gif` (or update README). The README currently has a placeholder.
