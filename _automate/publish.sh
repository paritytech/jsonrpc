#!/bin/bash

set -exu

VERSION=$(grep "^version" ./core/Cargo.toml | sed -e 's/.*"\(.*\)"/\1/')
ORDER=(core server-utils tcp ws http ipc stdio pubsub core-client/transports core-client derive test)

echo "Publishing version $VERSION"
cargo clean

for crate in ${ORDER[@]}; do
	cd $crate
	echo "Publishing $crate@$VERSION"
	sleep 5
	cargo publish $@ || read -p "\n\t Publishing $crate failed. Press [enter] to continue."
	cd -
done

echo "Tagging version $VERSION"
git tag -a v$VERSION -m "Version $VERSION"
git push --tags
