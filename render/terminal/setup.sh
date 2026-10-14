#!/bin/sh
set -eu
mkdir -p "$2"
cp "$1/render/terminal/desk.toml" "$2/"
