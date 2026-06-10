<script setup>
import { ref, watch, nextTick, computed } from 'vue'
import KernIcon from '../KernIcon.vue'

const props = defineProps({
  conv: Object,
  focusReply: String,
  stale: Object,
  resent: Object,
  busy: String,
  compose: String,
  thoughts: { type: Object, default: () => ({}) },
  editing: Object,
  draft: { type: String, default: '' },
})
const emit = defineEmits(['setFocusReply', 'setCompose', 'send', 'resend', 'startEdit', 'save', 'cancel', 'setDraft'])

const scrollRef = ref(null)
const taRef = ref(null)

const lastText = computed(() => {
  const msgs = props.conv?.messages
  if (!msgs?.length) return ''
  const last = msgs[msgs.length - 1]
  return Array.isArray(last.text) ? last.text.join('') : (last.text || '')
})
function scrollBottom() {
  nextTick(() => { if (scrollRef.value) scrollRef.value.scrollTop = scrollRef.value.scrollHeight })
}
watch(() => props.conv?.messages?.length, scrollBottom)
watch(lastText, scrollBottom)

function autosize(el) {
  if (!el) return
  el.style.height = 'auto'
  el.style.height = Math.min(el.scrollHeight, 200) + 'px'
}
function onKey(e) {
  if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); emit('send') }
}
function onInput(e) {
  emit('setCompose', e.target.value)
  autosize(e.target)
}
function onEditKey(e) {
  if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); emit('save') }
  if (e.key === 'Escape') { e.preventDefault(); emit('cancel') }
}

function paragraphs(m) {
  return Array.isArray(m.text) ? m.text : [m.text]
}
function hasText(m) { return paragraphs(m).some(p => p && p.length > 0) }
function nThoughts(m) { return (m.used || []).length }
function nReasons(m) { return (m.usedReasons || []).length }

// ── live memory whispers ──
function relatedThoughts(text, thoughts, k = 4) {
  const words = (text || '').toLowerCase().split(/\W+/).filter(w => w.length > 3)
  const arr = Object.values(thoughts || {}).map(th => ({
    id: th.id,
    hits: words.filter(w => th.text.toLowerCase().includes(w)).length,
  }))
  arr.sort((a, b) => b.hits - a.hits)
  return arr.slice(0, k).map((r, i) => ({
    id: r.id,
    score: Math.max(0.3, 0.93 - i * 0.14 - (r.hits ? 0 : 0.18)),
    cold: i >= 2 && !r.hits,
  }))
}

const composing = computed(() => props.compose.trim().length > 0)
const ctxRaw = computed(() => {
  const lastUser = [...(props.conv?.messages || [])].reverse().find(m => m.role === 'you')
  return props.compose.trim()
    || (lastUser ? (Array.isArray(lastUser.text) ? lastUser.text.join(' ') : lastUser.text) : '')
    || props.conv?.subject
    || ''
})
// debounced so relatedThoughts doesn't fire on every keypress
const debouncedCtx = ref('')
let _dbt = null
watch(ctxRaw, v => {
  clearTimeout(_dbt)
  _dbt = setTimeout(() => { debouncedCtx.value = v }, 120)
}, { immediate: true })
const related = computed(() => relatedThoughts(debouncedCtx.value, props.thoughts, 4))
const isEd = (id) => props.editing && props.editing.kind === 'thought' && props.editing.id === id
</script>

<template>
  <div class="thread">
    <div class="thread-scroll" ref="scrollRef">
      <div class="thread-inner">
        <div v-if="!conv.messages?.length" class="thread-empty" style="padding:24px 8px">
          <div class="te-t" style="font-size:16px">Write your first message</div>
          <div class="te-s">Fire it off like an email. kern replies with what it pulled from memory.</div>
        </div>

        <template v-for="m in conv.messages" :key="m.id">
          <!-- user message -->
          <div v-if="m.role==='you'" class="msg you">
            <div class="msg-by">
              <span class="avatar you">you</span>
              <span class="msg-who">You</span>
              <span class="grow"></span>
              <span class="msg-when">{{ m.when }}</span>
            </div>
            <div class="msg-card">
              <p v-for="(p, i) in paragraphs(m)" :key="i">{{ p }}</p>
            </div>
          </div>

          <!-- kern reply -->
          <div v-else class="msg kern">
            <div class="msg-by">
              <span class="avatar kern"><KernIcon n="layers" :size="13" /></span>
              <span class="msg-who">kern</span>
              <span class="grow"></span>
              <span v-if="resent[m.id]" class="status replied">
                <span class="dot" style="background:var(--sage)"></span>re-sent
              </span>
              <span class="msg-when">{{ m.when }}</span>
            </div>
            <div class="msg-card">
              <template v-if="!hasText(m) && !(m.toolCalls && m.toolCalls.length)">
                <p class="thinking-line">{{ busy === conv.id ? 'pulling from memory…' : '…' }}</p>
              </template>
              <template v-else>
                <p v-for="(p, i) in paragraphs(m)" :key="i">{{ p }}</p>
              </template>
              <!-- tool call events -->
              <div v-if="m.toolCalls && m.toolCalls.length" class="tool-calls">
                <div v-for="tc in m.toolCalls" :key="tc.idx" class="tool-call">
                  <div class="tc-row">
                    <span class="tc-name">{{ tc.name }}</span>
                    <span class="tc-args">{{ JSON.stringify(tc.args) }}</span>
                    <span v-if="tc.ok === null" class="tc-badge pending">running…</span>
                    <span v-else :class="['tc-badge', tc.ok ? 'ok' : 'err']">{{ tc.ok ? 'done' : 'failed' }}</span>
                  </div>
                  <div v-if="tc.result" class="tc-result">{{ tc.result }}</div>
                </div>
              </div>
              <div class="lean">
                <button :class="['lean-chip', focusReply===m.id ? 'on' : '']" @click="emit('setFocusReply', m.id)">
                  <span class="ic"><KernIcon n="thought" :size="13" /></span>
                  leaned on <span class="lean-num">{{ nThoughts(m) }}</span> thought{{ nThoughts(m)!==1 ? 's' : '' }}
                </button>
                <button v-if="nReasons(m)>0" :class="['lean-chip', focusReply===m.id ? 'on' : '']" @click="emit('setFocusReply', m.id)">
                  <span class="ic"><KernIcon n="link" :size="13" /></span>
                  <span class="lean-num">{{ nReasons(m) }}</span> reason{{ nReasons(m)!==1 ? 's' : '' }}
                </button>
                <span class="grow"></span>
                <button class="lean-show" @click="emit('setFocusReply', m.id)">
                  <KernIcon n="layers" :size="13" /> show its work
                </button>
              </div>
              <div v-if="stale[m.id]" class="stale">
                <span class="st-ic"><KernIcon n="refresh" :size="16" /></span>
                <span class="st-tx"><b>Memory changed</b> since this answer. Send it back through to rethink.</span>
                <button class="resend" @click="emit('resend', m.id)">
                  <template v-if="busy===m.id">rethinking…</template>
                  <template v-else><KernIcon n="refresh" :size="14" /> Resend</template>
                </button>
              </div>
            </div>
          </div>
        </template>
      </div>
    </div>

    <!-- live memory whispers — ambient surface between thread and compose -->
    <div v-if="Object.keys(thoughts).length > 0" class="whispers">
      <div class="whispers-inner">
        <div class="wh-head">
          <span class="ey">{{ composing ? 'about to query' : 'related memory' }}</span>
          <span class="wh-live"><span class="d"></span>live</span>
          <span class="wh-tip">{{ composing ? 'edit any before you send' : 'kern is always listening' }}</span>
        </div>
        <div class="wh-list">
          <template v-for="r in related" :key="r.id">
            <template v-if="thoughts[r.id]">
              <!-- inline edit state -->
              <div v-if="isEd(r.id)" style="padding:2px 0">
                <div class="edit-wrap">
                  <textarea class="edit-area" rows="3" :value="draft" autoFocus
                    @input="emit('setDraft', $event.target.value)"
                    @keydown="onEditKey"
                  />
                  <div class="edit-actions">
                    <button class="edit-save" @click="emit('save')"><KernIcon n="check" :size="13" /> Save thought</button>
                    <button class="edit-cancel" @click="emit('cancel')">Cancel</button>
                    <span class="edit-hint"><b>↵</b> save · <b>esc</b> cancel</span>
                  </div>
                </div>
              </div>
              <!-- whisper row -->
              <div v-else :class="['whisper', r.cold ? 'cold' : '']">
                <span class="wid">{{ r.id }}</span>
                <span class="wtx">{{ thoughts[r.id].text }}</span>
                <span v-if="r.cold" class="wid"><KernIcon n="snow" :size="11" /></span>
                <span class="wsc">{{ r.score.toFixed(2) }}</span>
                <button class="wedit" title="Refine this memory"
                  @click="emit('startEdit', 'thought', r.id, thoughts[r.id].text)">
                  <KernIcon n="edit" :size="12" />
                </button>
              </div>
            </template>
          </template>
        </div>
      </div>
    </div>

    <div class="compose">
      <div class="compose-inner">
        <div class="compose-box">
          <textarea
            ref="taRef"
            :value="compose"
            rows="1"
            @input="onInput"
            @keydown="onKey"
            :placeholder="conv.messages?.length ? 'Reply, or push the thought further…' : 'Write your first message…'"
          ></textarea>
          <div class="compose-bar">
            <span class="compose-hint">grounded in memory · <span class="kbd">↵</span> send · <span class="kbd">⇧↵</span> new line</span>
            <span class="grow"></span>
            <button class="send-btn" :disabled="!compose.trim()" @click="emit('send')">
              <KernIcon n="send" :size="14" /> Send
            </button>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>
