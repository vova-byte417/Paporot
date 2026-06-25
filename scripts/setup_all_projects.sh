#!/bin/bash
set -e
export PATH="$HOME/.cargo/bin:$PATH"

WASM=/mnt/d/trae_projects/Paporot/crates/paporot-core/target/wasm32-wasip1/release/paporot-core.wasm
SKILLS=/mnt/d/trae_projects/Paporot/.Paporot/skills
PAPOROT_BIN=/mnt/d/trae_projects/Paporot/target/release/paporot
API_KEY="sk-753abdb2391b4f598a12471edd00ff37"
MODEL="deepseek-v4-pro"

PROJECTS=(
  "/mnt/d/AI/kite"
  "/mnt/d/trae_projects/test_space/devika"
  "/mnt/d/trae_projects/test_space/e-commerce"
  "/mnt/d/trae_projects/goose"
)

setup_project() {
  local PROJ="$1"
  local PD="$PROJ/.Paporot"
  echo ""
  echo "=========================================="
  echo " Setting up: $PROJ"
  echo "=========================================="

  # Create dirs
  mkdir -p "$PD/snapshots" "$PD/reviews" "$PD/rules" "$PD/work" "$PD/reports" "$PD/skills" "$PD/bin"

  # Copy wasm
  cp "$WASM" "$PD/bin/paporot-core.wasm"

  # Copy skills (with content)
  rm -rf "$PD/skills"
  cp -r "$SKILLS" "$PD/skills"

  # Create config.toml
  cat > "$PD/config.toml" << CONFIGEOF
[llm]
endpoint = "https://api.deepseek.com/v1/chat/completions"
api_key = "$API_KEY"
model = "$MODEL"
temperature = 0.3
max_tokens = 4096
timeout_secs = 120

[storage]
snapshots_dir = ".Paporot/snapshots"

[agent]
diff_warn_threshold = 32000
diff_truncate_threshold = 96000

[trace]
auto_redact = false
redact_auth_header = true
redact_api_keys = true
redact_env_values = false
CONFIGEOF

  echo "  Setup complete for $PROJ"
}

# Setup all projects
for proj in "${PROJECTS[@]}"; do
  setup_project "$proj"
done

echo ""
echo "=========================================="
echo " All 4 projects setup complete"
echo "=========================================="
