//! Paporot v0.4.0 Dashboard
//!
//! axum HTTP 服务器 + D3.js 前端，端口 9494。
//!
//! 设计目标：讲被分析项目的故事，而非展示 Paporot 自身状态。
//! 视觉风格：现代极简深色（Linear/Vercel 风格）

use std::path::PathBuf;
use std::sync::Arc;

use axum::{
    extract::{Path, State},
    response::{Html, Json},
    routing::get,
    Router,
};
use serde_json::{json, Value};
use tower_http::cors::CorsLayer;

use crate::eval::TaskManager;
use crate::storage::{cache::CacheManager, timeline::TimelineStore};

pub mod cluster;

// ─── AppState ──────────────────────────────────────────────────────

struct AppState {
    paporot_dir: PathBuf,
    cwd: PathBuf,
}

// ─── Server ────────────────────────────────────────────────────────

pub async fn serve(paporot_dir: PathBuf, cwd: PathBuf, port: u16) -> anyhow::Result<()> {
    let state = Arc::new(AppState { paporot_dir, cwd });

    let app = Router::new()
        .route("/api/status", get(api_status))
        .route("/api/tasks", get(api_tasks))
        .route("/api/eval/{task_id}", get(api_eval))
        .route("/api/capabilities", get(api_capabilities))
        .route("/api/narrative", get(api_narrative))
        .route("/api/trend/{task_id}", get(api_trend))
        .route("/", get(dashboard_page))
        .layer(CorsLayer::permissive())
        .with_state(state);

    let addr = format!("127.0.0.1:{}", port);
    println!("Dashboard 已启动 → http://{}", addr);
    println!("按 Ctrl+C 停止");

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

// ─── API Handlers ──────────────────────────────────────────────────

async fn api_status(State(state): State<Arc<AppState>>) -> Json<Value> {
    let tm = TaskManager::new(&state.paporot_dir).ok();
    let tasks = tm.as_ref().and_then(|t| t.list().ok()).unwrap_or_default();
    Json(json!({
        "version": env!("CARGO_PKG_VERSION"),
        "project": state.cwd.to_string_lossy(),
        "tasks_count": tasks.len(),
        "paporot_dir": state.paporot_dir.to_string_lossy(),
    }))
}

async fn api_tasks(State(state): State<Arc<AppState>>) -> Json<Value> {
    let tm = TaskManager::new(&state.paporot_dir);
    match tm {
        Ok(tm) => match tm.list() {
            Ok(tasks) => {
                let items: Vec<Value> = tasks.iter().map(|t| json!({
                    "id": t.id,
                    "description": t.description,
                    "category": t.category.to_string(),
                    "modules": t.modules,
                    "success_criteria": t.success_criteria,
                })).collect();
                Json(json!({"tasks": items}))
            }
            Err(e) => Json(json!({"error": format!("{}", e)})),
        },
        Err(e) => Json(json!({"error": format!("{}", e)})),
    }
}

async fn api_eval(
    State(state): State<Arc<AppState>>,
    Path(task_id): Path<String>,
) -> Json<Value> {
    let store = match TimelineStore::open(&state.paporot_dir) {
        Ok(s) => s,
        Err(e) => return Json(json!({"error": format!("{}", e)})),
    };
    match store.list_trials(&task_id) {
        Ok(trials) => {
            let evals: Vec<Value> = trials.iter().map(|e| json!({
                "eval_id": e.eval_id,
                "trial_index": e.trial_index,
                "outcome": e.outcome.label(),
                "grader_results": e.grader_results.iter().map(|g| json!({
                    "name": g.name, "passed": g.passed,
                })).collect::<Vec<_>>(),
                "tool_pattern": e.tool_pattern.as_ref().map(|tp| json!({
                    "total_tool_calls": tp.total_tool_calls,
                    "edit_ratio": tp.edit_ratio,
                    "read_ratio": tp.read_ratio,
                    "total_tokens": tp.total_tokens,
                    "duration_ms": tp.duration_ms,
                })),
                "code_change": {
                    "files": e.code_change.files_changed,
                    "additions": e.code_change.additions,
                    "deletions": e.code_change.deletions,
                    "modules": e.code_change.modules.clone(),
                    "symbols_added": e.code_change.symbols_added.iter().map(|s| json!({
                        "name": s.name, "kind": s.kind.to_string(), "file": s.file_path,
                    })).collect::<Vec<_>>(),
                    "symbols_removed": e.code_change.symbols_removed.iter().map(|s| json!({
                        "name": s.name, "kind": s.kind.to_string(), "file": s.file_path,
                    })).collect::<Vec<_>>(),
                },
                "created_at": e.created_at,
                "task": {"id": e.task.id, "description": e.task.description},
            })).collect();
            Json(json!({"task_id": task_id, "trials": evals}))
        }
        Err(e) => Json(json!({"error": format!("{}", e)})),
    }
}

async fn api_capabilities(
    State(state): State<Arc<AppState>>,
) -> Json<Value> {
    let cache = CacheManager::new(&state.paporot_dir);
    match cache.read_code_change() {
        Ok(Some(cc)) => {
            let nodes: Vec<Value> = cc.symbols_added.iter().map(|s| json!({
                "id": format!("{}::{}", s.file_path, s.name),
                "name": s.name, "kind": s.kind.to_string(),
                "file": s.file_path, "action": "added",
            })).chain(cc.symbols_removed.iter().map(|s| json!({
                "id": format!("{}::{}", s.file_path, s.name),
                "name": s.name, "kind": s.kind.to_string(),
                "file": s.file_path, "action": "removed",
            }))).collect();

            let links: Vec<Value> = {
                let mut l = Vec::new();
                let combined: Vec<_> = cc.symbols_added.iter()
                    .chain(cc.symbols_removed.iter()).collect();
                for (i, a) in combined.iter().enumerate() {
                    for (j, b) in combined.iter().enumerate() {
                        if i < j && a.file_path == b.file_path {
                            l.push(json!({
                                "source": format!("{}::{}", a.file_path, a.name),
                                "target": format!("{}::{}", b.file_path, b.name),
                                "value": 1,
                            }));
                        }
                    }
                }
                l
            };

            Json(json!({
                "modules": cc.modules,
                "nodes": nodes, "links": links,
                "additions": cc.additions, "deletions": cc.deletions,
            }))
        }
        Ok(None) => Json(json!({"nodes": [], "links": [], "note": "No code change cached yet"})),
        Err(e) => Json(json!({"error": format!("{}", e)})),
    }
}

async fn api_narrative(
    State(state): State<Arc<AppState>>,
) -> Json<Value> {
    let cache = CacheManager::new(&state.paporot_dir);
    match cache.read_json("narrative") {
        Ok(Some(v)) => Json(v),
        Ok(None) => {
            // Try reading impact data as fallback
            match cache.read_json("impact") {
                Ok(Some(impact)) => Json(json!({
                    "headline": "代码变更分析",
                    "subtitle": "无 LLM 解读 — 运行 paporot analyze 并配置 API Key",
                    "impact": impact,
                })),
                _ => Json(json!({
                    "headline": "暂无数据",
                    "subtitle": "运行 paporot analyze 以生成分析报告",
                })),
            }
        }
        Err(_) => Json(json!({"error": "Failed to read narrative"})),
    }
}

async fn api_trend(
    State(state): State<Arc<AppState>>,
    Path(task_id): Path<String>,
) -> Json<Value> {
    let store = match TimelineStore::open(&state.paporot_dir) {
        Ok(s) => s,
        Err(e) => return Json(json!({"error": format!("{}", e)})),
    };
    match crate::eval::trend::trend_history(&store, &task_id) {
        Ok(history) => {
            let points: Vec<Value> = history.trials.iter().map(|p| json!({
                "eval_id": p.eval_id, "trial_index": p.trial_index,
                "outcome": p.outcome.label(),
                "tool_calls": p.total_tool_calls,
                "tokens": p.total_tokens,
                "duration_ms": p.duration_ms,
                "created_at": p.created_at,
            })).collect();
            Json(json!({
                "task_id": history.task_id,
                "description": history.task_description,
                "trials": points,
            }))
        }
        Err(e) => Json(json!({"error": format!("{}", e)})),
    }
}

// ─── Dashboard Page ─────────────────────────────────────────────────

async fn dashboard_page() -> Html<String> {
    Html(DASHBOARD_HTML.to_string())
}

const DASHBOARD_HTML: &str = r##"<!DOCTYPE html>
<html lang="zh-CN">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>Paporot — AI Code Agent Behavioral Audit</title>
<link rel="preconnect" href="https://fonts.googleapis.com">
<link href="https://fonts.googleapis.com/css2?family=Inter:wght@400;500;600;700&display=swap" rel="stylesheet">
<script src="https://d3js.org/d3.v7.min.js"></script>
<style>
*, *::before, *::after { margin: 0; padding: 0; box-sizing: border-box; }
:root {
  --bg-primary: #111827;
  --bg-card: rgba(255,255,255,0.03);
  --border-card: rgba(255,255,255,0.06);
  --text-primary: #F9FAFB;
  --text-secondary: #9CA3AF;
  --text-tertiary: #6B7280;
  --brand: #6366F1;
  --brand-glow: rgba(99,102,241,0.3);
  --accent-cyan: #06B6D4;
  --accent-amber: #F59E0B;
  --accent-red: #EF4444;
  --accent-green: #22C55E;
  --sidebar-width: 280px;
  --header-height: 56px;
}
body {
  font-family: 'Inter', -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif;
  background: var(--bg-primary); color: var(--text-primary);
  overflow: hidden; height: 100vh; user-select: none;
}
/* ── Header ── */
header {
  height: var(--header-height); display: flex; align-items: center;
  padding: 0 20px; border-bottom: 1px solid var(--border-card);
  background: rgba(17,24,39,0.9); backdrop-filter: blur(12px);
  position: fixed; top: 0; left: 0; right: 0; z-index: 100;
}
header .logo { font-size: 18px; font-weight: 700; color: var(--brand); letter-spacing: -0.5px; margin-right: 32px; white-space: nowrap; }
header .logo span { color: var(--text-secondary); font-weight: 400; margin-left: 4px; font-size: 13px; }
nav { display: flex; gap: 4px; flex: 1; }
nav .tab-btn {
  padding: 6px 20px; border-radius: 8px; font-size: 14px; font-weight: 500;
  color: var(--text-secondary); cursor: pointer; transition: all .2s;
  border: none; background: transparent; font-family: inherit;
}
nav .tab-btn:hover { color: var(--text-primary); background: rgba(255,255,255,0.04); }
nav .tab-btn.active { color: var(--text-primary); background: rgba(99,102,241,0.12); }
header .status { font-size: 12px; color: var(--text-tertiary); white-space: nowrap; }
/* ── Layout ── */
.app-layout { display: flex; height: calc(100vh - var(--header-height)); margin-top: var(--header-height); }
/* ── Sidebar ── */
aside {
  width: var(--sidebar-width); min-width: var(--sidebar-width);
  border-right: 1px solid var(--border-card); overflow-y: auto;
  padding: 16px; display: flex; flex-direction: column; gap: 16px;
  background: rgba(17,24,39,0.6);
}
aside h3 { font-size: 11px; font-weight: 600; text-transform: uppercase; letter-spacing: 0.1em; color: var(--text-tertiary); margin-bottom: 8px; }
aside .task-item {
  padding: 8px 10px; border-radius: 6px; cursor: pointer; transition: background .15s;
  display: flex; gap: 8px; align-items: flex-start;
}
aside .task-item:hover { background: var(--border-card); }
aside .task-dot {
  width: 8px; height: 8px; border-radius: 50%; margin-top: 4px; flex-shrink: 0;
  background: var(--text-tertiary);
}
aside .task-dot.changed { background: var(--brand); }
aside .task-dot.active { background: var(--accent-cyan); }
aside .task-item .info { flex: 1; min-width: 0; }
aside .task-item .date { font-size: 10px; color: var(--text-tertiary); }
aside .task-item .title { font-size: 13px; color: var(--text-primary); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; margin-top: 2px; }
aside .module-index { display: flex; flex-wrap: wrap; gap: 4px; }
aside .module-tag {
  font-size: 11px; padding: 2px 8px; border-radius: 4px; cursor: pointer;
  background: rgba(255,255,255,0.04); color: var(--text-secondary); transition: all .15s;
}
aside .module-tag:hover { background: rgba(99,102,241,0.15); color: var(--brand); }
/* ── Main ── */
main { flex: 1; overflow-y: auto; padding: 0; position: relative; }
.page { display: none; padding: 24px; }
.page.active { display: block; }
/* ── 大字报区 ── */
.billboard {
  text-align: center; padding: 60px 40px 40px;
  background: linear-gradient(180deg, rgba(99,102,241,0.08) 0%, transparent 100%);
  border-bottom: 1px solid var(--border-card);
}
.billboard h1 {
  font-size: 36px; font-weight: 700; color: var(--brand); letter-spacing: -1px; line-height: 1.2;
}
.billboard .subtitle {
  font-size: 16px; color: var(--text-secondary); margin-top: 12px; max-width: 640px; margin-left: auto; margin-right: auto; line-height: 1.6;
}
.billboard .meta-tags { display: flex; gap: 8px; justify-content: center; margin-top: 16px; flex-wrap: wrap; }
.billboard .meta-tag {
  font-size: 12px; padding: 4px 12px; border-radius: 6px;
  background: rgba(99,102,241,0.1); color: var(--brand); border: 1px solid rgba(99,102,241,0.2);
}
/* ── 瀑布式影响图 ── */
.impact-section { padding: 20px 24px; }
.impact-section h2 { font-size: 14px; font-weight: 600; color: var(--text-secondary); margin-bottom: 12px; text-transform: uppercase; letter-spacing: 0.08em; }
.waterfall-container {
  width: 100%; height: 480px; background: var(--bg-card); border: 1px solid var(--border-card);
  border-radius: 12px; overflow: hidden; position: relative;
}
.waterfall-container .tooltip {
  position: absolute; padding: 10px 14px; pointer-events: none; opacity: 0;
  background: rgba(30,41,59,0.95); border: 1px solid var(--border-card); border-radius: 8px;
  font-size: 12px; color: var(--text-primary); z-index: 10; line-height: 1.5;
  backdrop-filter: blur(8px); transition: opacity .15s;
}
.waterfall-container .legend { position: absolute; bottom: 12px; right: 16px; display: flex; gap: 12px; font-size: 11px; color: var(--text-tertiary); }
.waterfall-container .legend span { display: flex; align-items: center; gap: 4px; }
.waterfall-container .legend .dot { width: 8px; height: 8px; border-radius: 50%; display: inline-block; }
/* ── LLM 解读区 ── */
.interpretation { padding: 20px 24px; }
.interpretation h2 { font-size: 14px; font-weight: 600; color: var(--text-secondary); margin-bottom: 16px; text-transform: uppercase; letter-spacing: 0.08em; }
.module-card {
  background: var(--bg-card); border: 1px solid var(--border-card); border-radius: 10px;
  padding: 16px; margin-bottom: 12px; cursor: pointer; transition: border-color .2s;
}
.module-card:hover { border-color: rgba(99,102,241,0.3); }
.module-card .mod-name { font-size: 18px; font-weight: 600; color: var(--text-primary); }
.module-card .mod-desc { font-size: 14px; color: var(--text-secondary); margin-top: 8px; line-height: 1.6; }
.module-card .mod-symbols { display: flex; gap: 4px; margin-top: 10px; flex-wrap: wrap; }
.module-card .mod-symbols .sym {
  font-size: 11px; padding: 2px 8px; border-radius: 4px; background: rgba(99,102,241,0.08);
  color: var(--brand); font-family: 'SF Mono', 'Fira Code', monospace;
}
.module-card .mod-risk {
  margin-top: 10px; font-size: 12px; padding: 6px 10px; border-radius: 6px;
  display: inline-block;
}
.risk-low { background: rgba(34,197,94,0.1); color: var(--accent-green); }
.risk-medium { background: rgba(245,158,11,0.1); color: var(--accent-amber); }
.risk-high { background: rgba(239,68,68,0.1); color: var(--accent-red); }
/* ── 能力全景页 ── */
.panorama-container {
  width: 100%; height: calc(100vh - var(--header-height) - 48px);
  background: var(--bg-card); border: 1px solid var(--border-card); border-radius: 12px;
  position: relative; overflow: hidden;
}
.panorama-container .node-info {
  position: absolute; padding: 10px 14px; pointer-events: none; opacity: 0;
  background: rgba(30,41,59,0.95); border: 1px solid var(--border-card); border-radius: 8px;
  font-size: 12px; z-index: 10; line-height: 1.5; backdrop-filter: blur(8px);
}
/* ── Tasks 页 ── */
.task-table { width: 100%; border-collapse: collapse; font-size: 14px; }
.task-table th, .task-table td { text-align: left; padding: 12px 16px; border-bottom: 1px solid var(--border-card); }
.task-table th { font-size: 11px; font-weight: 600; color: var(--text-tertiary); text-transform: uppercase; letter-spacing: 0.08em; }
.task-table tr { cursor: pointer; transition: background .15s; }
.task-table tr:hover { background: var(--bg-card); }
.badge {
  font-size: 11px; padding: 2px 10px; border-radius: 4px; font-weight: 600; text-transform: uppercase;
}
/* ── Animations ── */
@keyframes pulse { 0%, 100% { opacity: 0.4; } 50% { opacity: 1; } }
@keyframes fadeInUp { from { opacity: 0; transform: translateY(20px); } to { opacity: 1; transform: translateY(0); } }
@keyframes scaleIn { from { opacity: 0; transform: scale(0.9); } to { opacity: 1; transform: scale(1); } }
.fade-up { animation: fadeInUp .6s ease-out; }
.scale-in { animation: scaleIn .5s ease-out; }
/* ── Empty state ── */
.empty-state { text-align: center; padding: 80px 20px; color: var(--text-tertiary); }
.empty-state .icon { font-size: 48px; margin-bottom: 16px; opacity: 0.3; }
.empty-state h3 { font-size: 18px; margin-bottom: 8px; color: var(--text-secondary); }
.empty-state p { font-size: 14px; max-width: 400px; margin: 0 auto; }
.empty-state code {
  background: rgba(99,102,241,0.1); color: var(--brand); padding: 2px 8px;
  border-radius: 4px; font-size: 13px;
}
::-webkit-scrollbar { width: 6px; }
::-webkit-scrollbar-track { background: transparent; }
::-webkit-scrollbar-thumb { background: rgba(255,255,255,0.06); border-radius: 3px; }
::-webkit-scrollbar-thumb:hover { background: rgba(255,255,255,0.12); }
</style>
</head>
<body>
<header>
  <div class="logo">Paporot<span>Dashboard</span></div>
  <nav>
    <button class="tab-btn active" data-page="narrative">变更叙事</button>
    <button class="tab-btn" data-page="panorama">能力全景</button>
    <button class="tab-btn" data-page="tasks">Tasks</button>
  </nav>
  <div class="status" id="header-status">Loading...</div>
</header>
<div class="app-layout">
  <aside>
    <div>
      <h3>Task 历史</h3>
      <div id="sidebar-tasks"><div style="font-size:12px;color:var(--text-tertiary);">加载中...</div></div>
    </div>
    <div>
      <h3>模块索引</h3>
      <div class="module-index" id="sidebar-modules"></div>
    </div>
  </aside>
  <main>
    <!-- 变更叙事页 -->
    <div class="page active" id="page-narrative">
      <div id="billboard-container"></div>
      <div class="impact-section fade-up" style="animation-delay: .2s">
        <h2>影响范围 · 瀑布式扩散</h2>
        <div class="waterfall-container" id="waterfall-chart">
          <div class="tooltip" id="waterfall-tooltip"></div>
          <div class="legend">
            <span><span class="dot" style="background:var(--brand)"></span> 核心变更</span>
            <span><span class="dot" style="background:var(--accent-cyan)"></span> 受影响模块</span>
            <span><span class="dot" style="background:var(--text-tertiary)"></span> 下游依赖</span>
          </div>
        </div>
      </div>
      <div class="interpretation fade-up" style="animation-delay: .4s">
        <h2>LLM 解读</h2>
        <div id="interpretation-body"></div>
      </div>
    </div>
    <!-- 能力全景页 -->
    <div class="page" id="page-panorama">
      <div class="panorama-container" id="panorama-graph">
        <div class="node-info" id="panorama-tooltip"></div>
      </div>
    </div>
    <!-- Tasks 页 -->
    <div class="page" id="page-tasks">
      <div style="padding:0 24px;">
        <h2 style="font-size:16px;font-weight:600;margin-bottom:16px;color:var(--text-primary);">全部 Task</h2>
        <table class="task-table"><thead>
          <tr><th>ID</th><th>描述</th><th>类别</th><th>模块</th><th>状态</th></tr>
        </thead><tbody id="tasks-table-body"><tr><td colspan="5" style="color:var(--text-tertiary);">加载中...</td></tr></tbody></table>
      </div>
    </div>
  </main>
</div>
<script>
const API = '';
let currentPage = 'narrative';
let allTasks = [];
let selectedTask = null;

async function fetchAPI(path) {
  const r = await fetch(API + path);
  if (!r.ok) throw new Error(r.statusText);
  return r.json();
}

// ── Navigation ──
document.querySelectorAll('.tab-btn').forEach(btn => {
  btn.addEventListener('click', () => {
    document.querySelectorAll('.tab-btn').forEach(b => b.classList.remove('active'));
    btn.classList.add('active');
    currentPage = btn.dataset.page;
    document.querySelectorAll('.page').forEach(p => p.classList.remove('active'));
    document.getElementById('page-' + currentPage).classList.add('active');
    if (currentPage === 'panorama') loadPanorama();
    if (currentPage === 'tasks') renderTasksPage();
  });
});

// ── Initialization ──
async function init() {
  try {
    const s = await fetchAPI('/api/status');
    document.getElementById('header-status').textContent = `v${s.version} · ${s.project.split('/').pop() || s.project}`;
    allTasks = (await fetchAPI('/api/tasks')).tasks || [];
    renderSidebar();
    renderTasksPage();
    loadNarrative();
  } catch(e) {
    document.getElementById('header-status').textContent = '离线';
    showEmpty('billboard-container', '无法连接 Paporot 服务', '请运行 <code>paporot dashboard</code>');
  }
}

function showEmpty(containerId, title, desc) {
  const el = document.getElementById(containerId);
  el.innerHTML = `<div class="empty-state"><div class="icon">&#128266;</div><h3>${title}</h3><p>${desc}</p></div>`;
}

// ── Sidebar ──
function renderSidebar() {
  const container = document.getElementById('sidebar-tasks');
  if (!allTasks.length) {
    container.innerHTML = '<div style="font-size:12px;color:var(--text-tertiary);">暂无 Task</div>';
    return;
  }
  container.innerHTML = allTasks.slice(0, 10).map((t, i) => `
    <div class="task-item" onclick="selectTask('${t.id}')">
      <div class="task-dot changed"></div>
      <div class="info">
        <div class="date">Task #${i + 1}</div>
        <div class="title">${escHtml(t.description || t.id)}</div>
      </div>
      <span style="font-size:10px;color:var(--text-tertiary);">${t.category}</span>
    </div>
  `).join('');

  // Module index
  const modSet = new Set();
  allTasks.forEach(t => (t.modules || []).forEach(m => modSet.add(m)));
  const modContainer = document.getElementById('sidebar-modules');
  if (modSet.size === 0) {
    modContainer.innerHTML = '<span style="font-size:11px;color:var(--text-tertiary);">无</span>';
  } else {
    modContainer.innerHTML = [...modSet].map(m =>
      `<span class="module-tag" onclick="focusModule('${m}')">${m}</span>`
    ).join('');
  }
}

function escapeHtml(str) {
  const div = document.createElement('div');
  div.textContent = str;
  return div.innerHTML;
}
const escHtml = escapeHtml;

// ── Task Selection ──
async function selectTask(taskId) {
  selectedTask = taskId;
  document.querySelectorAll('.tab-btn').forEach(b => b.classList.remove('active'));
  document.querySelector('[data-page="narrative"]').classList.add('active');
  document.querySelectorAll('.page').forEach(p => p.classList.remove('active'));
  document.getElementById('page-narrative').classList.add('active');
  currentPage = 'narrative';
  await loadNarrative(taskId);
}

// ── 变更叙事页 ──
async function loadNarrative(taskId) {
  const id = taskId || (allTasks.length > 0 ? allTasks[0].id : null);
  if (!id) {
    showEmpty('billboard-container', '没有 Task', '运行 <code>paporot task new "描述"</code> 创建 Task');
    document.getElementById('interpretation-body').innerHTML = '';
    drawWaterfall({nodes:[],links:[]});
    return;
  }
  selectedTask = id;
  try {
    const data = await fetchAPI('/api/eval/' + id);
    if (!data.trials || data.trials.length === 0) {
      showEmpty('billboard-container', '暂无评估', `Task <code>${id}</code> 还没有 Trial`);
      drawWaterfall({nodes:[],links:[]});
      document.getElementById('interpretation-body').innerHTML = '';
      return;
    }
    const trial = data.trials[data.trials.length - 1];
    renderBillboard(trial);
    renderInterpretation(trial);
    drawWaterfall(trial.code_change);
  } catch(e) {
    showEmpty('billboard-container', '加载失败', e.message);
  }
}

function renderBillboard(trial) {
  const cc = trial.code_change;
  const container = document.getElementById('billboard-container');
  const title = trial.task ? trial.task.description : (selectedTask || '代码变更');
  const symbolNames = (cc.symbols_added || []).map(s => s.name).slice(0, 5);
  const mods = cc.modules || cc.files?.map(f => f.split('/')[1] || f).filter((v,i,a)=>a.indexOf(v)===i) || [];

  container.innerHTML = `
    <div class="billboard scale-in">
      <h1>「${escHtml(title)}」</h1>
      <div class="subtitle">
        Agent 修改了 ${cc.files?.length || 0} 个文件，新增 ${cc.additions || 0} 行·删除 ${cc.deletions || 0} 行。
        涉及模块：${mods.slice(0,4).map(m => `<b>${m}</b>`).join('、') || '无'}
      </div>
      <div class="meta-tags">
        <span class="meta-tag">${cc.files?.length || 0} 个文件</span>
        <span class="meta-tag">+${cc.additions || 0}/-${cc.deletions || 0}</span>
        ${mods.slice(0,5).map(m => `<span class="meta-tag">${escHtml(m)}</span>`).join('')}
      </div>
    </div>
  `;
}

function renderInterpretation(trial) {
  const cc = trial.code_change;
  const mods = cc.modules || cc.files?.map(f => f.split('/')[1] || f).filter((v,i,a)=>a.indexOf(v)===i) || [];
  const symsByMod = {};
  (cc.symbols_added || []).forEach(s => {
    const mod = s.file?.split('/')[1] || 'root';
    (symsByMod[mod] = symsByMod[mod] || []).push(s);
  });
  const container = document.getElementById('interpretation-body');
  if (mods.length === 0) {
    container.innerHTML = '<div class="module-card"><div class="mod-name">无模块变更</div><div class="mod-desc">本次变更未检测到模块级变化。</div></div>';
    return;
  }
  container.innerHTML = mods.map(mod => {
    const syms = symsByMod[mod] || [];
    const riskLevel = computeRisk(cc);
    const riskClass = riskLevel === 'low' ? 'risk-low' : riskLevel === 'medium' ? 'risk-medium' : 'risk-high';
    return `
      <div class="module-card" onclick="focusModule('${escHtml(mod)}')">
        <div class="mod-name">📦 ${escHtml(mod)}</div>
        <div class="mod-desc">
          ${cc.files?.filter(f => f.includes(mod)).length || 0} 个文件被修改。
          ${syms.length > 0 ? `修改了 <b>${syms.length}</b> 个符号。` : ''}
        </div>
        <div class="mod-symbols">
          ${syms.slice(0, 8).map(s => `<span class="sym">${escHtml(s.name)}</span>`).join('')}
          ${syms.length > 8 ? `<span style="font-size:11px;color:var(--text-tertiary);">+${syms.length - 8}</span>` : ''}
        </div>
        ${cc.additions > 100 || cc.files?.length > 5 ?
          `<div class="mod-risk ${riskClass}">&#9888; 风险：${riskLevel === 'high' ? '高' : riskLevel === 'medium' ? '中' : '低'} · 大量变更建议审查</div>`
          : ''}
      </div>
    `;
  }).join('');
}

function computeRisk(cc) {
  if ((cc.additions || 0) + (cc.deletions || 0) > 500) return 'high';
  if ((cc.additions || 0) + (cc.deletions || 0) > 200) return 'medium';
  return 'low';
}

// ── 瀑布式影响图 ──
function drawWaterfall(codeChange) {
  const container = document.getElementById('waterfall-chart');
  const svgEl = container.querySelector('svg');
  if (svgEl) svgEl.remove();

  if (!codeChange) return;
  const files = codeChange.files || [];
  const syms = (codeChange.symbols_added || []).concat(codeChange.symbols_removed || []);
  if (syms.length === 0 && files.length === 0) {
    const rect = container.getBoundingClientRect();
    const svg = d3.select('#waterfall-chart').append('svg')
      .attr('width', rect.width).attr('height', 480);
    svg.append('text').attr('x', rect.width/2).attr('y', 240)
      .attr('text-anchor', 'middle').attr('fill', '#6B7280').attr('font-size', '14px')
      .text('暂无变更数据');
    return;
  }

  const rect = container.getBoundingClientRect();
  const W = rect.width, H = 480;
  const cx = W / 2, cy = H / 2;

  const svg = d3.select('#waterfall-chart').append('svg')
    .attr('width', W).attr('height', H);

  // Build data: inner ring = symbols, outer ring = modules
  const innerNodes = syms.slice(0, 12).map((s, i) => ({
    id: s.name, type: 'symbol', name: s.name, kind: s.kind,
    action: s.action || 'added', radius: 120,
    angle: (2 * Math.PI * i) / Math.min(syms.length, 12),
    size: 6 + Math.min(syms.length, 20) * 0.3,
  }));

  const modules = [...new Set(files.map(f => {
    const p = f.split('/'); return p.length > 1 ? p[1] : p[0];
  }))];

  const outerNodes = modules.slice(0, 10).map((m, i) => ({
    id: 'mod-' + m, type: 'module', name: m, radius: 200,
    angle: (2 * Math.PI * i) / Math.min(modules.length, 10),
    size: 12,
  }));

  const allNodes = [...innerNodes, ...outerNodes];
  const links = [];
  innerNodes.forEach(s => {
    outerNodes.forEach(m => {
      links.push({source: s.id, target: m.id, value: 1});
    });
  });
  // Downstream links between outer modules
  for (let i = 0; i < outerNodes.length - 1; i++) {
    links.push({source: outerNodes[i].id, target: outerNodes[i+1].id, value: 0.5, downstream: true});
  }

  // Position nodes
  innerNodes.forEach(n => { n.x = cx + n.radius * Math.cos(n.angle - Math.PI/2); n.y = cy + n.radius * Math.sin(n.angle - Math.PI/2); });
  outerNodes.forEach(n => { n.x = cx + n.radius * Math.cos(n.angle - Math.PI/2); n.y = cy + n.radius * Math.sin(n.angle - Math.PI/2); });

  // Add downstream tree nodes
  const treeNodes = [];
  outerNodes.forEach((n, i) => {
    for (let j = 0; j < 3; j++) {
      const dist = 270 + j * 50;
      const angle = n.angle + (j - 1) * 0.3;
      treeNodes.push({
        id: `tree-${n.id}-${j}`, type: 'downstream',
        name: `...`,
        x: cx + dist * Math.cos(angle - Math.PI/2),
        y: cy + dist * Math.sin(angle - Math.PI/2),
        size: 3, opacity: 0.4 - j * 0.1,
      });
    }
  });

  const allNodesCombined = [...innerNodes, ...outerNodes, ...treeNodes];

  // Draw links
  const linkG = svg.append('g');
  links.forEach(l => {
    const sNode = allNodesCombined.find(n => n.id === (typeof l.source === 'string' ? l.source : l.source.id));
    const tNode = allNodesCombined.find(n => n.id === (typeof l.target === 'string' ? l.target : l.target.id));
    if (!sNode || !tNode) return;
    linkG.append('line')
      .attr('x1', sNode.x).attr('y1', sNode.y)
      .attr('x2', tNode.x).attr('y2', tNode.y)
      .attr('stroke', l.downstream ? '#374151' : '#475569')
      .attr('stroke-width', l.downstream ? 0.5 : 0.8)
      .attr('stroke-opacity', l.downstream ? 0.3 : 0.4);
  });

  // Draw nodes
  const nodeG = svg.append('g');
  const tooltip = d3.select('#waterfall-tooltip');

  allNodesCombined.forEach(n => {
    const el = nodeG.append('g');
    if (n.type === 'symbol') {
      el.append('circle').attr('r', n.size)
        .attr('cx', n.x).attr('cy', n.y)
        .attr('fill', n.action === 'removed' ? '#EF4444' : '#6366F1')
        .attr('stroke', '#111827').attr('stroke-width', 2)
        .style('animation', 'pulse 3s infinite');
      el.append('text').attr('x', n.x + 12).attr('y', n.y + 4)
        .text(n.name).attr('fill', '#F9FAFB').attr('font-size', '11px');
    } else if (n.type === 'module') {
      el.append('rect')
        .attr('x', n.x - n.size).attr('y', n.y - n.size/2)
        .attr('width', n.size * 2).attr('height', n.size)
        .attr('rx', 3).attr('fill', '#06B6D4').attr('opacity', 0.6);
      el.append('text').attr('x', n.x + n.size + 8).attr('y', n.y + 4)
        .text(n.name).attr('fill', '#9CA3AF').attr('font-size', '12px').attr('font-weight', '500');
    } else {
      el.append('circle').attr('r', n.size)
        .attr('cx', n.x).attr('cy', n.y)
        .attr('fill', '#6B7280').attr('opacity', n.opacity);
    }

    // Hover for symbols and modules
    if (n.type !== 'downstream') {
      el.style('cursor', 'pointer');
      el.on('mouseenter', (e) => {
        tooltip.style('opacity', '1')
          .style('left', (e.clientX - container.getBoundingClientRect().left + 12) + 'px')
          .style('top', (e.clientY - container.getBoundingClientRect().top - 10) + 'px')
          .html(n.type === 'symbol'
            ? `<b>${n.name}</b><br>${n.kind} · ${n.action === 'removed' ? '已删除' : '新增'}`
            : `<b>${n.name}</b><br>受影响模块`);
      });
      el.on('mouseleave', () => { tooltip.style('opacity', '0'); });
    }
  });
}

// ── 能力全景页 ──
function loadPanorama() {
  if (currentPage !== 'panorama') return;
  const container = document.getElementById('panorama-graph');
  // Clear previous SVG
  const existing = container.querySelector('svg');
  if (existing) existing.remove();

  const W = container.clientWidth || 900, H = container.clientHeight || 600;
  const svg = d3.select('#panorama-graph').append('svg')
    .attr('width', W).attr('height', H);

  fetchAPI('/api/capabilities').then(data => {
    if (!data.nodes || data.nodes.length === 0) {
      svg.append('text').attr('x', W/2).attr('y', H/2)
        .attr('text-anchor', 'middle').attr('fill', '#6B7280').attr('font-size', '14px')
        .text('暂无能力数据 · 运行 paporot eval auto');
      return;
    }

    // Build module-level nodes
    const modMap = {};
    data.nodes.forEach(n => {
      const mod = n.file?.split('/')[1] || 'root';
      if (!modMap[mod]) modMap[mod] = { mod, symbols: [], action: n.action };
      modMap[mod].symbols.push(n);
    });

    const nodes = Object.values(modMap).map((m, i) => ({
      id: m.mod, name: m.mod, action: m.action,
      symbolCount: m.symbols.length, size: 10 + m.symbols.length * 3,
    }));

    const links = [];
    for (let i = 0; i < nodes.length; i++) {
      for (let j = i + 1; j < nodes.length; j++) {
        if (Math.random() < 0.3) {
          links.push({source: nodes[i].id, target: nodes[j].id, value: Math.random() * 0.5 + 0.1});
        }
      }
    }

    // Entry animation: start from center, spread outward
    nodes.forEach(n => { n.x = W/2; n.y = H/2; });

    const simulation = d3.forceSimulation(nodes)
      .force('link', d3.forceLink(links).id(d => d.id).distance(120))
      .force('charge', d3.forceManyBody().strength(-300))
      .force('center', d3.forceCenter(W/2, H/2))
      .force('collision', d3.forceCollide(30))
      .alpha(1);

    const linkEl = svg.append('g').selectAll('line')
      .data(links).join('line')
      .attr('stroke', '#374151').attr('stroke-width', d => d.value * 4).attr('stroke-opacity', 0.4);

    const nodeG = svg.append('g').selectAll('g')
      .data(nodes).join('g')
      .call(d3.drag()
        .on('start', (e,d) => { if(!e.active) simulation.alphaTarget(0.3).restart(); d.fx = d.x; d.fy = d.y; })
        .on('drag', (e,d) => { d.fx = e.x; d.fy = e.y; })
        .on('end', (e,d) => { if(!e.active) simulation.alphaTarget(0); d.fx = null; d.fy = null; }));

    nodeG.append('rect')
      .attr('width', d => d.symbolCount > 1 ? d.size * 2.5 : d.size * 2)
      .attr('height', d => d.symbolCount > 1 ? d.size * 2.5 : d.size * 2)
      .attr('x', d => -(d.symbolCount > 1 ? d.size * 1.25 : d.size))
      .attr('y', d => -(d.symbolCount > 1 ? d.size * 1.25 : d.size))
      .attr('rx', 4)
      .attr('fill', d => d.action === 'removed' ? '#EF4444' : '#6366F1')
      .attr('opacity', 0.7)
      .style('animation', d => d.action === 'added' ? 'pulse 3s infinite' : 'none');

    nodeG.append('text')
      .text(d => d.name).attr('y', d => d.size + 16)
      .attr('text-anchor', 'middle').attr('fill', '#9CA3AF').attr('font-size', '11px');

    const tooltip = d3.select('#panorama-tooltip');
    nodeG.on('mouseenter', (e, d) => {
      tooltip.style('opacity', '1')
        .style('left', (e.clientX - container.getBoundingClientRect().left + 12) + 'px')
        .style('top', (e.clientY - container.getBoundingClientRect().top - 10) + 'px')
        .html(`<b>${d.name}</b><br>${d.symbolCount} 个符号<br>状态：${d.action === 'added' ? '新增' : '已删除'}`);
    });
    nodeG.on('mouseleave', () => { tooltip.style('opacity', '0'); });

    simulation.on('tick', () => {
      linkEl.attr('x1', d => d.source.x).attr('y1', d => d.source.y)
             .attr('x2', d => d.target.x).attr('y2', d => d.target.y);
      nodeG.attr('transform', d => `translate(${d.x},${d.y})`);
    });

    // Animation: fade in after spread
    simulation.alphaDecay(0.02);
  });
}

function focusModule(name) {
  document.querySelectorAll('.tab-btn').forEach(b => b.classList.remove('active'));
  document.querySelector('[data-page="panorama"]').classList.add('active');
  document.querySelectorAll('.page').forEach(p => p.classList.remove('active'));
  document.getElementById('page-panorama').classList.add('active');
  currentPage = 'panorama';
  loadPanorama();
}

// ── Tasks 页 ──
function renderTasksPage() {
  const tbody = document.getElementById('tasks-table-body');
  if (!allTasks.length) {
    tbody.innerHTML = '<tr><td colspan="5" style="text-align:center;padding:40px;color:var(--text-tertiary);">暂无 Task · 运行 <code>paporot task new "描述"</code></td></tr>';
    return;
  }
  tbody.innerHTML = allTasks.map(t => `
    <tr onclick="selectTask('${t.id}')">
      <td style="font-family:monospace;font-size:12px;color:var(--text-secondary);">${t.id.slice(0,12)}...</td>
      <td style="font-weight:500;">${escHtml(t.description || t.id)}</td>
      <td>${t.category}</td>
      <td>${(t.modules || []).slice(0,3).join(', ') || '—'}</td>
      <td><span class="badge" style="background:rgba(99,102,241,0.1);color:var(--brand);">—</span></td>
    </tr>
  `).join('');
}

init();
</script>
</body>
</html>"##;
