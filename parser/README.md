# kanata-parser

A parser for configuration language of [kanata](https://github.com/jtroo/kanata).

This crate does not follow semver. It tracks the version of kanata.

## Tree-sitter grammar

The `parser/tree-sitter-kanata` directory contains the JavaScript grammar used by the
tree-sitter CLI and editor integrations.

To regenerate parser artifacts locally:

```bash
cd parser/tree-sitter-kanata
tree-sitter generate
```

The Rust helper exposed by the parser crate is available behind the `treesitter` feature.
