<script setup>
import { ref, onMounted, onBeforeUnmount } from 'vue'
import ForceGraph from 'force-graph'

const stats = ref('loading…')
const err = ref('')
const intervalMs = ref(5000)
const graphEl = ref(null)

let G = null
let timer = null
// Persist node objects across refreshes so positions hold; only reset the
// layout when the topology changes (nodes/edges added or removed).
const byId = new Map()
let topoKey = ''

async function load() {
  try {
    const d = await (await fetch('/graph')).json()
    const incoming = new Set(d.nodes.map((n) => n.id))
    const nodes = d.nodes.map((n) => {
      const ex = byId.get(n.id)
      if (ex) {
        ex.label = n.label; ex.kind = n.kind; ex.kern = n.kern
        ex.heat = n.heat; ex.conf = n.conf
        return ex
      }
      byId.set(n.id, n)
      return n
    })
    for (const id of [...byId.keys()]) if (!incoming.has(id)) byId.delete(id)

    const key =
      [...incoming].sort().join() +
      '|' + d.links.map((l) => l.source + '>' + l.target).sort().join()
    if (key !== topoKey) {
      topoKey = key
      G.graphData({ nodes, links: d.links }) // only new nodes settle
    }
    stats.value = `${d.nodes.length} thoughts · ${d.links.length} reasons · ${d.kerns} kerns`
    err.value = ''
  } catch (e) {
    err.value = String(e)
  }
}

function restart() {
  if (timer) clearInterval(timer)
  if (intervalMs.value > 0) timer = setInterval(load, intervalMs.value)
}

onMounted(() => {
  G = ForceGraph()(graphEl.value)
    .backgroundColor('#0b0d10')
    .nodeLabel((n) => `${n.kind} · heat ${(+n.heat).toFixed(2)} · conf ${(+n.conf).toFixed(2)}\n${n.label}`)
    .nodeAutoColorBy('kern')
    .nodeRelSize(4)
    .nodeVal((n) => 1 + (+n.heat || 0) * 3)
    .linkColor(() => 'rgba(120,140,160,0.25)')
    .linkDirectionalArrowLength(2.5)
    .linkDirectionalArrowRelPos(1)
  load()
  restart()
})

onBeforeUnmount(() => {
  if (timer) clearInterval(timer)
})
</script>

<template>
  <div class="hud">
    <b>kern</b> graph · {{ stats }}
    <span v-if="err" class="err"> — {{ err }}</span>
    <label>
      refresh
      <select v-model.number="intervalMs" @change="restart">
        <option :value="2000">2s</option>
        <option :value="5000">5s</option>
        <option :value="15000">15s</option>
        <option :value="0">off</option>
      </select>
    </label>
    <button @click="load">↻ now</button>
  </div>
  <div ref="graphEl" class="graph"></div>
</template>

<style>
.graph { width: 100vw; height: 100vh; }
.hud {
  position: fixed; top: 10px; left: 12px; z-index: 10;
  background: #11151aee; color: #cdd3da; padding: 8px 12px;
  border-radius: 8px; border: 1px solid #222a33;
  font: 13px system-ui, sans-serif; display: flex; gap: 10px; align-items: center;
}
.hud b { color: #7fd1ae; }
.err { color: #e06c75; }
.hud select, .hud button {
  background: #0b0d10; color: #cdd3da; border: 1px solid #2a323b;
  border-radius: 5px; padding: 2px 6px; cursor: pointer;
}
</style>
