//! 报告生成
//!
//! JSON 汇总 + HTML 单文件报告（含图表和数据）。

use crate::types::{SuiteResult, Verdict};
use chrono::Local;

/// 生成 JSON 汇总
pub fn json_summary(results: &[SuiteResult]) -> String {
    serde_json::to_string_pretty(results).unwrap_or_else(|e| format!("{{'error': '{}'}}", e))
}

/// 生成自包含 HTML 报告
pub fn html_report(title: &str, results: &[SuiteResult]) -> String {
    let now = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let total_pass: usize = results.iter().map(|r| r.pass + r.semantic_pass).sum();
    let total_fail: usize = results.iter().map(|r| r.fail).sum();
    let total_cases: usize = results.iter().map(|r| r.total).sum();
    let overall_rate = if total_cases > 0 {
        total_pass as f64 / total_cases as f64 * 100.0
    } else {
        100.0
    };

    let pie_data = format!(
        "[{pass}, {semantic}, {fail}]",
        pass = results.iter().map(|r| r.pass).sum::<usize>(),
        semantic = results.iter().map(|r| r.semantic_pass).sum::<usize>(),
        fail = total_fail,
    );

    let suite_rows: String = results.iter().map(|sr| {
        format!(
            r#"<tr><td>{}</td><td>{}</td><td>{}</td><td class="sp">{} SP</td><td class="fail-num">{}</td><td>{:.1}%</td><td>{}ms</td></tr>"#,
            sr.suite_name,
            sr.total,
            sr.pass,
            sr.semantic_pass,
            sr.fail,
            sr.pass_rate,
            sr.duration_ms,
        )
    }).collect::<Vec<_>>().join("\n");

    let case_rows: String = results.iter().flat_map(|sr| {
        sr.cases.iter().map(|c| {
            let (verdict_class, verdict_text) = match &c.verdict {
                Verdict::Pass => ("pass", "PASS".to_string()),
                Verdict::SemanticPass { confidence, .. } => ("semantic", format!("SEMANTIC ({:.0}%)", confidence * 100.0)),
                Verdict::Fail { reason } => ("fail", format!("FAIL - {}", reason)),
            };
            format!(
                r#"<tr class="{cls}"><td>{id}</td><td>{name}</td><td>{cat}</td><td>{v}</td><td>{exp}</td><td>{act}</td><td>{dur}ms</td></tr>"#,
                cls = verdict_class,
                id = c.case_id,
                name = c.name,
                cat = c.category,
                v = verdict_text,
                exp = c.expected_summary,
                act = c.actual_summary,
                dur = c.duration_ms,
            )
        }).collect::<Vec<_>>()
    }).collect::<Vec<_>>().join("\n");

    // Bar chart data per suite
    let bar_labels: String = results.iter().map(|r| format!(r#""{}""#, r.suite_name)).collect::<Vec<_>>().join(",");
    let bar_pass: String = results.iter().map(|r| r.pass.to_string()).collect::<Vec<_>>().join(",");
    let bar_sp: String = results.iter().map(|r| r.semantic_pass.to_string()).collect::<Vec<_>>().join(",");
    let bar_fail: String = results.iter().map(|r| r.fail.to_string()).collect::<Vec<_>>().join(",");

    format!(
        r#"<!DOCTYPE html>
<html lang="zh-CN">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>{title} — Paporot Benchmark Report</title>
<script src="https://cdn.jsdelivr.net/npm/chart.js@4"></script>
<style>
  :root {{
    --bg: #0d1117;
    --card-bg: #161b22;
    --border: #30363d;
    --text: #c9d1d9;
    --text-dim: #8b949e;
    --accent: #58a6ff;
    --pass: #3fb950;
    --warn: #d29922;
    --fail: #f85149;
    --semantic: #a371f7;
  }}
  * {{ margin:0; padding:0; box-sizing:border-box; }}
  body {{ background: var(--bg); color: var(--text); font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif; padding: 24px; }}
  h1 {{ font-size: 24px; margin-bottom: 4px; }}
  h2 {{ font-size: 18px; margin: 24px 0 12px; color: var(--accent); }}
  .meta {{ color: var(--text-dim); font-size: 13px; margin-bottom: 20px; }}
  .stats {{ display: flex; gap: 16px; margin-bottom: 24px; flex-wrap: wrap; }}
  .stat-card {{ background: var(--card-bg); border: 1px solid var(--border); border-radius: 8px; padding: 16px 24px; min-width: 120px; text-align: center; }}
  .stat-card .value {{ font-size: 28px; font-weight: 700; }}
  .stat-card .label {{ font-size: 12px; color: var(--text-dim); margin-top: 4px; }}
  .stat-card.pass .value {{ color: var(--pass); }}
  .stat-card.semantic .value {{ color: var(--semantic); }}
  .stat-card.fail .value {{ color: var(--fail); }}
  .stat-card.rate .value {{ color: var(--accent); }}
  .chart-row {{ display: flex; gap: 24px; margin-bottom: 24px; flex-wrap: wrap; }}
  .chart-box {{ background: var(--card-bg); border: 1px solid var(--border); border-radius: 8px; padding: 16px; flex: 1; min-width: 300px; }}
  .chart-box canvas {{ max-height: 280px; }}
  table {{ width: 100%; border-collapse: collapse; margin-bottom: 24px; font-size: 13px; }}
  th {{ text-align: left; padding: 8px 12px; border-bottom: 2px solid var(--border); color: var(--text-dim); font-weight: 600; }}
  td {{ padding: 8px 12px; border-bottom: 1px solid var(--border); }}
  tr:hover {{ background: rgba(255,255,255,0.03); }}
  .pass {{ color: var(--pass); }}
  .semantic {{ color: var(--semantic); }}
  .fail {{ color: var(--fail); }}
  .fail-num {{ color: var(--fail); }}
  .sp {{ color: var(--semantic); }}
  .summary {{ background: var(--card-bg); border: 1px solid var(--border); border-radius: 8px; padding: 16px; margin-bottom: 24px; }}
  .summary p {{ margin-bottom: 8px; line-height: 1.6; }}
  .conclusion {{ background: var(--card-bg); border: 1px solid var(--border); border-radius: 8px; padding: 16px; }}
  .conclusion h3 {{ margin-bottom: 8px; }}
</style>
</head>
<body>
<h1>Paporot Benchmark Report</h1>
<div class="meta">{title} &middot; {date}</div>

<div class="stats">
  <div class="stat-card pass"><div class="value">{total_cases}</div><div class="label">Total Cases</div></div>
  <div class="stat-card pass"><div class="value">{total_pass}</div><div class="label">Pass + Semantic</div></div>
  <div class="stat-card fail"><div class="value">{total_fail}</div><div class="label">Failed</div></div>
  <div class="stat-card rate"><div class="value">{overall_rate:.1}%</div><div class="label">Pass Rate</div></div>
</div>

<div class="chart-row">
  <div class="chart-box">
    <h2>Verdict Distribution</h2>
    <canvas id="pieChart"></canvas>
  </div>
  <div class="chart-box">
    <h2>Per Suite Breakdown</h2>
    <canvas id="barChart"></canvas>
  </div>
</div>

<h2>Suite Summary</h2>
<table>
  <thead><tr><th>Suite</th><th>Total</th><th>Pass</th><th>Semantic</th><th>Fail</th><th>Rate</th><th>Duration</th></tr></thead>
  <tbody>{suite_rows}</tbody>
</table>

<h2>Case Details</h2>
<table>
  <thead><tr><th>ID</th><th>Name</th><th>Category</th><th>Verdict</th><th>Expected</th><th>Actual</th><th>Duration</th></tr></thead>
  <tbody>{case_rows}</tbody>
</table>

<div class="conclusion">
  <h3>Analysis Conclusion</h3>
  <p>Benchmark run completed on {date}.</p>
  <p>Total: {total_cases} cases across {suite_count} suites.</p>
  <p>Pass rate: {overall_rate:.1}% ({total_pass}/{total_cases} passing, {total_fail} failed).</p>
  <p style="margin-top:8px;color:var(--text-dim)">Categories verified: capability extraction (name/status/categories), diff summary (counts), regression detection.</p>
  <p style="color:var(--text-dim)">Evaluator: Exact match (name, status, categories) → Semantic fallback (token Jaccard &gt; 0.85).</p>
</div>

<script>
new Chart(document.getElementById('pieChart'), {{
  type: 'doughnut',
  data: {{
    labels: ['Exact Pass', 'Semantic Pass', 'Failed'],
    datasets: [{{ data: {pie_data}, backgroundColor: ['#3fb950','#a371f7','#f85149'], borderColor: '#0d1117' }}]
  }},
  options: {{ responsive: true, plugins: {{ legend: {{ labels: {{ color: '#c9d1d9' }} }} }} }}
}});
new Chart(document.getElementById('barChart'), {{
  type: 'bar',
  data: {{
    labels: [{bar_labels}],
    datasets: [
      {{ label:'Pass', data:[{bar_pass}], backgroundColor:'#3fb950' }},
      {{ label:'Semantic', data:[{bar_sp}], backgroundColor:'#a371f7' }},
      {{ label:'Fail', data:[{bar_fail}], backgroundColor:'#f85149' }}
    ]
  }},
  options: {{
    responsive: true,
    scales: {{ x:{{ stacked:true, ticks:{{ color:'#8b949e' }} }}, y:{{ stacked:true, ticks:{{ color:'#8b949e' }} }} }},
    plugins: {{ legend: {{ labels: {{ color:'#c9d1d9' }} }} }}
  }}
}});
</script>
</body></html>"#,
        title = title,
        date = now,
        total_cases = total_cases,
        total_pass = total_pass,
        total_fail = total_fail,
        overall_rate = overall_rate,
        suite_rows = suite_rows,
        case_rows = case_rows,
        pie_data = pie_data,
        bar_labels = bar_labels,
        bar_pass = bar_pass,
        bar_sp = bar_sp,
        bar_fail = bar_fail,
        suite_count = results.len(),
    )
}
