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

set +x

# Then actually perform publishing.
for crate in ${ORDER[@]}; do
	cd $crate
	while : ; do
		VERSION=$(grep "^version" ./Cargo.toml | sed -e 's/.*"\(.*\)"/\1/')
		# give the user an opportunity to abort or skip before publishing
		RET=""
		read -t 5 -p "Publishing $crate@$VERSION. Type [s] to skip. " RET || true
		if [ "$RET" != "s" ]; then
			set -x
			cargo publish $@ || read -p ">>>>> Publishing $crate failed. Press [enter] to continue or type [r] to retry. " CHOICE
			set +x
			if [ "$CHOICE" != "r" ]; then
				break
			fi
		else
			echo "Skipping $crate@$VERSION"
			break
		fi
	done

  echo "  Waiting for published version $VERSION to be available..."
	CRATE_NAME=$(grep "^name" ./Cargo.toml | sed -e 's/.*"\(.*\)"/\1/')
	LATEST_VERSION=0
	while [[ $LATEST_VERSION != $VERSION ]]
	do
	  sleep 3
	  LATEST_VERSION=$(cargo search "$CRATE_NAME" | grep "^$CRATE_NAME =" | sed -e 's/.*"\(.*\)".*/\1/')
	  echo "    Latest available version: $LATEST_VERSION"
	done
	cd -
done

# Make tags in one go
for crate in ${ORDER[@]}; do
	cd $crate
	VERSION=$(grep "^version" ./Cargo.toml | sed -e 's/.*"\(.*\)"/\1/')
	echo "Tagging $crate@$VERSION"
	set -x
	git tag -a "$crate-$VERSION" -m "$crate $VERSION" || true
	set +x
	cd -
done

set -x

git push --tags

VERSION=$(grep "^version" ./core/Cargo.toml | sed -e 's/.*"\(.*\)"/\1/')
echo "Tagging main $VERSION"
git tag -a v$VERSION -m "Version $VERSION"
git push --tags
