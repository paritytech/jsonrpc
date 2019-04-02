#!/bin/bash

set -exu

ORDER=(core client server-utils tcp ws ws/client http ipc stdio pubsub derive test)

for crate in ${ORDER[@]}; do
	cd $crate
	cargo publish $@
	cd -
done

