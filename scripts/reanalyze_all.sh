#!/bin/bash
set -e
export PAPOROT_API_KEY="sk-753abdb2391b4f598a12471edd00ff37"
export PATH="$HOME/.cargo/bin:$PATH"

WASM=/mnt/d/trae_projects/Paporot/crates/paporot-core/target/wasm32-wasip1/release/paporot-core.wasm
BIN=/mnt/d/trae_projects/Paporot/target/release/paporot
RESULTS=/mnt/d/trae_projects/Paporot/scripts/analysis_results

PROJECTS=(
  "/mnt/d/AI/kite:kiTE"
  "/mnt/d/trae_projects/test_space/devika:Devika"
  "/mnt/d/trae_projects/test_space/e-commerce:eCommerce"
  "/mnt/d/trae_projects/goose:Goose"
)

echo "=== Copying new WASM to all projects ==="
for entry in "${PROJECTS[@]}"; do
  PROJ="${entry%%:*}"
  NAME="${entry##*:}"
  cp "$WASM" "$PROJ/.Paporot/bin/paporot-core.wasm"
  echo "  $NAME: WASM copied"
done

echo ""
echo "=== Running analysis ==="
for entry in "${PROJECTS[@]}"; do
  PROJ="${entry%%:*}"
  NAME="${entry##*:}"
  LOG="$RESULTS/${NAME}_v2.log"
  
  echo ""
  echo "===== $NAME ====="
  cd "$PROJ"
  
  if timeout 240 "$BIN" analyze > "$LOG" 2>&1; then
    echo "  $NAME: OK"
    grep -E "Analysis Complete|Preprocessor:|Total|OK|Skip|Fail" "$LOG"
  else
    echo "  $NAME: FAILED (exit=$?)"
    tail -5 "$LOG"
  fi
done

echo ""
echo "=== All done ==="
ls -la "$RESULTS/"*_v2.log 2>/dev/null
