<script setup lang="ts">
import { computed } from 'vue'
import type { FeedbackLoopData, FeedbackChange } from '../types'

const props = defineProps<{ data: FeedbackLoopData }>()

const suppressedChanges = computed(() =>
  props.data.changes.filter(c => c.suppressed != null)
)

const warnedChanges = computed(() =>
  props.data.changes.filter(c => c.tags.some(t => t.includes('fp-history')))
)

const activeRules = computed(() =>
  props.data.suppressions.filter(s => s.status === 'active')
)

const exactCount = computed(() =>
  suppressedChanges.value.filter(c => c.suppressed?.level === 'Exact').length
)
const ruleCount = computed(() =>
  suppressedChanges.value.filter(c => c.suppressed?.level === 'Rule').length
)
const warnCount = computed(() =>
  suppressedChanges.value.filter(c => c.suppressed?.level === 'Warning').length + warnedChanges.value.length
)

function suppressionReason(c: FeedbackChange): string {
  if (c.suppressed) return c.suppressed.reason
  return '文件路径与历史拒绝记录匹配'
}
function shortFile(f: string): string {
  const parts = f.replace(/\\/g, '/').split('/')
  return parts.length <= 2 ? f : '…/' + parts.slice(-2).join('/')
}
</script>

<template>
  <div v-if="data.loaded">
    <!-- ── Overview ── -->
    <div class="stat-grid">
      <div class="stat-card">
        <div class="stat-value" style="color:var(--purple)">{{ data.exact_reject_count }}</div>
        <div class="stat-label">精确拒绝记录</div>
        <div class="stat-desc">人类标记为误报的变更</div>
      </div>
      <div class="stat-card">
        <div class="stat-value" style="color:var(--accent)">{{ data.rule_suppression_count }}</div>
        <div class="stat-label">规则级抑制</div>
        <div class="stat-desc">active: {{ activeRules.length }}</div>
      </div>
      <div class="stat-card">
        <div class="stat-value" style="color:var(--warning)">{{ exactCount }}</div>
        <div class="stat-label">本次精确命中</div>
        <div class="stat-desc">Layer 1: 完全相同变更被抑制</div>
      </div>
      <div class="stat-card">
        <div class="stat-value" style="color:var(--accent)">{{ ruleCount }}</div>
        <div class="stat-label">本次规则命中</div>
        <div class="stat-desc">Layer 2: 规则模式匹配</div>
      </div>
    </div>

    <!-- ── Suppression Layers Explanation ── -->
    <div class="card">
      <div class="card-header">
        <span class="card-title">三层抑制机制</span>
        <span class="card-subtitle">{{ suppressedChanges.length }} 项变更被抑制，{{ warnCount }} 项收到警告</span>
      </div>
      <div class="layers-visual">
        <div class="layer-row">
          <div class="layer-badge layer-l1">L1</div>
          <div class="layer-body">
            <div class="layer-title">精确匹配抑制</div>
            <div class="layer-desc">(符号, 文件, 变更类型) 三元组精确匹配历史 reject 记录 → 置信度降至 0.2</div>
          </div>
          <div class="layer-count">{{ exactCount }}</div>
        </div>
        <div class="layer-arrow">↓ 未命中则进入</div>
        <div class="layer-row">
          <div class="layer-badge layer-l2">L2</div>
          <div class="layer-body">
            <div class="layer-title">规则级抑制</div>
            <div class="layer-desc">(规则ID, 文件glob) 匹配 suppressions.toml → 置信度降至 0.2（需人类审批）</div>
          </div>
          <div class="layer-count">{{ ruleCount }}</div>
        </div>
        <div class="layer-arrow">↓ 未命中则进入</div>
        <div class="layer-row">
          <div class="layer-badge layer-l3">L3</div>
          <div class="layer-body">
            <div class="layer-title">文件前缀警告</div>
            <div class="layer-desc">文件路径部分匹配拒绝历史 → 仅打标签，不改变置信度</div>
          </div>
          <div class="layer-count">{{ warnCount }}</div>
        </div>
      </div>
    </div>

    <!-- ── Suppressed Changes Table ── -->
    <div v-if="suppressedChanges.length > 0" class="card">
      <div class="card-header">
        <span class="card-title">本次被抑制的变更</span>
        <span class="card-subtitle">自动应用历史反馈决策</span>
      </div>
      <div class="table-wrap">
        <table class="data-table">
          <thead>
            <tr><th>符号</th><th>文件</th><th>抑制层</th><th>原因</th></tr>
          </thead>
          <tbody>
            <tr v-for="c in suppressedChanges" :key="c.id">
              <td><code class="symbol-code">{{ c.symbol }}</code></td>
              <td class="file-cell" :title="c.file">{{ shortFile(c.file) }}</td>
              <td>
                <span class="badge" :class="c.suppressed?.level === 'Exact' ? 'badge-purple' : c.suppressed?.level === 'Rule' ? 'badge-info' : 'badge-warning'">
                  {{ c.suppressed?.level === 'Exact' ? '精确匹配' : c.suppressed?.level === 'Rule' ? '规则抑制' : '警告' }}
                </span>
              </td>
              <td style="font-size:12px;color:var(--text-secondary);max-width:300px">{{ suppressionReason(c) }}</td>
            </tr>
          </tbody>
        </table>
      </div>
    </div>

    <!-- ── Active Suppression Rules ── -->
    <div v-if="activeRules.length > 0" class="card">
      <div class="card-header">
        <span class="card-title">活跃的抑制规则</span>
        <span class="card-subtitle">suppressions.toml</span>
      </div>
      <div class="table-wrap">
        <table class="data-table">
          <thead>
            <tr><th>规则ID</th><th>文件模式</th><th>效果</th><th>命中次数</th><th>原因</th></tr>
          </thead>
          <tbody>
            <tr v-for="r in activeRules" :key="r.rule_id">
              <td><code style="color:var(--purple);font-size:12px">{{ r.rule_id }}</code></td>
              <td><code style="font-size:11px;color:var(--text-secondary)">{{ r.file_pattern }}</code></td>
              <td>
                <span class="badge" :class="r.effect === 'suppress' ? 'badge-purple' : 'badge-warning'">
                  {{ r.effect === 'suppress' ? '抑制' : '提醒' }}
                </span>
              </td>
              <td style="font-family:var(--font-mono);font-size:12px">{{ r.hit_count }}</td>
              <td style="font-size:12px;color:var(--text-secondary);max-width:300px">{{ r.reason }}</td>
            </tr>
          </tbody>
        </table>
      </div>
    </div>
  </div>

  <!-- No feedback data -->
  <div v-else class="empty-state">
    <div class="empty-state-icon">↻</div>
    <h3>反馈回路未激活</h3>
    <p>运行 <code>paporot feedback generate</code> 创建审查文件，编辑 TOML 后 <code>paporot feedback apply</code> 启用反馈回路。</p>
    <p style="margin-top:8px;font-size:12px">启用后，人类审批的决策将自动应用到后续分析中，抑制已知误报。</p>
  </div>
</template>

<style scoped>
.stat-desc { font-size: 11px; color: var(--text-tertiary); margin-top: 2px; }

/* ── Layers visual ── */
.layers-visual { display: flex; flex-direction: column; gap: 2px; }
.layer-row {
  display: flex; align-items: center; gap: 12px;
  padding: 12px; border-radius: var(--radius-sm);
  background: var(--bg);
  border: 1px solid var(--border);
}
.layer-badge {
  width: 36px; height: 36px; border-radius: 50%;
  display: flex; align-items: center; justify-content: center;
  font-size: 12px; font-weight: 700; flex-shrink: 0;
}
.layer-l1 { background: rgba(163,113,247,0.15); color: var(--purple); border: 2px solid rgba(163,113,247,0.3); }
.layer-l2 { background: rgba(88,166,255,0.15); color: var(--accent); border: 2px solid rgba(88,166,255,0.3); }
.layer-l3 { background: rgba(210,153,34,0.1); color: var(--warning); border: 2px solid rgba(210,153,34,0.2); }
.layer-body { flex: 1; }
.layer-title { font-size: 13px; font-weight: 600; margin-bottom: 2px; }
.layer-desc { font-size: 11px; color: var(--text-secondary); }
.layer-count { font-size: 20px; font-weight: 700; color: var(--text); font-family: var(--font-mono); }
.layer-arrow { text-align: center; font-size: 12px; color: var(--text-tertiary); padding: 2px 0; }

.symbol-code { font-size: 12px; font-family: var(--font-mono); color: var(--accent-emphasis); }
.file-cell { font-size: 12px; font-family: var(--font-mono); color: var(--text-secondary); max-width: 200px; overflow: hidden; text-overflow: ellipsis; }
.table-wrap { overflow-x: auto; }
</style>
