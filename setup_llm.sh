#!/bin/bash
API_KEY='sk-ffa4ddf6a4004645b3bb385530660ba3'
MODEL='deepseek-v4-pro'

for proj in \
    /mnt/d/ai/trae_projects/Paporot \
    /mnt/d/ai/trae_projects/agent-test-proj \
    /mnt/d/ai/trae_projects/oss/fd \
    /mnt/d/ai/trae_projects/oss/ripgrep \
    /mnt/d/ai/trae_projects/oss/rsmark \
    /mnt/d/ai/trae_projects/oss/rsdiff \
    /mnt/d/ai/trae_projects/oss/rslog
do
    conf="${proj}/.Paporot/config.toml"
    cat > "$conf" << TOML
[llm]
api_key = "${API_KEY}"
endpoint = "https://api.deepseek.com/v1/chat/completions"
model = "${MODEL}"
temperature = 0.3
max_tokens = 4096
max_retries = 3
timeout_secs = 120
TOML
    echo "Updated: $conf"
done

# Update skill.toml preferred_model
for skill_toml in /mnt/d/ai/trae_projects/Paporot/.Paporot/skills/*/skill.toml; do
    sed -i 's/deepseek-pro/deepseek-v4-pro/g' "$skill_toml"
done
echo "Skills updated too"
