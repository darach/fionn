# fionn-cli

[![Crates.io](https://img.shields.io/crates/v/fionn-cli.svg)](https://crates.io/crates/fionn-cli)
[![License](https://img.shields.io/crates/l/fionn-cli.svg)](https://github.com/darach/fionn#license)

Command-line interface for fionn.

`fionn-cli` puts the full fionn toolkit on the command line: gron, ungron, diff, patch,
schema inference, JSONL streaming, CRDT sync, and format conversion between JSON, YAML,
TOML, and CSV. Install it with `cargo install fionn` and run `fionn --help`.

```bash
# Make JSON greppable
fionn gron data.json

# Diff two documents
fionn diff a.json b.json

# Stream JSONL with schema filtering
fionn stream --schema id,name large.jsonl
```

## Part of fionn

This crate provides the CLI for the [fionn](https://crates.io/crates/fionn) JSON
toolkit. For library usage, see the top-level [fionn](https://crates.io/crates/fionn)
crate.

## License

MIT OR Apache-2.0
