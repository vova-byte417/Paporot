//! Dashboard HTML 模板
//!
//! 生成自包含的单文件 HTML Dashboard，包含：
//! - Skill Pipeline DAG 可视化
//! - 执行状态卡片
//! - Mermaid 图表渲染（client-side mermaid.js CDN）
//! - 风险等级指示器

use super::generator::ConsolidatedReport;
use serde::Serialize;

/// 供 Dashboard HTML 渲染的 JSON 数据
#[derive(Debug, Serialize)]
pub struct DashboardData {
    pub project_name: String,
    pub analyzed_at: String,
    pub total_skills: usize,
    pub ok: usize,
    pub skipped: usize,
    pub failed: usize,
    pub duration_secs: f64,
    pub risk_level: String,
    pub dag_layers_json: String,
    pub skill_results_json: String,
    pub mermaid_deps: String,
    pub mermaid_flows: String,
}

/// 将 ConsolidatedReport 渲染为完整 HTML 页面
pub fn render_dashboard_html(report: &ConsolidatedReport) -> String {
    let dag_json = serde_json::to_string(&report.dag_layers).unwrap_or_else(|_| "[]".into());
    let results_json = serde_json::to_string(&report.skill_results).unwrap_or_else(|_| "[]".into());
    let mermaid_deps = report.mermaid_deps.clone().unwrap_or_default();
    let mermaid_flows = report.mermaid_flows.clone().unwrap_or_default();

    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>{project_name} — Architecture Analysis Dashboard</title>
<script src="https://cdn.jsdelivr.net/npm/mermaid@10/dist/mermaid.min.js"></script>
<style>
  :root {{
    --bg: #0d1117;
    --card-bg: #161b22;
    --border: #30363d;
    --text: #c9d1d9;
    --text-dim: #8b949e;
    --accent: #58a6ff;
    --ok: #3fb950;
    --warn: #d29922;
    --fail: #f85149;
    --skip: #8b949e;
    --layer-0: #1f6feb;
    --layer-1: #8957e5;
    --layer-2: #db6d28;
    --layer-3: #238636;
    --layer-4: #da3633;
    --font: -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif;
  }}
  * {{ margin:0; padding:0; box-sizing:border-box; }}
  body {{ font-family: var(--font); background: var(--bg); color: var(--text); min-height: 100vh; }}
  .header {{ background: linear-gradient(135deg, #1a2332 0%, #0d1117 100%); border-bottom: 1px solid var(--border); padding: 24px 32px; }}
  .header h1 {{ font-size: 24px; font-weight: 600; color: #f0f6fc; }}
  .header .sub {{ color: var(--text-dim); font-size: 13px; margin-top: 4px; }}
  .container {{ max-width: 1280px; margin: 0 auto; padding: 24px 32px; }}
  .grid {{ display: grid; grid-template-columns: 2fr 1fr; gap: 24px; margin-bottom: 24px; }}
  .card {{ background: var(--card-bg); border: 1px solid var(--border); border-radius: 8px; padding: 20px; }}
  .card h3 {{ font-size: 14px; font-weight: 600; color: var(--text-dim); text-transform: uppercase; letter-spacing: 0.5px; margin-bottom: 16px; }}
  .meters {{ display: grid; grid-template-columns: repeat(4, 1fr); gap: 16px; margin-bottom: 24px; }}
  .meter {{ background: var(--card-bg); border: 1px solid var(--border); border-radius: 8px; padding: 16px; text-align: center; }}
  .meter .num {{ font-size: 36px; font-weight: 700; }}
  .meter .label {{ font-size: 12px; color: var(--text-dim); text-transform: uppercase; letter-spacing: 0.5px; }}
  .meter.ok .num {{ color: var(--ok); }}
  .meter.skip .num {{ color: var(--skip); }}
  .meter.fail .num {{ color: var(--fail); }}
  .meter.dur .num {{ color: var(--accent); font-size: 24px; }}

  /* DAG visualization */
  .dag {{ display: flex; flex-direction: column; gap: 12px; }}
  .dag-layer {{ display: flex; align-items: center; gap: 16px; }}
  .dag-label {{ width: 80px; font-size: 11px; color: var(--text-dim); text-align: right; }}
  .dag-nodes {{ display: flex; gap: 12px; flex-wrap: wrap; flex: 1; }}
  .dag-node {{ padding: 10px 18px; border-radius: 6px; font-size: 13px; font-weight: 500; color: #fff; position: relative; transition: transform 0.2s; cursor: default; }}
  .dag-node:hover {{ transform: scale(1.05); }}
  .dag-node.ok {{ background: var(--ok); }}
  .dag-node.skipped {{ background: var(--skip); }}
  .dag-node.failed {{ background: var(--fail); }}
  .dag-arrow {{ display: flex; justify-content: center; padding: 4px 0; }}
  .dag-arrow svg {{ width: 24px; height: 24px; fill: var(--border); }}

  /* Risk indicator */
  .risk {{ display: inline-flex; align-items: center; gap: 8px; padding: 6px 14px; border-radius: 20px; font-size: 12px; font-weight: 600; text-transform: uppercase; }}
  .risk.low {{ background: rgba(63,185,80,0.15); color: var(--ok); }}
  .risk.medium {{ background: rgba(210,153,34,0.15); color: var(--warn); }}
  .risk.high {{ background: rgba(248,81,73,0.15); color: var(--fail); }}

  /* Progress bar */
  .progress-bar {{ height: 8px; border-radius: 4px; background: var(--border); margin-bottom: 24px; overflow: hidden; display: flex; }}
  .progress-bar .ok-seg {{ background: var(--ok); height: 100%; }}
  .progress-bar .skip-seg {{ background: var(--skip); height: 100%; }}
  .progress-bar .fail-seg {{ background: var(--fail); height: 100%; }}

  /* Mermaid */
  .mermaid-container {{ background: #fff; border-radius: 6px; padding: 16px; overflow-x: auto; margin-top: 16px; }}
  .mermaid-container .mermaid {{ text-align: center; }}

  /* Behavior changes table */
  .changelog-table {{ width: 100%; border-collapse: collapse; font-size: 13px; }}
  .changelog-table th {{ text-align: left; padding: 8px 12px; border-bottom: 1px solid var(--border); color: var(--text-dim); font-weight: 600; }}
  .changelog-table td {{ padding: 8px 12px; border-bottom: 1px solid var(--border); }}

  .beh {{ color: var(--ok); font-weight: 600; }}
  .non-beh {{ color: var(--text-dim); }}

  /* Responsive */
  @media (max-width: 768px) {{
    .grid {{ grid-template-columns: 1fr; }}
    .meters {{ grid-template-columns: repeat(2, 1fr); }}
  }}
</style>
</head>
<body>

<div class="header">
  <h1>{project_name}</h1>
  <div class="sub">Architecture Analysis Dashboard — {analyzed_at}</div>
</div>

<div class="container">

  <!-- Progress Bar -->
  <div class="progress-bar" id="progressBar"></div>

  <!-- Summary Meters -->
  <div class="meters">
    <div class="meter ok"><div class="num" id="okCount">0</div><div class="label">OK</div></div>
    <div class="meter skip"><div class="num" id="skipCount">0</div><div class="label">Skipped</div></div>
    <div class="meter fail"><div class="num" id="failCount">0</div><div class="label">Failed</div></div>
    <div class="meter dur"><div class="num" id="durValue">0s</div><div class="label">Duration</div></div>
  </div>

  <div class="grid">
    <!-- DAG Execution Plan -->
    <div class="card">
      <h3>Execution Plan (DAG) &nbsp; <span class="risk" id="riskBadge">LOW</span></h3>
      <div class="dag" id="dagContainer"></div>
    </div>

    <!-- Skill Status Table -->
    <div class="card">
      <h3>Skill Results</h3>
      <table class="changelog-table" id="skillTable">
        <thead><tr><th>Skill</th><th>Status</th><th>Duration</th><th>Output</th></tr></thead>
        <tbody></tbody>
      </table>
    </div>
  </div>

  <!-- Mermaid: Dependency Graph -->
  <div class="card" id="depCard" style="display:none">
    <h3>Dependency Graph</h3>
    <div class="mermaid-container"><div class="mermaid" id="depMermaid"></div></div>
  </div>

  <!-- Mermaid: Runtime Flows -->
  <div class="card" id="flowCard" style="display:none">
    <h3>Runtime Flows</h3>
    <div class="mermaid-container"><div class="mermaid" id="flowMermaid"></div></div>
  </div>

</div>

<script>
// ─── Inline Data ──────────────────────────────────────────
var DAG_LAYERS = {dag_json};
var SKILL_RESULTS = {results_json};
var MERMAID_DEPS = {mermaid_deps:?};
var MERMAID_FLOWS = {mermaid_flows:?};
var TOTAL_SKILLS = {total};
var OK = {ok};
var SKIPPED = {skipped};
var FAILED = {failed};
var DURATION = {duration};

// ─── Mermaid Init ────────────────────────────────────────
mermaid.initialize({{ startOnLoad: false, theme: 'neutral', securityLevel: 'loose' }});

// ─── Render Progress Bar ─────────────────────────────────
(function() {{
  var total = OK + SKIPPED + FAILED || 1;
  var okPct = (OK / total * 100).toFixed(1);
  var skipPct = (SKIPPED / total * 100).toFixed(1);
  var failPct = (FAILED / total * 100).toFixed(1);
  document.getElementById('progressBar').innerHTML =
    '<div class="ok-seg" style="width:' + okPct + '%"></div>' +
    '<div class="skip-seg" style="width:' + skipPct + '%"></div>' +
    '<div class="fail-seg" style="width:' + failPct + '%"></div>';
}})();

// ─── Render Meters ───────────────────────────────────────
document.getElementById('okCount').textContent = OK;
document.getElementById('skipCount').textContent = SKIPPED;
document.getElementById('failCount').textContent = FAILED;
document.getElementById('durValue').textContent = DURATION.toFixed(1) + 's';

// ─── Risk Badge ─────────────────────────────────────────
var riskLevel = FAILED > 0 ? 'HIGH' : SKIPPED > 0 ? 'MEDIUM' : 'LOW';
var badge = document.getElementById('riskBadge');
badge.textContent = riskLevel;
badge.className = 'risk ' + riskLevel.toLowerCase();

// ─── Render DAG ─────────────────────────────────────────
(function() {{
  var container = document.getElementById('dagContainer');
  var layerColors = ['var(--layer-0)', 'var(--layer-1)', 'var(--layer-2)', 'var(--layer-3)', 'var(--layer-4)'];

  DAG_LAYERS.forEach(function(layer, li) {{
    // Arrow before layer (except first)
    if (li > 0) {{
      var arrow = document.createElement('div');
      arrow.className = 'dag-arrow';
      arrow.innerHTML = '<svg viewBox="0 0 24 24"><path d="M12 4l-1.41 1.41L16.17 11H4v2h12.17l-5.58 5.59L12 20l8-8z" transform="rotate(90 12 12)"/></svg>';
      container.appendChild(arrow);
    }}

    var layerDiv = document.createElement('div');
    layerDiv.className = 'dag-layer';

    var label = document.createElement('div');
    label.className = 'dag-label';
    label.textContent = 'Layer ' + (li + 1) + (layer.parallel ? ' ∥' : '');
    layerDiv.appendChild(label);

    var nodes = document.createElement('div');
    nodes.className = 'dag-nodes';

    layer.skills.forEach(function(skillName) {{
      var node = document.createElement('div');
      node.className = 'dag-node';
      // Find matching result
      var result = SKILL_RESULTS.find(function(r) {{ return r.name === skillName; }});
      if (result) {{
        node.classList.add(result.status);
        node.textContent = (result.status === 'ok' ? '\u2713 ' : result.status === 'failed' ? '\u2717 ' : '\u2192 ') + skillName;
        node.title = result.status + ' (' + result.duration_ms + 'ms)';
      }} else {{
        node.classList.add('skipped');
        node.textContent = skillName;
      }}
      nodes.appendChild(node);
    }});

    layerDiv.appendChild(nodes);
    container.appendChild(layerDiv);
  }});
}})();

// ─── Render Skill Table ─────────────────────────────────
(function() {{
  var tbody = document.querySelector('#skillTable tbody');
  SKILL_RESULTS.forEach(function(r) {{
    var tr = document.createElement('tr');
    tr.innerHTML =
      '<td>' + r.name + '</td>' +
      '<td class="' + (r.status === 'ok' ? 'beh' : r.status === 'failed' ? 'fail-color' : 'non-beh') + '">' + r.status + '</td>' +
      '<td>' + r.duration_ms + 'ms</td>' +
      '<td style="max-width:300px;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;" title="' + (r.output_summary || '') + '">' + (r.output_summary || '\u2014') + '</td>';
    tbody.appendChild(tr);
  }});
}})();

// ─── Render Mermaid ─────────────────────────────────────
(function() {{
  if (MERMAID_DEPS && MERMAID_DEPS.length > 0) {{
    document.getElementById('depCard').style.display = 'block';
    mermaid.render('depGraph', MERMAID_DEPS).then(function(result) {{
      document.getElementById('depMermaid').innerHTML = result.svg;
    }});
  }}
  if (MERMAID_FLOWS && MERMAID_FLOWS.length > 0) {{
    document.getElementById('flowCard').style.display = 'block';
    mermaid.render('flowGraph', MERMAID_FLOWS).then(function(result) {{
      document.getElementById('flowMermaid').innerHTML = result.svg;
    }});
  }}
}})();
</script>
</body>
</html>"#,
        project_name = report.project_name,
        analyzed_at = report.analyzed_at,
        dag_json = dag_json,
        results_json = results_json,
        mermaid_deps = serde_json::to_string(&mermaid_deps).unwrap_or_else(|_| "null".into()),
        mermaid_flows = serde_json::to_string(&mermaid_flows).unwrap_or_else(|_| "null".into()),
        total = report.summary.total_skills,
        ok = report.summary.ok,
        skipped = report.summary.skipped,
        failed = report.summary.failed,
        duration = report.summary.duration_secs,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::generator::*;

    #[test]
    fn test_dashboard_html_generation() {
        let report = ConsolidatedReport {
            project_name: "TestProject".into(),
            analyzed_at: "2026-01-01 12:00:00".into(),
            summary: AnalysisReportSummary {
                total_skills: 3, ok: 2, skipped: 0, failed: 1,
                duration_secs: 2.5, risk_level: "high".into(),
            },
            skill_results: vec![
                SkillReportItem {
                    name: "repo-understanding".into(),
                    status: "ok".into(),
                    duration_ms: 100,
                    output_summary: Some(r#"{"project_name":"test"}"#.into()),
                    error: None,
                },
                SkillReportItem {
                    name: "module-discovery".into(),
                    status: "ok".into(),
                    duration_ms: 200,
                    output_summary: Some(r#"{"modules":[]}"#.into()),
                    error: None,
                },
                SkillReportItem {
                    name: "dep-analysis".into(),
                    status: "failed".into(),
                    duration_ms: 50,
                    output_summary: None,
                    error: Some("missing input".into()),
                },
            ],
            dag_layers: vec![
                DagLayerDesc { layer_index: 0, skills: vec!["repo-understanding".into()], parallel: false },
                DagLayerDesc { layer_index: 1, skills: vec!["module-discovery".into(), "dep-analysis".into()], parallel: true },
            ],
            mermaid_deps: Some("graph TD\n  A --> B".into()),
            mermaid_flows: Some("flowchart TD\n  Start --> End".into()),
        };

        let html = render_dashboard_html(&report);
        assert!(html.contains("TestProject"));
        assert!(html.contains("repo-understanding"));
        assert!(html.contains("graph TD"));
        assert!(html.contains("flowchart TD"));
    }
}
