#!/bin/bash
D=/mnt/d/trae_projects/Paporot/scripts/analysis_results
for f in kiTE Devika e-Commerce Goose; do
  echo "========== $f =========="
  wc -l "$D/$f.log"
  grep -E "Preprocessor:|Analysis Complete|Snapshot|Reports written" "$D/$f.log"
  grep -E "^  Total|^  OK|^  Skip|^  Fail" "$D/$f.log"
  echo ""
done
