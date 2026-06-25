<script setup lang="ts">
import { computed } from 'vue'
import type { DashboardData, DirectoryChange } from '../types'

const props = defineProps<{ data: DashboardData }>()

// ── Confidence distribution ──
const confDist = computed(() => props.data.l1_analysis.confidence_distribution)

// ── Top directories by change count ──
const topDirs = computed(() => {
  const dirs = [...props.data.l1_analysis.by_directory]
  dirs.sort((a, b) => b.changes - a.changes)
  return dirs.slice(0, 8)
})
const maxDirChanges = computed(() => Math.max(...topDirs.value.map(d => d.changes), 1))

// ── L2 severity stats ──
const sevOrder = ['Critical', 'High', 'Medium', 'Low', 'Info']
const sevColors: Record<string, string> = {
  Critical: 'var(--danger)',
  High: 'var(--warning)',
  Medium: 'var(--accent)',
  Low: 'var(--success)',
  Info: 'var(--text-secondary)',
}
const sevBgColors: Record<string, string> = {
  Critical: 'rgba(248,81,73,0.15)',
  High: 'rgba(210,153,34,0.15)',
  Medium: 'rgba(88,166,255,0.15)',
  Low: 'rgba(63,185,80,0.15)',
  Info: 'rgba(139,148,158,0.1)',
}
const sevStats = computed(() => {
  const bySev = props.data.l2_analysis.by_severity
  return sevOrder.map(s => ({ severity: s, count: bySev[s] ?? 0 }))
})

// ── Category breakdown ──
const catStats = computed(() => {
  const byCat = props.data.l2_analysis.by_category
  return Object.entries(byCat)
    .map(([cat, count]) => ({ category: cat, count }))
    .sort((a, b) => b.count - a.count)
})

// ── Changes with L2 matches ──
const changesWithRules = computed(() => {
  return props.data.l1_analysis.changes.filter(c => c.rules.length > 0)
})

// ── Suppressed changes ──
const suppressedChanges = computed(() => {
  return props.data.l1_analysis.changes.filter(c => c.suppressed != null || c.tags.some(t => t.includes('suppress') || t.includes('reject') || t.includes('fp-history')))
})

// ── Clean changes (no suppression, no rules) ──
const cleanChanges = computed(() => {
  return props.data.l1_analysis.changes.filter(c => c.rules.length === 0 && !c.suppressed && !c.tags.some(t => t.includes('suppress') || t.includes('reject')))
})

// ── Helpers ──
function dirBar(dir: DirectoryChange): string {
  return ((dir.changes / maxDirChanges.value) * 100).toFixed(0) + '%'
}
function confColor(c: number): string {
  if (c >= 0.85) return 'var(--success)'
  if (c >= 0.5) return 'var(--warning)'
  return 'var(--danger)'
}
function confBg(c: number): string {
  if (c >= 0.85) return 'rgba(63,185,80,0.12)'
  if (c >= 0.5) return 'rgba(210,153,34,0.12)'
  return 'rgba(248,81,73,0.12)'
}
function changeTypeLabel(t: string): string {
  return t.replace(/([A-Z])/g, ' $1').trim()
}
function changeTypeClass(t: string): string {
  if (t.includes('Added')) return 'badge-success'
  if (t.includes('Removed') || t.includes('Deleted')) return 'badge-danger'
  if (t.includes('Modified') || t.includes('Changed')) return 'badge-warning'
  return 'badge-info'
}
function shortFile(f: string): string {
  const parts = f.replace(/\\/g, '/').split('/')
  if (parts.length <= 2) return f
  return '…/' + parts.slice(-2).join('/')
}
function isSuppressed(c: { suppressed?: { level: string; reason: string } | null; tags: string[] }): boolean {
  return c.suppressed != null || c.tags.some(t => t.includes('suppress') || t.includes('reject'))
}
function suppressionLabel(c: { suppressed?: { level: string } | null; tags: string[] }): string {
  if (c.suppressed) return c.suppressed.level === 'Exact' ? '精确抑制' : c.suppressed.level === 'Rule' ? '规则抑制' : '警告'
  if (c.tags.some(t => t.includes('fp-history'))) return '历史警告'
  if (c.tags.some(t => t.includes('reject'))) return '已拒绝'
  return '已抑制'
}
</script>

<template>
  <div>
    <!-- ── Overview Stats ── -->
    <div class="stat-grid">
      <div class="stat-card">
        <div class="stat-value" style="color:var(--accent)">{{ data.l1_analysis.total_changes }}</div>
        <div class="stat-label">行为变更总数</div>
      </div>
      <div class="stat-card">
        <div class="stat-value" style="color:var(--success)">{{ confDist.high }}</div>
        <div class="stat-label">高置信度 (≥85%)</div>
      </div>
      <div class="stat-card">
        <div class="stat-value" style="color:var(--warning)">{{ confDist.medium }}</div>
        <div class="stat-label">中置信度 (50-85%)</div>
      </div>
      <div class="stat-card">
        <div class="stat-value" style="color:var(--danger)">{{ confDist.low }}</div>
        <div class="stat-label">低置信度 (&lt;50%)</div>
      </div>
      <div class="stat-card">
        <div class="stat-value" style="color:var(--purple)">{{ data.l2_analysis.total_matches }}</div>
        <div class="stat-label">规则命中 (L2)</div>
      </div>
      <div class="stat-card">
        <div class="stat-value" style="color:var(--success)">{{ cleanChanges.length }}</div>
        <div class="stat-label">无害变更</div>
      </div>
    </div>

    <!-- ── Two Column: Directory Breakdown + Severity ── -->
    <div class="two-col">
      <!-- Directory Breakdown -->
      <div class="card">
        <div class="card-header">
          <span class="card-title">代码目录变更分布</span>
          <span class="card-subtitle">{{ topDirs.length }} 个目录受影响</span>
        </div>
        <div class="dir-list">
          <div v-for="dir in topDirs" :key="dir.directory" class="dir-row">
            <div class="dir-name">{{ dir.directory }}</div>
            <div class="dir-bar-track">
              <div class="dir-bar-fill" :style="{ width: dirBar(dir) }"></div>
            </div>
            <div class="dir-stats">
              <span v-if="dir.added" class="dir-stat-add">+{{ dir.added }}</span>
              <span v-if="dir.removed" class="dir-stat-del">−{{ dir.removed }}</span>
              <span v-if="dir.modified" class="dir-stat-mod">~{{ dir.modified }}</span>
              <span class="dir-stat-files">{{ dir.file_count }} files</span>
            </div>
          </div>
        </div>
      </div>

      <!-- Severity & Category -->
      <div class="card">
        <div class="card-header">
          <span class="card-title">风险分级</span>
          <span class="card-subtitle">L2 规则引擎分析结果</span>
        </div>

        <!-- Severity bars -->
        <div v-if="data.l2_analysis.total_matches > 0" class="sev-section">
          <div class="sev-label">严重程度分布</div>
          <div class="sev-bars">
            <div v-for="s in sevStats" :key="s.severity" class="sev-row">
              <span class="sev-name" :style="{ color: sevColors[s.severity] }">{{ s.severity }}</span>
              <div class="sev-track">
                <div
                  class="sev-fill"
                  :style="{
                    width: ((s.count / data.l2_analysis.total_matches) * 100).toFixed(0) + '%',
                    background: sevColors[s.severity],
                  }"
                ></div>
              </div>
              <span class="sev-count">{{ s.count }}</span>
            </div>
          </div>
        </div>

        <!-- Categories -->
        <div v-if="catStats.length > 0" class="cat-section">
          <div class="sev-label">规则类别</div>
          <div class="cat-tags">
            <span
              v-for="cat in catStats"
              :key="cat.category"
              class="cat-tag"
            >
              {{ cat.category }}
              <span class="cat-count">{{ cat.count }}</span>
            </span>
          </div>
        </div>

        <div v-if="data.l2_analysis.total_matches === 0" class="empty-state">
          <div class="empty-state-icon">✓</div>
          <h3>未命中安全/破坏性规则</h3>
          <p>所有行为变更通过了确定性规则检查</p>
        </div>
      </div>
    </div>

    <!-- ── Behavior Changes Table ── -->
    <div class="card">
      <div class="card-header">
        <span class="card-title">行为变更明细</span>
        <span class="card-subtitle">{{ data.l1_analysis.total_changes }} 项变更</span>
      </div>

      <div class="table-wrap">
        <table class="data-table">
          <thead>
            <tr>
              <th>符号</th>
              <th>文件</th>
              <th>变更类型</th>
              <th>置信度</th>
              <th>规则</th>
              <th>状态</th>
            </tr>
          </thead>
          <tbody>
            <tr v-for="change in data.l1_analysis.changes" :key="change.id">
              <td>
                <code class="symbol-code">{{ change.symbol }}</code>
              </td>
              <td class="file-cell" :title="change.file">{{ shortFile(change.file) }}</td>
              <td>
                <span :class="`badge ${changeTypeClass(change.change_type)}`">
                  {{ changeTypeLabel(change.change_type) }}
                </span>
              </td>
              <td>
                <div class="conf-cell">
                  <div class="conf-bar-track">
                    <div
                      class="conf-bar-fill"
                      :style="{
                        width: (change.confidence * 100).toFixed(0) + '%',
                        background: confColor(change.confidence),
                      }"
                    ></div>
                  </div>
                  <span class="conf-num">{{ (change.confidence * 100).toFixed(0) }}%</span>
                </div>
              </td>
              <td>
                <span
                  v-for="rid in change.rules"
                  :key="rid"
                  class="tag"
                  :style="{ color: 'var(--purple)', background: 'rgba(163,113,247,0.1)' }"
                >{{ rid }}</span>
              </td>
              <td>
                <span v-if="isSuppressed(change)" class="badge badge-purple">
                  {{ suppressionLabel(change) }}
                </span>
                <span v-else-if="change.rules.length > 0" class="badge badge-warning">需关注</span>
                <span v-else class="badge badge-success">正常</span>
              </td>
            </tr>
          </tbody>
        </table>
      </div>
    </div>
  </div>
</template>

<style scoped>
.two-col { display: grid; grid-template-columns: 1fr 1fr; gap: 16px; margin-bottom: 16px; }
@media (max-width: 900px) { .two-col { grid-template-columns: 1fr; } }

/* ── Directory List ── */
.dir-list { display: flex; flex-direction: column; gap: 8px; }
.dir-row { display: flex; align-items: center; gap: 8px; }
.dir-name { font-size: 12px; font-family: var(--font-mono); color: var(--text); min-width: 120px; white-space: nowrap; }
.dir-bar-track { flex: 1; height: 6px; background: var(--bg); border-radius: 3px; overflow: hidden; }
.dir-bar-fill { height: 100%; background: var(--accent); border-radius: 3px; min-width: 2px; transition: width 0.4s ease; }
.dir-stats { display: flex; gap: 6px; font-size: 11px; font-family: var(--font-mono); white-space: nowrap; }
.dir-stat-add { color: var(--success); }
.dir-stat-del { color: var(--danger); }
.dir-stat-mod { color: var(--warning); }
.dir-stat-files { color: var(--text-tertiary); }

/* ── Severity ── */
.sev-section { margin-bottom: 16px; }
.sev-label { font-size: 11px; font-weight: 600; color: var(--text-secondary); text-transform: uppercase; letter-spacing: 0.4px; margin-bottom: 8px; }
.sev-bars { display: flex; flex-direction: column; gap: 6px; }
.sev-row { display: flex; align-items: center; gap: 8px; }
.sev-name { font-size: 11px; font-weight: 600; min-width: 56px; }
.sev-track { flex: 1; height: 6px; background: var(--bg); border-radius: 3px; overflow: hidden; }
.sev-fill { height: 100%; border-radius: 3px; min-width: 2px; transition: width 0.4s ease; }
.sev-count { font-size: 11px; color: var(--text-secondary); font-family: var(--font-mono); min-width: 24px; text-align: right; }

/* ── Categories ── */
.cat-section { }
.cat-tags { display: flex; flex-wrap: wrap; gap: 6px; }
.cat-tag {
  display: inline-flex; align-items: center; gap: 6px;
  padding: 5px 12px; border-radius: var(--radius-sm);
  background: var(--bg); border: 1px solid var(--border);
  font-size: 12px; color: var(--text); font-weight: 500;
}
.cat-count { font-size: 10px; color: var(--accent); font-weight: 700; }

/* ── Table ── */
.table-wrap { overflow-x: auto; }
.symbol-code { font-size: 12px; font-family: var(--font-mono); color: var(--accent-emphasis); }
.file-cell { font-size: 12px; font-family: var(--font-mono); color: var(--text-secondary); max-width: 200px; overflow: hidden; text-overflow: ellipsis; }

/* ── Confidence cell ── */
.conf-cell { display: flex; align-items: center; gap: 8px; }
.conf-bar-track { width: 60px; height: 6px; background: var(--bg); border-radius: 3px; overflow: hidden; }
.conf-bar-fill { height: 100%; border-radius: 3px; min-width: 2px; }
.conf-num { font-size: 11px; color: var(--text-secondary); font-family: var(--font-mono); }
</style>
