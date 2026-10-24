#!/usr/bin/env sh
# Fail when the dependency graph needs a C++ toolchain.
#
# `model2vec-rs`'s tokenizer backend features (`onig`, `fancy-regex`) also
# enable `tokenizers/esaxx_fast`, which compiles C++. ft selects the
# backend on `tokenizers` directly to avoid it; this guards against a
# dependency update re-enabling either path.
set -eu
cd "$(dirname "$0")/.."

if cargo tree -q -p ft -e features -i esaxx-rs 2>/dev/null | grep -q 'esaxx-rs feature "cpp"'; then
    echo "error: esaxx-rs is built with its C++ \"cpp\" feature" >&2
    cargo tree -q -p ft -e features -i esaxx-rs >&2
    exit 1
fi
if cargo tree -q -p ft --prefix none 2>/dev/null | grep -Eq '^onig(_sys)? '; then
    echo "error: an onig (Oniguruma, C) crate is in the dependency graph" >&2
    exit 1
fi
echo "ok: no C++ toolchain required"
