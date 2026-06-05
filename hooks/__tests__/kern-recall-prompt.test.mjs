import { test } from 'node:test';
import assert from 'node:assert/strict';
import { parseHits, buildContext, buildOutput, MIN_SCORE } from '../kern-recall-prompt.mjs';

const SAMPLE = [
  '1. [0.8322] b874727a8651  Trello board routing for kern work (corrected 2026-06-05)',
  '2. [0.7354] 2fadb1541c40  Kern tasks and findings must use the dedicated Kern Trello board',
  '3. [0.5011] fa5826a3afe0  A weak, below-threshold hit that should be dropped',
  'garbage line that is not a hit',
].join('\n');

test('parseHits extracts score/id/text and ignores non-hit lines', () => {
  const hits = parseHits(SAMPLE);
  assert.equal(hits.length, 3);
  assert.deepEqual(hits[0], { score: 0.8322, id: 'b874727a8651', text: 'Trello board routing for kern work (corrected 2026-06-05)' });
  assert.equal(hits[2].score, 0.5011);
});

test('parseHits returns [] on empty / junk', () => {
  assert.deepEqual(parseHits(''), []);
  assert.deepEqual(parseHits('no hits here\nfoo bar'), []);
});

test('buildContext drops sub-threshold hits and caps count', () => {
  const ctx = buildContext(parseHits(SAMPLE));
  assert.match(ctx, /semantic recall/);
  assert.match(ctx, /\[0\.83\] Trello board routing/);
  assert.match(ctx, /\[0\.74\] Kern tasks/);
  assert.doesNotMatch(ctx, /below-threshold/); // 0.50 < MIN_SCORE
});

test('buildContext respects max cap', () => {
  const many = Array.from({ length: 10 }, (_, i) => `${i + 1}. [0.90] id${i}  thought ${i}`).join('\n');
  const ctx = buildContext(parseHits(many), { max: 3 });
  const bullets = ctx.split('\n').filter((l) => l.startsWith('- '));
  assert.equal(bullets.length, 3);
});

test('buildContext returns empty when nothing clears the floor', () => {
  assert.equal(buildContext(parseHits('1. [0.10] x  weak')), '');
  assert.ok(MIN_SCORE > 0.10);
});

test('buildOutput wraps in the UserPromptSubmit envelope, empty stays empty', () => {
  assert.equal(buildOutput(''), '');
  const out = JSON.parse(buildOutput('hello'));
  assert.equal(out.hookSpecificOutput.hookEventName, 'UserPromptSubmit');
  assert.equal(out.hookSpecificOutput.additionalContext, 'hello');
});
