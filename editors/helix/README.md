# BMake grammar for Helix

Helix needs a compiled tree-sitter grammar, not just a query file. Steps:

1. `npm install -g tree-sitter-cli` (or use the repo's own toolchain)
2. `cd editors/helix/tree-sitter-bmake && tree-sitter generate`
3. Append the contents of `languages.toml.snippet` to your
   `~/.config/helix/languages.toml`, pointing `source.path` at this
   directory's absolute path.
4. Run `hx --grammar build` so Helix compiles and loads it.
5. Copy `queries/highlights.scm` to
   `~/.config/helix/runtime/queries/bmake/highlights.scm`.

Known limitation: `.bm.kts` currently gets plain BMake highlighting only —
true Kotlin-region injection (like the VS Code/Neovim integrations do) would
require extending the grammar with a distinct `kotlin_line` node type first,
which isn't done yet.