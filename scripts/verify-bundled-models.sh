#!/usr/bin/env bash
set -euo pipefail
# verify-bundled-models.sh
# Asserts that enhanced diarization models are bundled for release builds.

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
MODELS_DIR="$SCRIPT_DIR/../frontend/src-tauri/models"
SEG="$MODELS_DIR/segmentation-3.0.onnx"
EMB="$MODELS_DIR/titanet_large.onnx"

assert_model() {
  local path="$1"
  local label="$2"
  if [ ! -f "$path" ]; then
    echo "Missing $label at $path. Diarization will fail at runtime (no bundled models). Rebuild with network." >&2
    exit 1
  fi
  local size
  size=$(wc -c < "$path" | tr -d ' ')
  if [ "$size" -le 1024 ]; then
    echo "$label at $path is too small ($size bytes). Expected >1KB." >&2
    exit 1
  fi
  echo "✅ $label found: $path ($size bytes)"
}

assert_model "$SEG" "segmentation-3.0.onnx"
assert_model "$EMB" "titanet_large.onnx"

# Optional: check bundle resources if present
if find target -name "segmentation-3.0.onnx" -type f 2>/dev/null | grep -q .; then
  echo "✅ Bundled resource check: found segmentation-3.0.onnx in target"
fi

echo "✅ All enhanced models verified for bundling"
