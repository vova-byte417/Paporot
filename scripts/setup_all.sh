#!/bin/bash
set -e
export PAPOROT_API_KEY="sk-753abdb2391b4f598a12471edd00ff37"
export PATH="$HOME/.cargo/bin:$PATH"

WASM=/mnt/d/trae_projects/Paporot/crates/paporot-core/target/wasm32-wasip1/release/paporot-core.wasm
PAPOROT_BIN=/mnt/d/trae_projects/Paporot/target/release/paporot

PROJECTS=(
  "/mnt/d/AI/kite:kiTE"
  "/mnt/d/trae_projects/test_space/devika:Devika"
  "/mnt/d/trae_projects/test_space/e-commerce:e-Commerce"
  "/mnt/d/trae_projects/goose:Goose"
)

# ── Write a skill.toml ──
write_skill_toml() {
  local dir="$1"
  local name="$2"
  local requires="$3"
  local desc="$4"
  local deps="$5"
  mkdir -p "$dir"
  touch "$dir/skill.wasm"
  cat > "$dir/skill.toml" << EOF
[skill]
name = "$name"
version = "0.1.0"
requires_paporot = ">=0.2.0"
description = "$desc"
timeout_secs = 60

[inputs]
required = ["$requires"]
optional = []

[outputs]
schema = "${name}_output"
format = "json"

[dependencies]
$deps
EOF
}

setup_project() {
  local PROJ="$1"
  local NAME="$2"
  local PD="$PROJ/.Paporot"
  echo ""
  echo "===== $NAME ($PROJ) ====="

  mkdir -p "$PD/snapshots" "$PD/reviews" "$PD/rules" "$PD/work" "$PD/reports" "$PD/skills" "$PD/bin"
  cp "$WASM" "$PD/bin/paporot-core.wasm"

  # Write skills
  write_skill_toml "$PD/skills/repository-understanding" "repository-understanding" "sources" "Understand repository structure" ""
  write_skill_toml "$PD/skills/module-discovery" "module-discovery" "repository_summary" "Discover code modules" "uses_outputs_from = [\"repository_summary\"]"
  write_skill_toml "$PD/skills/dependency-analysis" "dependency-analysis" "modules" "Analyze dependencies" "uses_outputs_from = [\"modules\"]"
  write_skill_toml "$PD/skills/runtime-flow-analysis" "runtime-flow-analysis" "dependency_graph" "Analyze runtime flows" "uses_outputs_from = [\"dependency_graph\"]"
  write_skill_toml "$PD/skills/architecture-doc-generator" "architecture-doc-generator" "dependency_graph" "Generate architecture docs" "uses_outputs_from = [\"dependency_graph\", \"runtime_flows\"]"
  write_skill_toml "$PD/skills/verification-runner" "verification-runner" "architecture_doc" "Run verification" "uses_outputs_from = [\"architecture_doc\", \"coverage_report\"]"
  write_skill_toml "$PD/skills/prd-coverage" "prd-coverage" "modules" "Check PRD coverage" "uses_outputs_from = [\"modules\"]"

  # Create config.toml
  cat > "$PD/config.toml" << 'CONFIGEOF'
[llm]
endpoint = "https://api.deepseek.com/v1/chat/completions"
api_key = "sk-753abdb2391b4f598a12471edd00ff37"
model = "deepseek-v4-pro"
temperature = 0.3
max_tokens = 4096
timeout_secs = 120

[storage]
snapshots_dir = ".Paporot/snapshots"
CONFIGEOF

  echo "  Setup OK: $NAME"
}

# ── Setup all ──
for entry in "${PROJECTS[@]}"; do
  PROJ="${entry%%:*}"
  NAME="${entry##*:}"
  setup_project "$PROJ" "$NAME"
done

echo ""
echo "========== All 4 projects ready =========="
