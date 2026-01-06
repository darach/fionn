#!/bin/bash -eu

cd $SRC/fionn

# Build all fuzz targets
cargo +nightly fuzz build --release

# Copy fuzz target binaries to $OUT
for target in $(cargo +nightly fuzz list); do
    cp fuzz/target/x86_64-unknown-linux-gnu/release/$target $OUT/
done

# Copy seed corpus if available
for corpus_dir in fuzz/corpus/*/; do
    target_name=$(basename "$corpus_dir")
    if [ -d "$corpus_dir" ] && [ "$(ls -A "$corpus_dir")" ]; then
        zip -j $OUT/${target_name}_seed_corpus.zip "$corpus_dir"/* 2>/dev/null || true
    fi
done
