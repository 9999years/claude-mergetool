#!/usr/bin/env sh

set -e

cargo metadata --format-version=1 \
    | jq --raw-output '.packages[]|select(.name == "claude-mergetool").version'
