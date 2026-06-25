<script setup lang="ts">
import { ref, computed } from 'vue'
import type { DashboardData } from './types'
import TopBar from './components/TopBar.vue'
import BehaviorChanges from './components/BehaviorChanges.vue'
import FeedbackLoop from './components/FeedbackLoop.vue'
import SnapshotTimeline from './components/SnapshotTimeline.vue'
import TraceAnalysis from './components/TraceAnalysis.vue'
import SkillsPanel from './components/SkillsPanel.vue'

// Read data injected by Paporot at generation time
declare global {
  interface Window {
    __PAPOROT_DATA__?: DashboardData
  }
}

const data = ref<DashboardData | null>(window.__PAPOROT_DATA__ ?? null)

const activeTab = ref<'behavior' | 'feedback' | 'snapshot' | 'trace'>('behavior')

const tabs = computed(() => [
  { key: 'behavior' as const, label: '行为变更', icon: '◆', count: data.value?.l1_analysis?.total_changes ?? 0 },
  { key: 'feedback' as const, label: '反馈回路', icon: '↻', count: data.value?.feedback_loop?.changes?.filter(c => c.suppressed).length ?? 0, show: data.value?.feedback_loop?.loaded },
  { key: 'snapshot' as const, label: '版本历史', icon: '◷', count: data.value?.snapshot?.version_count ?? 0, show: (data.value?.snapshot?.version_count ?? 0) > 0 },
  { key: 'trace' as const, label: 'Agent 轨迹', icon: '◉', count: data.value?.trace_association?.matched_traces?.length ?? 0, show: (data.value?.trace_association?.matched_traces?.length ?? 0) > 0 },
])
</script>

<template>
  <div v-if="data" class="dashboard">
    <TopBar :data="data" />

    <!-- Tab Navigation -->
    <nav class="tab-nav">
      <button
        v-for="tab in tabs"
        v-show="tab.show !== false"
        :key="tab.key"
        :class="['tab-btn', { active: activeTab === tab.key }]"
        @click="activeTab = tab.key"
      >
        <span class="tab-icon">{{ tab.icon }}</span>
        <span class="tab-label">{{ tab.label }}</span>
        <span class="tab-count">{{ tab.count }}</span>
      </button>
    </nav>

    <!-- Tab Content -->
    <main class="tab-content">
      <BehaviorChanges v-if="activeTab === 'behavior'" :data="data" />
      <FeedbackLoop v-if="activeTab === 'feedback'" :data="data.feedback_loop" />
      <SnapshotTimeline v-if="activeTab === 'snapshot'" :snapshot="data.snapshot!" />
      <TraceAnalysis v-if="activeTab === 'trace'" :trace="data.trace_association!" />
    </main>

    <!-- Skills Panel (always visible at bottom) -->
    <SkillsPanel :skills="data.skills" />
  </div>

  <div v-else class="no-data">
    <div class="no-data-icon">⚠</div>
    <h2>No Analysis Data</h2>
    <p>Run <code>paporot analyze</code> to generate a dashboard.</p>
  </div>
</template>

<style>
/* ══════════════════════════════════════════════════════════
   Paporot Dashboard — Global Styles
   Theme: GitHub Dark
   ══════════════════════════════════════════════════════════ */
:root {
  --bg: #0d1117;
  --bg-elevated: #161b22;
  --bg-overlay: #1c2129;
  --border: #30363d;
  --border-emphasis: #484f58;
  --text: #e6edf3;
  --text-secondary: #8b949e;
  --text-tertiary: #6e7681;

  --accent: #58a6ff;
  --accent-emphasis: #79c0ff;
  --success: #3fb950;
  --success-emphasis: #56d364;
  --warning: #d29922;
  --warning-emphasis: #e3b341;
  --danger: #f85149;
  --danger-emphasis: #ff7b72;
  --purple: #a371f7;
  --purple-emphasis: #bc8cff;
  --teal: #39d353;

  --radius: 8px;
  --radius-sm: 6px;
  --font-mono: 'SF Mono', 'Fira Code', 'Cascadia Code', 'JetBrains Mono', monospace;
  --shadow: 0 1px 3px rgba(0,0,0,0.3);
}

*, *::before, *::after { box-sizing: border-box; margin: 0; padding: 0; }

body {
  font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', 'Noto Sans SC', sans-serif;
  background: var(--bg);
  color: var(--text);
  line-height: 1.5;
  -webkit-font-smoothing: antialiased;
}

.dashboard { max-width: 1400px; margin: 0 auto; padding: 0 24px 40px; }

/* ── Tab Nav ── */
.tab-nav {
  display: flex; gap: 2px;
  background: var(--bg-elevated);
  border: 1px solid var(--border);
  border-radius: var(--radius);
  padding: 4px;
  margin-bottom: 20px;
}
.tab-btn {
  flex: 1; display: flex; align-items: center; justify-content: center; gap: 6px;
  padding: 10px 16px; border: none; border-radius: 6px;
  background: transparent; color: var(--text-secondary);
  font-size: 13px; font-weight: 500; cursor: pointer;
  transition: all 0.15s ease;
}
.tab-btn:hover { color: var(--text); background: rgba(255,255,255,0.04); }
.tab-btn.active {
  color: var(--text); background: var(--bg);
  box-shadow: var(--shadow);
}
.tab-icon { font-size: 14px; }
.tab-label { white-space: nowrap; }
.tab-count {
  font-size: 11px; font-weight: 600; padding: 1px 8px;
  border-radius: 10px; background: rgba(255,255,255,0.08);
  color: var(--text-secondary);
}
.tab-btn.active .tab-count { background: rgba(88,166,255,0.15); color: var(--accent); }

/* ── Tab Content ── */
.tab-content { min-height: 400px; }

/* ── Common Card ── */
.card {
  background: var(--bg-elevated);
  border: 1px solid var(--border);
  border-radius: var(--radius);
  padding: 20px;
  margin-bottom: 16px;
}
.card-header {
  display: flex; justify-content: space-between; align-items: center;
  margin-bottom: 16px; padding-bottom: 12px;
  border-bottom: 1px solid var(--border);
}
.card-title { font-size: 15px; font-weight: 700; color: var(--text); }
.card-subtitle { font-size: 12px; color: var(--text-secondary); }

/* ── Stat Grid ── */
.stat-grid { display: grid; grid-template-columns: repeat(auto-fit, minmax(140px, 1fr)); gap: 12px; margin-bottom: 20px; }
.stat-card {
  background: var(--bg); border: 1px solid var(--border);
  border-radius: var(--radius-sm); padding: 14px 16px;
}
.stat-value { font-size: 28px; font-weight: 700; line-height: 1.2; }
.stat-label { font-size: 11px; color: var(--text-secondary); text-transform: uppercase; letter-spacing: 0.4px; margin-top: 4px; }

/* ── Badge / Tag ── */
.badge {
  display: inline-flex; align-items: center; gap: 4px;
  padding: 3px 10px; border-radius: 12px;
  font-size: 11px; font-weight: 600; white-space: nowrap;
}
.badge-success { color: var(--success); background: rgba(63,185,80,0.12); border: 1px solid rgba(63,185,80,0.25); }
.badge-warning { color: var(--warning); background: rgba(210,153,34,0.12); border: 1px solid rgba(210,153,34,0.25); }
.badge-danger { color: var(--danger); background: rgba(248,81,73,0.12); border: 1px solid rgba(248,81,73,0.25); }
.badge-info { color: var(--accent); background: rgba(88,166,255,0.12); border: 1px solid rgba(88,166,255,0.25); }
.badge-purple { color: var(--purple); background: rgba(163,113,247,0.12); border: 1px solid rgba(163,113,247,0.25); }

.tag {
  display: inline-block; padding: 2px 8px; border-radius: 4px;
  font-size: 11px; font-family: var(--font-mono); font-weight: 500;
  margin-right: 4px; margin-bottom: 4px;
}

/* ── Table ── */
.data-table {
  width: 100%; border-collapse: collapse; font-size: 13px;
}
.data-table th {
  text-align: left; padding: 8px 12px;
  font-size: 11px; font-weight: 600; color: var(--text-secondary);
  text-transform: uppercase; letter-spacing: 0.4px;
  border-bottom: 1px solid var(--border);
  white-space: nowrap;
}
.data-table td {
  padding: 8px 12px; border-bottom: 1px solid rgba(48,54,61,0.5);
  vertical-align: middle;
}
.data-table tr:hover td { background: rgba(255,255,255,0.02); }

/* ── Progress Bar ── */
.progress-bar { height: 6px; border-radius: 3px; background: var(--bg); overflow: hidden; display: flex; }
.progress-seg { height: 100%; transition: width 0.4s ease; }

/* ── Empty State ── */
.empty-state { text-align: center; padding: 48px 24px; color: var(--text-secondary); }
.empty-state-icon { font-size: 32px; margin-bottom: 12px; }
.empty-state h3 { font-size: 15px; font-weight: 600; color: var(--text); margin-bottom: 4px; }
.empty-state p { font-size: 13px; }

/* ── No Data Fallback ── */
.no-data { text-align: center; padding: 80px 24px; }
.no-data-icon { font-size: 48px; margin-bottom: 16px; }
.no-data h2 { font-size: 20px; color: var(--text); margin-bottom: 8px; }
.no-data p { color: var(--text-secondary); font-size: 14px; }
.no-data code { color: var(--accent); background: rgba(88,166,255,0.1); padding: 2px 6px; border-radius: 4px; }

/* ── Footer ── */
.dashboard-footer {
  text-align: center; color: var(--text-tertiary); font-size: 11px;
  padding: 24px; border-top: 1px solid var(--border); margin-top: 32px;
}

/* ── Scrollbar ── */
::-webkit-scrollbar { width: 8px; height: 8px; }
::-webkit-scrollbar-track { background: var(--bg); }
::-webkit-scrollbar-thumb { background: var(--border); border-radius: 4px; }
::-webkit-scrollbar-thumb:hover { background: var(--border-emphasis); }
</style>
