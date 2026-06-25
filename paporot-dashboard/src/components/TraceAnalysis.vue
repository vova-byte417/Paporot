<script setup lang="ts">
import { computed } from 'vue'
import type { TraceAssociation, TraceMatch, TrajectoryAnalysis } from '../types'

const props = defineProps<{ trace: TraceAssociation }>()

const matches = computed(() => props.trace.matched_traces)
const traj = computed(() => props.trace.trajectory_analysis)

const hasTraj = computed(() => traj.value != null)

function matchLevelLabel(level: string): string {
  switch (level) {
    case 'commit': return 'Git Commit 精确匹配'
    case 'file_overlap': return '文件重叠度匹配'
    case 'time_window': return '时间窗口匹配'
    default: return level
  }
}
function matchLevelBadge(level: string): string {
  switch (level) {
    case 'commit': return 'badge-success'
    case 'file_overlap': return 'badge-warning'
    case 'time_window': return 'badge-info'
    default: return 'badge-info'
  }
}
function scoreColor(score: number): string {
  if (score <= 0.2) return 'var(--success)'
  if (score <= 0.5) return 'var(--warning)'
  return 'var(--danger)'
}
function scoreLabel(score: number): string {
  if (score <= 0.2) return '稳定'
  if (score <= 0.5) return '轻微波动'
  return '显著变化'
}

const evalResult = computed(() => traj.value?.evaluation)
</script>

<template>
  <div v-if="matches.length > 0">
    <!-- ── Overview ── -->
    <div class="stat-grid">
      <div class="stat-card">
        <div class="stat-value" style="color:var(--accent)">{{ matches.length }}</div>
        <div class="stat-label">匹配的 Agent Trace</div>
      </div>
      <div v-if="hasTraj" class="stat-card">
        <div :class="['stat-value']" :style="{ color: scoreColor(traj!.tool_churn_score) }">
          {{ (traj!.tool_churn_score * 100).toFixed(0) }}%
        </div>
        <div class="stat-label">工具变更率</div>
        <div class="stat-desc">{{ scoreLabel(traj!.tool_churn_score) }}</div>
      </div>
      <div v-if="hasTraj" class="stat-card">
        <div :class="['stat-value']" :style="{ color: scoreColor(traj!.phase_reorder_score) }">
          {{ (traj!.phase_reorder_score * 100).toFixed(0) }}%
        </div>
        <div class="stat-label">阶段重排率</div>
        <div class="stat-desc">{{ scoreLabel(traj!.phase_reorder_score) }}</div>
      </div>
      <div v-if="hasTraj" class="stat-card">
        <div :class="['stat-value']" :style="{ color: scoreColor(traj!.capability_shift_score) }">
          {{ (traj!.capability_shift_score * 100).toFixed(0) }}%
        </div>
        <div class="stat-label">能力漂移率</div>
        <div class="stat-desc">{{ scoreLabel(traj!.capability_shift_score) }}</div>
      </div>
    </div>

    <!-- ── Trace Matches ── -->
    <div class="card">
      <div class="card-header">
        <span class="card-title">Agent Trace 快照关联</span>
        <span class="card-subtitle">三级自动匹配算法</span>
      </div>
      <div class="table-wrap">
        <table class="data-table">
          <thead>
            <tr>
              <th>Trace ID</th>
              <th>匹配方式</th>
              <th>置信度</th>
              <th>平台</th>
              <th>工具调用</th>
              <th>耗时</th>
            </tr>
          </thead>
          <tbody>
            <tr v-for="m in matches" :key="m.trace_id">
              <td><code class="symbol-code">{{ m.trace_id }}</code></td>
              <td>
                <span :class="`badge ${matchLevelBadge(m.match_level)}`">
                  {{ matchLevelLabel(m.match_level) }}
                </span>
              </td>
              <td>
                <div class="match-conf">
                  <div class="mini-bar-track">
                    <div
                      class="mini-bar-fill"
                      :style="{ width: (m.confidence * 100).toFixed(0) + '%' }"
                    ></div>
                  </div>
                  <span class="mini-num">{{ (m.confidence * 100).toFixed(0) }}%</span>
                </div>
              </td>
              <td style="font-size:12px;color:var(--text-secondary)">{{ m.agent_platform ?? 'Unknown' }}</td>
              <td style="font-family:var(--font-mono);font-size:12px">{{ m.tool_count ?? '—' }}</td>
              <td style="font-family:var(--font-mono);font-size:12px;color:var(--text-secondary)">{{ m.duration_secs ? m.duration_secs + 's' : '—' }}</td>
            </tr>
          </tbody>
        </table>
      </div>
    </div>

    <!-- ── Trajectory Analysis ── -->
    <div v-if="hasTraj" class="card">
      <div class="card-header">
        <span class="card-title">Agent 行为趋势分析</span>
        <span class="card-subtitle">工具使用模式变化</span>
      </div>

      <div class="traj-grid">
        <div class="traj-col">
          <h4 class="traj-col-title">工具使用对比</h4>
          <div class="traj-compare">
            <div class="traj-compare-item">
              <div class="traj-compare-label">上一版本</div>
              <div class="traj-compare-value">{{ traj!.tool_count_a }}</div>
            </div>
            <div class="traj-compare-arrow">→</div>
            <div class="traj-compare-item">
              <div class="traj-compare-label">当前版本</div>
              <div :class="['traj-compare-value', traj!.tool_count_b > traj!.tool_count_a ? 'inc' : traj!.tool_count_b < traj!.tool_count_a ? 'dec' : '']">
                {{ traj!.tool_count_b }}
              </div>
            </div>
          </div>
          <div class="traj-detail">
            <div class="traj-detail-row">
              <span>共享工具</span>
              <span class="traj-num">{{ traj!.shared_tool_count }}</span>
            </div>
            <div class="traj-detail-row">
              <span>新增工具</span>
              <span class="traj-num" style="color:var(--success)">+{{ traj!.added_tool_count }}</span>
            </div>
            <div class="traj-detail-row">
              <span>移除工具</span>
              <span class="traj-num" style="color:var(--danger)">−{{ traj!.deleted_tool_count }}</span>
            </div>
          </div>
        </div>

        <!-- Evaluation -->
        <div v-if="evalResult" class="traj-col">
          <h4 class="traj-col-title">行为评估</h4>
          <div :class="['eval-card', evalResult.verdict]">
            <div class="eval-verdict">
              {{ evalResult.verdict === 'improved' ? '↑ 改进' : evalResult.verdict === 'degraded' ? '↓ 退化' : '→ 持平' }}
            </div>
            <div class="eval-score">评分: {{ (evalResult.score * 100).toFixed(0) }}/100</div>
          </div>
          <ul v-if="evalResult.details.length" class="eval-details">
            <li v-for="d in evalResult.details" :key="d">{{ d }}</li>
          </ul>
        </div>
      </div>
    </div>
  </div>

  <div v-else class="empty-state">
    <div class="empty-state-icon">◉</div>
    <h3>暂无 Agent Trace 关联</h3>
    <p>导入 Agent 执行轨迹以启用行为趋势分析。</p>
    <p style="font-size:12px;margin-top:8px">支持 Claude / OpenAI / DeepSeek 平台的 trace 数据。</p>
  </div>
</template>

<style scoped>
.stat-desc { font-size: 11px; color: var(--text-tertiary); margin-top: 2px; }
.symbol-code { font-size: 12px; font-family: var(--font-mono); color: var(--accent-emphasis); }
.table-wrap { overflow-x: auto; }

.match-conf { display: flex; align-items: center; gap: 8px; }
.mini-bar-track { width: 50px; height: 5px; background: var(--bg); border-radius: 3px; overflow: hidden; }
.mini-bar-fill { height: 100%; background: var(--accent); border-radius: 3px; min-width: 2px; }
.mini-num { font-size: 11px; color: var(--text-secondary); font-family: var(--font-mono); }

/* ── Trajectory grid ── */
.traj-grid { display: grid; grid-template-columns: 1fr 1fr; gap: 24px; }
@media (max-width: 700px) { .traj-grid { grid-template-columns: 1fr; } }
.traj-col { }
.traj-col-title { font-size: 13px; font-weight: 600; margin-bottom: 12px; color: var(--text-secondary); text-transform: uppercase; letter-spacing: 0.4px; }
.traj-compare { display: flex; align-items: center; gap: 12px; margin-bottom: 16px; }
.traj-compare-item { text-align: center; }
.traj-compare-label { font-size: 10px; color: var(--text-tertiary); text-transform: uppercase; }
.traj-compare-value { font-size: 36px; font-weight: 700; font-family: var(--font-mono); color: var(--text); }
.traj-compare-value.inc { color: var(--success); }
.traj-compare-value.dec { color: var(--danger); }
.traj-compare-arrow { font-size: 20px; color: var(--text-tertiary); }
.traj-detail { display: flex; flex-direction: column; gap: 6px; }
.traj-detail-row { display: flex; justify-content: space-between; font-size: 13px; }
.traj-num { font-family: var(--font-mono); font-weight: 600; }

/* ── Evaluation ── */
.eval-card {
  padding: 20px; border-radius: var(--radius);
  text-align: center; margin-bottom: 12px;
}
.eval-card.improved { background: rgba(63,185,80,0.08); border: 1px solid rgba(63,185,80,0.25); }
.eval-card.degraded { background: rgba(248,81,73,0.08); border: 1px solid rgba(248,81,73,0.25); }
.eval-card.unchanged { background: rgba(139,148,158,0.06); border: 1px solid rgba(139,148,158,0.2); }
.eval-verdict { font-size: 20px; font-weight: 700; margin-bottom: 4px; }
.eval-card.improved .eval-verdict { color: var(--success); }
.eval-card.degraded .eval-verdict { color: var(--danger); }
.eval-card.unchanged .eval-verdict { color: var(--text-secondary); }
.eval-score { font-size: 13px; color: var(--text-secondary); font-family: var(--font-mono); }
.eval-details { list-style: none; font-size: 12px; color: var(--text-secondary); }
.eval-details li { padding: 3px 0; }
.eval-details li::before { content: '· '; color: var(--accent); }
</style>
