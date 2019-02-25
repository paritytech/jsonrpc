#!/bin/sh
cargo clippy --all-features --all-targets -- -D clippy::pedantic -D clippy::correctness -D clippy::complexity -D clippy::perf -W clippy::style -W clippy::nursery
