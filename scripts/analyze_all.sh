#!/bin/bash
set -e
export PAPOROT_API_KEY="sk-753abdb2391b4f598a12471edd00ff37"
export PATH="$HOME/.cargo/bin:$PATH"

PAPOROT_BIN=/mnt/d/trae_projects/Paporot/target/release/paporot
RESULTS_DIR=/mnt/d/trae_projects/Paporot/scripts/analysis_results
mkdir -p "$RESULTS_DIR"

PROJECTS=(
  "/mnt/d/AI/kite:kiTE"
  "/mnt/d/trae_projects/test_space/devika:Devika"
  "/mnt/d/trae_projects/test_space/e-commerce:e-Commerce"
  "/mnt/d/trae_projects/goose:Goose"
)

echo "Starting analysis at $(date)"

for entry in "${PROJECTS[@]}"; do
  PROJ="${entry%%:*}"
  NAME="${entry##*:}"
  LOG="$RESULTS_DIR/${NAME}.log"
  
  echo ""
  echo "============================================"
  echo " Analyzing: $NAME ($PROJ)"
  echo "============================================"
  
  cd "$PROJ"
  
  # Run analyze, capture output
  if "$PAPOROT_BIN" analyze > "$LOG" 2>&1; then
    echo "  $NAME: OK"
  else
    echo "  $NAME: FAILED (exit=$?)"
  fi
  
  # Show key lines
  grep -E "(config|Found|DAG|Preprocessor|Feedback|Analysis Complete|Total|OK|Skip|Fail)" "$LOG" 2>/dev/null || true
  echo "  Log: $LOG"
done

echo ""
echo "========== All analyses complete =========="
echo "Results in: $RESULTS_DIR"
ls -la "$RESULTS_DIR/"
