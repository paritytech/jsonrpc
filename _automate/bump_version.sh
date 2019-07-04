#!/bin/sh

set -xeu

VERSION=$1
PREV_DEPS=$2
NEW_DEPS=$3

ack "^version = \"" -l | \
	grep toml | \
	xargs sed -i "s/^version = \".*/version = \"$VERSION\"/"

ack "{ version = \"$PREV_DEPS" -l | \
	grep toml | \
	xargs sed -i "s/{ version = \"$PREV_DEPS/{ version = \"$NEW_DEPS/"

ack " = \"$PREV_DEPS" -l | \
    grep md | \
    xargs sed -i "s/ = \"$PREV_DEPS/ = \"$NEW_DEPS/"

cargo check
