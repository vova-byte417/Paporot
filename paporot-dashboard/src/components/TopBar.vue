<script setup lang="ts">
import { computed } from 'vue'
import type { DashboardData } from '../types'

const props = defineProps<{ data: DashboardData }>()

const riskLevel = computed(() => {
  const l2 = props.data.l2_analysis
  const critHigh = (l2.by_severity['Critical'] ?? 0) + (l2.by_severity['High'] ?? 0)
  if (critHigh > 0) return { level: 'High', cls: 'danger', desc: '存在高风险变更，建议人工审查' }
  if (l2.total_matches > 3) return { level: 'Medium', cls: 'warning', desc: '发现多项需关注的变更模式' }
  return { level: 'Low', cls: 'success', desc: '未检测到高风险行为模式' }
})

const changeBreakdown = computed(() => {
  const byType = props.data.l1_analysis.by_type
  const added = Object.entries(byType).filter(([k]) => k.includes('Added')).reduce((s, [,v]) => s + v, 0)
  const removed = Object.entries(byType).filter(([k]) => k.includes('Removed') || k.includes('Deleted')).reduce((s, [,v]) => s + v, 0)
  const modified = Object.entries(byType).filter(([k]) => k.includes('Modified') || k.includes('Changed')).reduce((s, [,v]) => s + v, 0)
  return { added, removed, modified }
})

const suppressedCount = computed(() =>
  props.data.feedback_loop.changes.filter(c => c.suppressed).length
)
</script>

<template>
  <header class="topbar">
    <div class="topbar-left">
      <h1 class="topbar-title">
        <span class="title-icon">◈</span>
        {{ data.project_name }}
      </h1>
      <div class="topbar-meta">
        <span>{{ data.analyzed_at }}</span>
        <span v-if="data.git_commit" class="meta-sep">·</span>
        <code v-if="data.git_commit">{{ data.git_commit.slice(0, 7) }}</code>
        <span v-if="data.git_ref" class="meta-sep">·</span>
        <span v-if="data.git_ref">{{ data.git_ref }}</span>
      </div>
    </div>

    <div class="topbar-stats">
      <div class="top-stat">
        <div class="top-stat-value" style="color:var(--accent)">{{ data.l1_analysis.total_changes }}</div>
        <div class="top-stat-label">行为变更</div>
      </div>
      <div class="top-stat">
        <div class="top-stat-value" :style="{ color: changeBreakdown.added > 0 ? 'var(--success)' : 'var(--text-secondary)' }">+{{ changeBreakdown.added }}</div>
        <div class="top-stat-label">新增能力</div>
      </div>
      <div class="top-stat">
        <div class="top-stat-value" :style="{ color: changeBreakdown.removed > 0 ? 'var(--danger)' : 'var(--text-secondary)' }">−{{ changeBreakdown.removed }}</div>
        <div class="top-stat-label">移除能力</div>
      </div>
      <div class="top-stat">
        <div class="top-stat-value" :style="{ color: suppressedCount > 0 ? 'var(--purple)' : 'var(--text-secondary)' }">{{ suppressedCount }}</div>
        <div class="top-stat-label">已抑制</div>
      </div>
      <div class="top-risk" :class="`badge badge-${riskLevel.cls}`">
        {{ riskLevel.level }}
      </div>
    </div>
  </header>
</template>

<style scoped>
.topbar {
  background: var(--bg-elevated);
  border: 1px solid var(--border);
  border-radius: var(--radius);
  padding: 20px 24px;
  margin: 20px 0 0;
  display: flex;
  justify-content: space-between;
  align-items: center;
  flex-wrap: wrap;
  gap: 16px;
}
.topbar-left { display: flex; flex-direction: column; gap: 4px; }
.topbar-title { font-size: 20px; font-weight: 700; display: flex; align-items: center; gap: 8px; }
.title-icon { font-size: 18px; color: var(--accent); }
.topbar-meta { font-size: 12px; color: var(--text-secondary); display: flex; align-items: center; gap: 6px; flex-wrap: wrap; }
.topbar-meta code { font-size: 11px; color: var(--accent); background: rgba(88,166,255,0.1); padding: 1px 6px; border-radius: 4px; }
.meta-sep { color: var(--border-emphasis); }

.topbar-stats { display: flex; gap: 20px; align-items: center; flex-wrap: wrap; }
.top-stat { text-align: center; min-width: 60px; }
.top-stat-value { font-size: 22px; font-weight: 700; line-height: 1; }
.top-stat-label { font-size: 10px; color: var(--text-secondary); text-transform: uppercase; letter-spacing: 0.4px; margin-top: 2px; }
.top-risk { font-size: 12px !important; padding: 6px 16px !important; }
</style>
