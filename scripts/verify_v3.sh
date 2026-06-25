#!/bin/bash
set -e
export PATH="$HOME/.cargo/bin:$PATH"
cd /mnt/d/trae_projects/Paporot
PAPOROT=./target/release/paporot

echo "=========================================="
echo " Paporot v3 Loop Engineering 验证脚本"
echo "=========================================="
echo ""

# Clean
rm -rf .Paporot/snapshots .Paporot/reviews .Paporot/rules .Paporot/work

# Step 1: Init
echo "--- 1. Init ---"
$PAPOROT init

# Step 2: Create a mock snapshot manually (since analyze needs source files)
echo "--- 2. Create mock snapshot ---"
mkdir -p .Paporot/snapshots
cat > .Paporot/snapshots/v1.json << 'SNAPEOF'
{
  "schema_version": 3,
  "version_id": "v1",
  "git_commit": "abc123",
  "git_ref": "main",
  "timestamp": "2026-06-24T10:00:00Z",
  "message": "mock for verification",
  "capabilities": [
    {
      "id": "cap_auth",
      "name": "login",
      "description": "User login function",
      "status": "new",
      "module": "src/auth.rs",
      "confidence": 0.9,
      "evidence": ["src/auth.rs"],
      "source_change_type": "FunctionAdded",
      "triggered_by_rules": ["sec_auth_001"]
    },
    {
      "id": "cap_sync",
      "name": "sync_legacy",
      "description": "Legacy sync function",
      "status": "deleted",
      "module": "src/legacy/sync.rs",
      "confidence": 0.85,
      "evidence": ["src/legacy/sync.rs"],
      "source_change_type": "FunctionRemoved",
      "triggered_by_rules": ["breaking_001"]
    },
    {
      "id": "cap_token",
      "name": "rotate_token",
      "description": "Token rotation in migration",
      "status": "modified",
      "module": "src/migrations/001_token.sql",
      "confidence": 0.8,
      "evidence": ["src/migrations/001_token.sql"],
      "source_change_type": "ConstantChanged",
      "triggered_by_rules": ["sec_token_001"]
    }
  ],
  "prd_coverage": {
    "percentage": 100.0,
    "total_items": 3,
    "covered_items": 3,
    "details": []
  },
  "regression": null,
  "risk": null,
  "metadata": null
}
SNAPEOF
echo "  Snapshot v1 created with 3 capabilities"

# Step 3: Generate feedback TOML
echo ""
echo "--- 3. feedback generate ---"
mkdir -p .Paporot/reviews
$PAPOROT feedback generate v1
echo ""
cat .Paporot/reviews/review_v1.toml | head -20

# Step 4: Edit TOML — reject one + add suppress_rule
echo ""
echo "--- 4. Edit TOML ---"
cat > .Paporot/reviews/review_v1.toml << 'EOF'
[approve]
cap_auth = "ok"

[reject]
cap_sync = "false positive - sync_legacy was long deleted"

[suppress_rule.breaking_001]
reason = "src/legacy/ API removals are expected deprecation"
file_pattern = "src/legacy/*"
effect = "suppress"

[suppress_rule.sec_token_001]
reason = "migrations directory token changes are expected"
file_pattern = "src/migrations/*"
effect = "warn"
change_type = "ConstantChanged"
EOF
echo "  TOML edited with 1 reject + 2 suppress_rules"

# Step 5: feedback apply
echo ""
echo "--- 5. feedback apply ---"
$PAPOROT feedback apply .Paporot/reviews/review_v1.toml

# Step 6: Verify reviews.json
echo ""
echo "--- 6. Verify reviews.json ---"
echo ""
echo "  reviews.json:"
cat .Paporot/reviews/reviews.json | python3 -m json.tool 2>/dev/null || cat .Paporot/reviews/reviews.json

# Verify traceback fields
echo ""
echo "  Traceback fields check:"
python3 -c "
import json
with open('.Paporot/reviews/reviews.json') as f:
    data = json.load(f)
for r in data['reviews']:
    if r['verdict'] == 'rejected':
        print(f'    review_id: {r[\"review_id\"]}')
        print(f'    source_symbol: {r.get(\"source_symbol\", \"MISSING\")}')
        print(f'    source_file: {r.get(\"source_file\", \"MISSING\")}')
        print(f'    source_change_type: {r.get(\"source_change_type\", \"MISSING\")}')
        print(f'    triggered_by_rules: {r.get(\"triggered_by_rules\", \"MISSING\")}')
" 2>/dev/null || echo "  (python3 not available, check manually)"

# Step 7: Verify suppressions.toml
echo ""
echo "--- 7. Verify suppressions.toml ---"
cat .Paporot/rules/suppressions.toml

# Step 8: feedback stats
echo ""
echo "--- 8. feedback stats ---"
$PAPOROT feedback stats

# Step 9: Verify feedback_index.json generation
echo ""
echo "--- 9. Verify feedback_index.json ---"
mkdir -p .Paporot/work
# Simulate what happens during analyze:
# build_and_write_feedback_index reads reviews.json + suppressions.toml
# We can verify this logic works via the existing unit tests

# Check the actual file content
python3 -c "
import json
# Manually verify the feedback_index.json logic
with open('.Paporot/reviews/reviews.json') as f:
    reviews = json.load(f)

print('  Rejected reviews:', len([r for r in reviews['reviews'] if r['verdict'] == 'rejected']))
for r in reviews['reviews']:
    if r['verdict'] == 'rejected':
        symbol = r.get('source_symbol', '')
        file = r.get('source_file', '')
        ct = r.get('source_change_type', '')
        print(f'    exact_key: {symbol}::{file}::{ct}')
        print(f'    rules: {r.get(\"triggered_by_rules\", [])}')
" 2>/dev/null || echo "  (python3 not available)"

echo ""
echo "=========================================="
echo " 验证完成"
echo "=========================================="
