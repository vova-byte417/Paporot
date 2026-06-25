<script setup lang="ts">
import { computed } from 'vue'
import type { SnapshotOverview } from '../types'

const props = defineProps<{ snapshot: SnapshotOverview }>()

const diff = computed(() => props.snapshot.current_diff)

const hasDiff = computed(() =>
  diff.value != null &&
  (diff.value.added.length > 0 || diff.value.modified.length > 0 || diff.value.deleted.length > 0)
)

const sortedVersions = computed(() => {
  return [...props.snapshot.versions].reverse()
})

function statusBadge(status: string): string {
  switch (status) {
    case 'new': return 'badge-success'
    case 'modified': return 'badge-warning'
    case 'deleted': return 'badge-danger'
    default: return 'badge-info'
  }
}
</script>

<template>
  <div v-if="props.snapshot">
    <!-- ── Overview ── -->
    <div class="stat-grid">
      <div class="stat-card">
        <div class="stat-value" style="color:var(--accent)">{{ snapshot.version_count }}</div>
        <div class="stat-label">版本总数</div>
      </div>
      <div class="stat-card">
        <div class="stat-value" style="color:var(--success)">{{ snapshot.current_diff?.added.length ?? 0 }}</div>
        <div class="stat-label">新增能力</div>
      </div>
      <div class="stat-card">
        <div class="stat-value" style="color:var(--warning)">{{ snapshot.current_diff?.modified.length ?? 0 }}</div>
        <div class="stat-label">修改能力</div>
      </div>
      <div class="stat-card">
        <div class="stat-value" style="color:var(--danger)">{{ snapshot.current_diff?.deleted.length ?? 0 }}</div>
        <div class="stat-label">删除能力</div>
      </div>
    </div>

    <!-- ── Version Timeline ── -->
    <div class="card">
      <div class="card-header">
        <span class="card-title">版本时间线</span>
        <span class="card-subtitle">当前: {{ snapshot.current_version }}</span>
      </div>
      <div class="timeline">
        <div
          v-for="(v, i) in sortedVersions"
          :key="v.version_id"
          :class="['tl-item', { current: v.version_id === snapshot.current_version }]"
        >
          <div class="tl-dot"></div>
          <div v-if="i < sortedVersions.length - 1" class="tl-line"></div>
          <div class="tl-content">
            <div class="tl-header">
              <span class="tl-version">{{ v.version_id }}</span>
              <span v-if="v.version_id === snapshot.current_version" class="badge badge-info">当前</span>
            </div>
            <div class="tl-meta">
              <span>{{ v.created_at }}</span>
              <span v-if="v.git_commit">· <code>{{ v.git_commit.slice(0, 7) }}</code></span>
              <span>· {{ v.capability_count }} 能力</span>
            </div>
            <div v-if="v.message" class="tl-msg">{{ v.message }}</div>
          </div>
        </div>
      </div>
    </div>

    <!-- ── Current Diff ── -->
    <div v-if="hasDiff" class="card">
      <div class="card-header">
        <span class="card-title">能力变更对比 (vs 上一版本)</span>
        <span class="card-subtitle">{{ diff!.added.length }} 新增 · {{ diff!.modified.length }} 修改 · {{ diff!.deleted.length }} 删除</span>
      </div>

      <div class="diff-grid">
        <!-- Added -->
        <div v-if="diff!.added.length > 0" class="diff-col">
          <h4 class="diff-col-title" style="color:var(--success)">+ 新增能力 ({{ diff!.added.length }})</h4>
          <div v-for="cap in diff!.added" :key="cap.id" class="diff-card diff-added">
            <div class="diff-card-name">{{ cap.name }}</div>
            <div class="diff-card-meta">
              <span v-if="cap.module" class="diff-module">{{ cap.module }}</span>
              <span class="diff-conf" :style="{ color: cap.confidence >= 0.8 ? 'var(--success)' : 'var(--warning)' }">
                {{ (cap.confidence * 100).toFixed(0) }}% 置信
              </span>
            </div>
            <div v-if="cap.evidence.length" class="diff-evidence">{{ cap.evidence[0] }}</div>
            <div v-if="cap.categories.length" class="diff-tags">
              <span v-for="cat in cap.categories" :key="cat" class="tag" style="background:rgba(88,166,255,0.1);color:var(--accent)">{{ cat }}</span>
            </div>
          </div>
        </div>

        <!-- Modified -->
        <div v-if="diff!.modified.length > 0" class="diff-col">
          <h4 class="diff-col-title" style="color:var(--warning)">~ 修改能力 ({{ diff!.modified.length }})</h4>
          <div v-for="cap in diff!.modified" :key="cap.id" class="diff-card diff-modified">
            <div class="diff-card-name">{{ cap.name }}</div>
            <div class="diff-card-meta">
              <span v-if="cap.module" class="diff-module">{{ cap.module }}</span>
            </div>
            <div v-if="cap.evidence.length" class="diff-evidence">{{ cap.evidence[0] }}</div>
          </div>
        </div>

        <!-- Deleted -->
        <div v-if="diff!.deleted.length > 0" class="diff-col">
          <h4 class="diff-col-title" style="color:var(--danger)">− 删除能力 ({{ diff!.deleted.length }})</h4>
          <div v-for="cap in diff!.deleted" :key="cap.id" class="diff-card diff-deleted">
            <div class="diff-card-name">{{ cap.name }}</div>
            <div class="diff-card-meta">
              <span v-if="cap.module" class="diff-module">{{ cap.module }}</span>
            </div>
            <div v-if="cap.evidence.length" class="diff-evidence">{{ cap.evidence[0] }}</div>
          </div>
        </div>
      </div>
    </div>

    <div v-if="!hasDiff" class="card">
      <div class="empty-state">
        <div class="empty-state-icon">◷</div>
        <h3>仅有一个版本</h3>
        <p>运行多次 <code>paporot analyze</code> 积累版本历史后，将展示能力变更对比。</p>
      </div>
    </div>
  </div>

  <div v-else class="empty-state">
    <div class="empty-state-icon">◷</div>
    <h3>尚无版本快照</h3>
    <p>运行 <code>paporot analyze</code> 将自动创建行为快照，开启版本历史追踪。</p>
  </div>
</template>

<style scoped>
/* ── Timeline ── */
.timeline { position: relative; padding-left: 32px; }
.tl-item { position: relative; padding-bottom: 24px; }
.tl-item:last-child { padding-bottom: 0; }
.tl-dot {
  position: absolute; left: -27px; top: 4px;
  width: 12px; height: 12px; border-radius: 50%;
  background: var(--border); border: 2px solid var(--bg-elevated);
  z-index: 1;
}
.tl-item.current .tl-dot {
  background: var(--accent); border-color: var(--accent);
  box-shadow: 0 0 0 4px rgba(88,166,255,0.15);
}
.tl-line {
  position: absolute; left: -22px; top: 16px; bottom: 0;
  width: 2px; background: var(--border);
}
.tl-content {
  background: var(--bg);
  border: 1px solid var(--border);
  border-radius: var(--radius-sm);
  padding: 12px 16px;
}
.tl-item.current .tl-content { border-color: rgba(88,166,255,0.3); }
.tl-header { display: flex; align-items: center; gap: 8px; margin-bottom: 4px; }
.tl-version { font-size: 14px; font-weight: 700; font-family: var(--font-mono); }
.tl-meta { font-size: 11px; color: var(--text-secondary); display: flex; gap: 6px; align-items: center; }
.tl-meta code { font-size: 10px; color: var(--accent); background: rgba(88,166,255,0.1); padding: 1px 5px; border-radius: 3px; }
.tl-msg { font-size: 12px; color: var(--text-secondary); margin-top: 4px; }

/* ── Diff Grid ── */
.diff-grid { display: grid; grid-template-columns: repeat(auto-fit, minmax(300px, 1fr)); gap: 16px; }
.diff-col { display: flex; flex-direction: column; gap: 8px; }
.diff-col-title { font-size: 13px; font-weight: 600; margin-bottom: 4px; }
.diff-card {
  padding: 10px 14px; border-radius: var(--radius-sm);
  border-left: 3px solid;
}
.diff-added { background: rgba(63,185,80,0.05); border-color: var(--success); }
.diff-modified { background: rgba(210,153,34,0.05); border-color: var(--warning); }
.diff-deleted { background: rgba(248,81,73,0.05); border-color: var(--danger); }
.diff-card-name { font-size: 13px; font-weight: 600; }
.diff-card-meta { display: flex; gap: 8px; align-items: center; margin-top: 4px; }
.diff-module { font-size: 11px; color: var(--text-secondary); font-family: var(--font-mono); }
.diff-conf { font-size: 11px; font-family: var(--font-mono); }
.diff-evidence { font-size: 11px; color: var(--text-tertiary); font-family: var(--font-mono); margin-top: 4px; }
.diff-tags { margin-top: 6px; }
</style>
