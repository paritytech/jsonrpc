#!/bin/bash

set -eu

ORDER=(core server-utils tcp ws http ipc stdio pubsub core-client/transports core-client derive test)

# First display the plan
for crate in ${ORDER[@]}; do
	cd $crate > /dev/null
	VERSION=$(grep "^version" ./Cargo.toml | sed -e 's/.*"\(.*\)"/\1/')
	echo "$crate@$VERSION"
	cd - > /dev/null
done

read -p ">>>>  Really publish?. Press [enter] to continue. "

set -x

cargo clean

# Then actually perform publishing.
for crate in ${ORDER[@]}; do
	cd $crate
	VERSION=$(grep "^version" ./Cargo.toml | sed -e 's/.*"\(.*\)"/\1/')
	echo "Publishing $crate@$VERSION"
	sleep 5
	cargo publish $@ || read -p ">>>>> Publishing $crate failed. Press [enter] to continue. "
	cd -
done

# Make tags in one go
for crate in ${ORDER[@]}; do
	cd $crate
	VERSION=$(grep "^version" ./Cargo.toml | sed -e 's/.*"\(.*\)"/\1/')
	echo "Tagging $crate@$VERSION"
	git tag -a "$crate-$VERSION" -m "$crate $VERSION" || true
	cd -
done

git push --tags

VERSION=$(grep "^version" ./core/Cargo.toml | sed -e 's/.*"\(.*\)"/\1/')
echo "Tagging main $VERSION"
git tag -a v$VERSION -m "Version $VERSION"
git push --tags
