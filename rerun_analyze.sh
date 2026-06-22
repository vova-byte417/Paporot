#!/bin/bash
# Re-run Paporot analyze with real LLM for all projects
source ~/.cargo/env

PAPOROT_BIN=/mnt/d/ai/trae_projects/Paporot/target/release/paporot

analyze_one() {
    local proj="$1" name="$2"
    local pd="${proj}/.Paporot"
    
    echo ""
    echo "========================================" 
    echo "  ANALYZE: $name"
    echo "========================================"
    
    # Clean old stubs
    rm -f "${pd}/reports/analysis_result.json" "${pd}/reports/dashboard.html" "${pd}/reports/architecture.md"
    
    # Ensure wasm exists
    mkdir -p "${pd}/bin"
    if [ ! -f "${pd}/bin/paporot-core.wasm" ]; then
        cp /mnt/d/ai/trae_projects/Paporot/.Paporot/bin/paporot-core.wasm "${pd}/bin/"
    fi
    
    # Ensure skills exist
    mkdir -p "${pd}/skills"
    if [ -z "$(ls -A "${pd}/skills" 2>/dev/null)" ]; then
        cp -r /mnt/d/ai/trae_projects/Paporot/.Paporot/skills/* "${pd}/skills/"
    fi
    
    cd "$proj"
    "$PAPOROT_BIN" analyze 2>&1
    local exit_code=$?
    
    # Verify
    echo ""
    local json="${pd}/reports/analysis_result.json"
    local html="${pd}/reports/dashboard.html"
    if [ -f "$json" ]; then
        local sz=$(stat -c%s "$json")
        local has_llm=$(grep -c '"LLM unavailable"\|"stub"' "$json" 2>/dev/null || echo 0)
        if [ "$has_llm" -gt 0 ]; then
            echo "  [WARN] Still has stub data!"
        else
            echo "  [OK] analysis_result.json ($sz bytes, real data)"
        fi
    else
        echo "  [FAIL] analysis_result.json MISSING"
    fi
    if [ -f "$html" ]; then
        echo "  [OK] dashboard.html ($(stat -c%s "$html") bytes)"
    fi
    
    return $exit_code
}

# Run each project
analyze_one /mnt/d/ai/trae_projects/Paporot "Paporot Self"
analyze_one /mnt/d/ai/trae_projects/agent-test-proj "agent-test-proj"
analyze_one /mnt/d/ai/trae_projects/oss/fd "fd"
analyze_one /mnt/d/ai/trae_projects/oss/ripgrep "ripgrep"
analyze_one /mnt/d/ai/trae_projects/oss/rsmark "rsmark"
analyze_one /mnt/d/ai/trae_projects/oss/rsdiff "rsdiff"
analyze_one /mnt/d/ai/trae_projects/oss/rslog "rslog"

echo ""
echo "========================================"
echo "  FINAL VERIFICATION"
echo "========================================"
for proj in \
    /mnt/d/ai/trae_projects/Paporot \
    /mnt/d/ai/trae_projects/agent-test-proj \
    /mnt/d/ai/trae_projects/oss/fd \
    /mnt/d/ai/trae_projects/oss/ripgrep \
    /mnt/d/ai/trae_projects/oss/rsmark \
    /mnt/d/ai/trae_projects/oss/rsdiff \
    /mnt/d/ai/trae_projects/oss/rslog
do
    name=$(basename "$proj")
    json="${proj}/.Paporot/reports/analysis_result.json"
    html="${proj}/.Paporot/reports/dashboard.html"
    if [ -f "$json" ]; then
        sz=$(stat -c%s "$json")
        # Check if it contains real analysis data, not just stub
        has_modules=$(grep -c '"modules"' "$json" 2>/dev/null || echo 0)
        has_cap=$(grep -c '"capability\|"behavior"' "$json" 2>/dev/null || echo 0)
        echo "  $name : $sz B (modules=$has_modules, cap=$has_cap)"
    else
        echo "  $name : MISSING"
    fi
done
