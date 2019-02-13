#!/bin/bash

set -exu

ORDER=(core server-utils tcp ws http ipc stdio pubsub macros derive test)

for crate in ${ORDER[@]}; do
	cd $crate
	cargo publish $@
	cd -
done

