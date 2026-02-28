# fionn-gron

[![Crates.io](https://img.shields.io/crates/v/fionn-gron.svg)](https://crates.io/crates/fionn-gron)
[![Documentation](https://docs.rs/fionn-gron/badge.svg)](https://docs.rs/fionn-gron)
[![License](https://img.shields.io/crates/l/fionn-gron.svg)](https://github.com/darach/fionn#license)

Make JSON greppable.

`fionn-gron` flattens a JSON document into discrete `path = value` assignments -- the
same idea as [gron](https://github.com/tomnomnom/gron), but backed by fionn's SIMD tape
for speed and zero-copy for memory efficiency. The output is plain text that `grep`,
`awk`, and `sort` handle naturally. `ungron` reassembles the assignments back into JSON.

```text
$ echo '{"name":"Alice","scores":[90,85]}' | fionn gron
json.name = "Alice";
json.scores[0] = 90;
json.scores[1] = 85;
```

## Part of fionn

This crate is one building block of the [fionn](https://crates.io/crates/fionn) JSON
toolkit. For the full feature set -- SIMD skipping, CRDTs, diff/patch, streaming, and
more -- see the top-level [fionn](https://crates.io/crates/fionn) crate.

## License

MIT OR Apache-2.0
