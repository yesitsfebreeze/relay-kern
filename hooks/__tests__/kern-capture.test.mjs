import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readLines, extractDelta } from '../kern-capture.mjs';

// ── readLines ─────────────────────────────────────────────────────────────

test('readLines splits on newlines', () => {
  assert.deepEqual(readLines('a\nb\nc'), ['a', 'b', 'c']);
});

test('readLines drops trailing empty element from final newline', () => {
  assert.deepEqual(readLines('a\nb\n'), ['a', 'b']);
});

test('readLines on empty string returns empty array', () => {
  assert.deepEqual(readLines(''), []);
});

test('readLines single line no newline', () => {
  assert.deepEqual(readLines('hello'), ['hello']);
});

test('readLines preserves blank lines mid-content', () => {
  assert.deepEqual(readLines('a\n\nb'), ['a', '', 'b']);
});

// ── extractDelta helpers ──────────────────────────────────────────────────

function makeUser(content) {
  return JSON.stringify({ type: 'user', message: { content } });
}

function makeAssistant(blocks) {
  return JSON.stringify({ type: 'assistant', message: { content: blocks } });
}

function makeTextBlock(text) {
  return { type: 'text', text };
}

// ── extractDelta: consumed + offset chaining ──────────────────────────────

test('consumed equals lines.length and enables offset chaining', () => {
  const lines = [makeUser('first'), makeUser('second')];
  const { consumed } = extractDelta(lines, 0);
  assert.equal(consumed, lines.length);
  // Second call with consumed as offset sees nothing new.
  const { text: text2, consumed: c2 } = extractDelta(lines, consumed);
  assert.equal(text2, '');
  assert.equal(c2, lines.length);
});

// ── extractDelta: user messages ───────────────────────────────────────────

test('extracts user string content', () => {
  const { text } = extractDelta([makeUser('hello world')], 0);
  assert.match(text, /^user: hello world$/);
});

test('skips non-text content blocks (tool_result, tool_use)', () => {
  const lines = [
    makeUser([{ type: 'tool_result', content: 'result' }]),
    makeAssistant([{ type: 'tool_use', id: 'x', name: 'Bash', input: {} }]),
    makeUser('real'),
  ];
  const { text } = extractDelta(lines, 0);
  assert.match(text, /real/);
  assert.equal(text.split('\n\n').filter(Boolean).length, 1);
});

test('skips blank content for both message types', () => {
  const lines = [makeUser('   '), makeAssistant([makeTextBlock('   ')]), makeUser('real')];
  const { text } = extractDelta(lines, 0);
  assert.match(text, /real/);
  assert.equal(text.split('\n\n').filter(Boolean).length, 1);
});

// ── extractDelta: assistant messages ─────────────────────────────────────

test('extracts assistant text blocks', () => {
  const { text } = extractDelta([makeAssistant([makeTextBlock('hello from assistant')])], 0);
  assert.match(text, /^assistant: hello from assistant$/);
});

test('extracts multiple text blocks from one assistant turn', () => {
  const { text } = extractDelta([makeAssistant([makeTextBlock('part one'), makeTextBlock('part two')])], 0);
  assert.match(text, /assistant: part one/);
  assert.match(text, /assistant: part two/);
});

// ── extractDelta: mixed conversation ─────────────────────────────────────

test('interleaves user and assistant correctly', () => {
  const lines = [
    makeUser('question'),
    makeAssistant([makeTextBlock('answer')]),
    makeUser('follow-up'),
  ];
  const parts = extractDelta(lines, 0).text.split('\n\n');
  assert.equal(parts[0], 'user: question');
  assert.equal(parts[1], 'assistant: answer');
  assert.equal(parts[2], 'user: follow-up');
});

// ── extractDelta: offset ──────────────────────────────────────────────────

test('offset skips earlier lines', () => {
  const lines = [makeUser('before'), makeUser('after')];
  const { text } = extractDelta(lines, 1);
  assert.doesNotMatch(text, /before/);
  assert.match(text, /after/);
});

test('offset at lines.length yields empty text', () => {
  const lines = [makeUser('hi')];
  assert.equal(extractDelta(lines, lines.length).text, '');
});

// ── extractDelta: resilience ──────────────────────────────────────────────

test('extracts valid content from noisy input', () => {
  const lines = [
    '',
    '   ',
    'not json',
    JSON.stringify({ type: 'tool_result', content: 'noise' }),
    makeUser('signal'),
  ];
  const { text } = extractDelta(lines, 0);
  assert.match(text, /signal/);
  assert.doesNotMatch(text, /noise/);
  assert.doesNotMatch(text, /not json/);
});
