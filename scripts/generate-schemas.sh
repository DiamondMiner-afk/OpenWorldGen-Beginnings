#!/usr/bin/env sh
set -eu

destination="${1:-schemas}"
mkdir -p "$destination"

# Result-bundle v1 is a frozen compatibility artifact. The CLI emits v2.
# Checkpoint v1 is retained for inspection; the CLI emits incompatible v2.
if ! cmp -s schemas/result-bundle-v1.schema.json "$destination/result-bundle-v1.schema.json"; then
  cp schemas/result-bundle-v1.schema.json "$destination/result-bundle-v1.schema.json"
fi
if ! cmp -s schemas/checkpoint-v1.schema.json "$destination/checkpoint-v1.schema.json"; then
  cp schemas/checkpoint-v1.schema.json "$destination/checkpoint-v1.schema.json"
fi

target/debug/owg schema --kind capabilities --output "$destination/capabilities-v1.schema.json"
target/debug/owg schema --kind scenario --output "$destination/scenario-spec-v1.schema.json"
target/debug/owg schema --kind data-bundle --output "$destination/data-bundle-v1.schema.json"
target/debug/owg schema --kind data-vintage --output "$destination/data-vintage-v1.schema.json"
target/debug/owg schema --kind data-vintage-strict --output "$destination/data-vintage-v2.schema.json"
target/debug/owg schema --kind model-pack --output "$destination/model-pack-v1.schema.json"
target/debug/owg schema --kind policy --output "$destination/agent-policy-v1.schema.json"
target/debug/owg schema --kind decision-tape --output "$destination/decision-tape-v1.schema.json"
target/debug/owg schema --kind run-manifest --output "$destination/run-manifest-v1.schema.json"
target/debug/owg schema --kind result-bundle --output "$destination/result-bundle-v2.schema.json"
target/debug/owg schema --kind simulation-result --output "$destination/simulation-result-v1.schema.json"
target/debug/owg schema --kind checkpoint --output "$destination/checkpoint-v2.schema.json"
