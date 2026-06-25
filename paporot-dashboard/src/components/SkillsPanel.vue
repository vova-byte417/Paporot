<script setup lang="ts">
import { ref } from 'vue'
import type { SkillSummary } from '../types'

defineProps<{ skills: SkillSummary[] }>()

const expanded = ref<Record<string, boolean>>({})

function toggle(name: string) {
  expanded.value[name] = !expanded.value[name]
}

function formatOutput(output: string): string {
  if (!output) return ''
  // Try to extract key info from JSON output
  try {
    const parsed = JSON.parse(output)
    // Return a human-readable summary
    const parts: string[] = []
    if (parsed.project_name) parts.push(`项目: ${parsed.project_name}`)
    if (parsed.purpose) parts.push(`用途: ${parsed.purpose}`)
    if (parsed.languages && Array.isArray(parsed.languages)) parts.push(`语言: ${parsed.languages.join(', ')}`)
    if (parsed.frameworks && Array.isArray(parsed.frameworks)) parts.push(`框架: ${parsed.frameworks.join(', ')}`)
    if (parsed.modules && Array.isArray(parsed.modules)) parts.push(`发现 ${parsed.modules.length} 个模块`)
    if (parsed.flows && Array.isArray(parsed.flows)) parts.push(`追踪到 ${parsed.flows.length} 条执行路径`)
    if (parsed.dependencies && Array.isArray(parsed.dependencies)) parts.push(`分析 ${parsed.dependencies.length} 条依赖关系`)
    if (parsed.module_count) parts.push(`模块数: ${parsed.module_count}`)
    if (parsed.total_dependencies) parts.push(`依赖数: ${parsed.total_dependencies}`)
    if (parsed.flow_count) parts.push(`执行路径: ${parsed.flow_count}`)
    if (parsed.sections && Array.isArray(parsed.sections)) parts.push(`生成 ${parsed.sections.length} 个章节`)
    if (parts.length === 0) {
      // Show first few keys
      const keys = Object.keys(parsed).slice(0, 3).map(k => `${k}: ${typeof parsed[k] === 'object' ? '...' : String(parsed[k]).slice(0, 40)}`)
      return keys.join(' · ')
    }
    return parts.join(' · ')
  } catch {
    return output.slice(0, 200)
  }
}

function skillIcon(name: string): string {
  if (name.includes('repository')) return '⬡'
  if (name.includes('module')) return '⊞'
  if (name.includes('dependency')) return '⟐'
  if (name.includes('runtime') || name.includes('flow')) return '↗'
  if (name.includes('boundary')) return '⟐'
  if (name.includes('architecture')) return '⬡'
  return '◆'
}
</script>

<template>
  <div class="card" style="margin-top:24px">
    <div class="card-header">
      <span class="card-title">分析管线输出</span>
      <span class="card-subtitle">{{ skills.length }} 个 Skill · {{ skills.filter(s => s.status === 'ok').length }} 完成分析</span>
    </div>

    <div class="skills-list">
      <div
        v-for="s in skills"
        :key="s.name"
        :class="['skill-item', s.status]"
        @click="toggle(s.name)"
      >
        <div class="skill-head">
          <div class="skill-info">
            <span class="skill-icon">{{ skillIcon(s.name) }}</span>
            <div>
              <div class="skill-name">{{ s.name }}</div>
              <div class="skill-result" v-if="s.output_summary">
                {{ formatOutput(s.output_summary) }}
              </div>
              <div class="skill-result" v-else-if="s.status === 'skipped'" style="color:var(--text-tertiary)">
                上游依赖未满足，跳过执行
              </div>
              <div class="skill-result" v-else-if="s.status === 'failed' && s.error" style="color:var(--danger)">
                错误: {{ s.error }}
              </div>
            </div>
          </div>
          <div class="skill-meta">
            <span :class="['badge', s.status === 'ok' ? 'badge-success' : s.status === 'failed' ? 'badge-danger' : 'badge-info']">
              {{ s.status === 'ok' ? '完成' : s.status === 'failed' ? '失败' : '跳过' }}
            </span>
            <span class="skill-dur">{{ s.duration_ms }}ms</span>
            <span class="skill-expand">{{ expanded[s.name] ? '▾' : '▸' }}</span>
          </div>
        </div>

        <!-- Expanded detail -->
        <div v-if="expanded[s.name] && s.output_summary" class="skill-detail">
          <pre class="skill-pre">{{ s.output_summary }}</pre>
        </div>
        <div v-if="expanded[s.name] && s.error" class="skill-detail">
          <pre class="skill-pre skill-error">{{ s.error }}</pre>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.skills-list { display: flex; flex-direction: column; gap: 4px; }
.skill-item {
  padding: 12px 16px; border-radius: var(--radius-sm);
  background: var(--bg); border: 1px solid var(--border);
  cursor: pointer; transition: border-color 0.15s, background 0.15s;
}
.skill-item:hover { border-color: var(--border-emphasis); }
.skill-item.ok { border-left: 3px solid var(--success); }
.skill-item.failed { border-left: 3px solid var(--danger); }
.skill-item.skipped { border-left: 3px solid var(--text-tertiary); opacity: 0.7; }

.skill-head { display: flex; justify-content: space-between; align-items: center; gap: 12px; }
.skill-info { display: flex; align-items: flex-start; gap: 10px; flex: 1; min-width: 0; }
.skill-icon { font-size: 16px; flex-shrink: 0; color: var(--accent); margin-top: 1px; }
.skill-name { font-size: 13px; font-weight: 600; font-family: var(--font-mono); }
.skill-result { font-size: 12px; color: var(--text-secondary); margin-top: 2px; line-height: 1.5; }
.skill-meta { display: flex; align-items: center; gap: 10px; flex-shrink: 0; }
.skill-dur { font-size: 11px; color: var(--text-tertiary); font-family: var(--font-mono); }
.skill-expand { font-size: 12px; color: var(--text-tertiary); }

.skill-detail { margin-top: 12px; padding-top: 12px; border-top: 1px solid var(--border); }
.skill-pre {
  font-family: var(--font-mono); font-size: 11px; color: var(--text-secondary);
  white-space: pre-wrap; word-break: break-all; max-height: 400px; overflow-y: auto;
  background: rgba(0,0,0,0.2); padding: 12px; border-radius: var(--radius-sm);
  margin: 0;
}
.skill-error { color: var(--danger); background: rgba(248,81,73,0.05); }
</style>
