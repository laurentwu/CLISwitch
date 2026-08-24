#!/usr/bin/env sh
set -eu
case "${1:-}" in
  --version|-V|version) printf '%s\n' "fixture-cli 0.1.0" ;;
  *) printf '%s\n' "fixture CLI: no real user files are touched" ;;
esac
