<script setup>
import { ref, computed, onMounted, onBeforeUnmount } from 'vue'

const props = defineProps({
  thoughts: Object,
  reasons: Object,
  activeUsed: Object,
  maxNodes: { type: Number, default: 60 },
})

const tf = ref({ x: 0, y: 0, k: 1 })
const drag = ref(null)
const containerRef = ref(null)
const cw = ref(380)
const ch = ref(300)
let ro = null

// Cap to top-N by heat; sort descending so hottest nodes get first ring positions
const sortedVisible = computed(() => {
  const all = Object.values(props.thoughts || {})
  const sorted = [...all].sort((a, b) => (b.heat || 0) - (a.heat || 0))
  return sorted.slice(0, props.maxNodes)
})

const visibleIds = computed(() => new Set(sortedVisible.value.map(t => t.id)))

const visibleReasons = computed(() =>
  Object.values(props.reasons || {}).filter(r => visibleIds.value.has(r.from) && visibleIds.value.has(r.to))
)

const maxHeat = computed(() => {
  const vals = sortedVisible.value.map(t => t.heat || 0)
  return Math.max(1, ...vals)
})

// Radius scales with heat: cool=5.5, hot=11. Precomputed to avoid double-call per node in template.
const nodeRadii = computed(() => {
  const max = maxHeat.value
  const out = {}
  for (const t of sortedVisible.value) out[t.id] = 5.5 + ((t.heat || 0) / max) * 5.5
  return out
})

const pts = computed(() => {
  const nodes = sortedVisible.value
  const N = nodes.length
  if (!N) return {}
  const W = cw.value, H = ch.value
  const cx = W / 2, cy = H / 2
  const R = Math.min(W, H) * 0.38
  const result = {}
  nodes.forEach((t, i) => {
    const a = (i / N) * Math.PI * 2 - Math.PI / 2
    result[t.id] = {
      x: cx + Math.cos(a) * R,
      y: cy + Math.sin(a) * R,
    }
  })
  return result
})

const totalNodes = computed(() => Object.keys(props.thoughts || {}).length)
const showing = computed(() => visibleIds.value.size)

function nodeLabel(id) {
  const tx = (props.thoughts || {})[id]?.text || ''
  return tx.length > 18 ? tx.slice(0, 17) + '…' : tx || id.slice(0, 8)
}

const splitEdges = computed(() => {
  const active = [], inactive = []
  for (const r of visibleReasons.value) {
    if (props.activeUsed?.has(r.from) && props.activeUsed?.has(r.to)) active.push(r)
    else inactive.push(r)
  }
  return { active, inactive }
})

function down(e) {
  drag.value = { x: e.clientX, y: e.clientY, ox: tf.value.x, oy: tf.value.y }
  e.currentTarget.classList.add('drag')
}
function move(e) {
  if (!drag.value) return
  tf.value = { ...tf.value, x: drag.value.ox + (e.clientX - drag.value.x), y: drag.value.oy + (e.clientY - drag.value.y) }
}
function up(e) {
  drag.value = null
  e.currentTarget.classList.remove('drag')
}
function wheel(e) {
  e.preventDefault()
  tf.value = { ...tf.value, k: Math.max(0.3, Math.min(3, tf.value.k * (e.deltaY < 0 ? 1.1 : 0.9))) }
}

onMounted(() => {
  ro = new ResizeObserver(entries => {
    for (const e of entries) {
      cw.value = Math.max(120, e.contentRect.width)
      ch.value = Math.max(80, e.contentRect.height)
    }
  })
  if (containerRef.value) ro.observe(containerRef.value)
})
onBeforeUnmount(() => { if (ro) ro.disconnect() })
</script>

<template>
  <div class="graph" ref="containerRef"
    @pointerdown="down" @pointermove="move" @pointerup="up" @pointerleave="up" @wheel.prevent="wheel">
    <svg :viewBox="`0 0 ${cw} ${ch}`" preserveAspectRatio="xMidYMid meet">
      <g :transform="`translate(${tf.x},${tf.y}) scale(${tf.k})`">
        <!-- Inactive edges first (behind nodes), active edges on top -->
        <line v-for="r in splitEdges.inactive" :key="r.id"
          class="ge"
          :x1="pts[r.from]?.x" :y1="pts[r.from]?.y"
          :x2="pts[r.to]?.x" :y2="pts[r.to]?.y"
        />
        <line v-for="r in splitEdges.active" :key="r.id"
          class="ge used"
          :x1="pts[r.from]?.x" :y1="pts[r.from]?.y"
          :x2="pts[r.to]?.x" :y2="pts[r.to]?.y"
        />
        <g v-for="id in visibleIds" :key="id"
          :class="['gn', activeUsed?.has(id) ? 'used' : '']"
          :transform="`translate(${pts[id]?.x ?? 0},${pts[id]?.y ?? 0})`"
        >
          <title>{{ thoughts?.[id]?.text || id }}</title>
          <circle :r="nodeRadii[id] ?? 6" />
          <text text-anchor="middle" :y="`-${(nodeRadii[id] ?? 6) + 4}`">{{ nodeLabel(id) }}</text>
        </g>
      </g>
    </svg>
    <div v-if="!totalNodes" class="graph-empty">no memory nodes yet</div>
    <span class="graph-hint">
      {{ showing < totalNodes ? `top ${showing} of ${totalNodes}` : `${totalNodes} node${totalNodes===1?'':'s'}` }}
      · drag · scroll to zoom
    </span>
  </div>
</template>
