#!/bin/bash

set -exu

# NOTE https://github.com/sunng87/cargo-release is required.

VERSION=$(grep "^version" ./core/Cargo.toml | sed -e 's/.*"\(.*\)"/\1/')
echo "Publishing version $VERSION"
cargo clean
read -p "\n\t Press [enter] to continue."
cargo release --dry-run --sign --skip-push --no-dev-version
read -p "\n\t Press [enter] to confirm release the plan."
cargo release --sign --skip-push --no-dev-version
echo "Release finished."
git push
git push --tags
