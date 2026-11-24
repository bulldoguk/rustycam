#!/usr/bin/env bash
# Fail unless the Dockerfile's rust builder tag matches rust-toolchain.toml,
# so development, CI and the shipped image all use one Rust version.
set -euo pipefail
cd "$(dirname "$0")/../rustycam"
want=$(sed -n 's/^channel *= *"\(.*\)"/\1/p' rust-toolchain.toml)
have=$(sed -n 's/^FROM rust:\([^ ]*\)-alpine.*/\1/p' Dockerfile)
echo "rust-toolchain.toml: ${want:-?}  Dockerfile: ${have:-?}"
[ -n "$want" ] && [ "$want" = "$have" ]
