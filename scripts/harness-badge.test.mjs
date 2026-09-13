import test from 'node:test'
import assert from 'node:assert/strict'
import { mkdtempSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import path from 'node:path'
import { normalizeAcpRegistry } from '../npm/lib/harness-registry.js'
const badge = await import('../npm/lib/harness-badge.js').catch(() => ({}))
const svg = '<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16"><circle cx="8" cy="8" r="7" fill="currentColor"/></svg>'

test('Registry retains icon URLs without inventing brand mappings', () => {
  const [entry] = normalizeAcpRegistry({ agents: [{ id: 'example', name: 'Example', version: '1', icon: 'https://example.com/icon.svg', distribution: { npx: { package: 'example' } } }] })
  assert.equal(entry.icon, 'https://example.com/icon.svg')
})

test('Registry SVG becomes PNG and survives a new cache instance offline', async t => {
  assert.equal(typeof badge.createIconCache, 'function')
  const root = mkdtempSync(path.join(tmpdir(), 'martty-icons-'))
  t.after(() => rmSync(root, { recursive: true, force: true }))
  let calls = 0
  const options = { settingsPath: path.join(root, 'settings.json'), fetchImpl: async () => { calls++; return new Response(svg) } }
  const first = badge.createIconCache(options)
  const [a,b] = await Promise.all([first('https://example.com/icon.svg'), first('https://example.com/icon.svg')])
  assert.equal(a, b)
  assert.ok(Buffer.from(a, 'base64').subarray(0, 8).equals(Buffer.from([137,80,78,71,13,10,26,10])))
  assert.equal(calls, 1)
  const second = badge.createIconCache({ ...options, fetchImpl: async () => { throw new Error('offline') } })
  assert.equal(await second('https://example.com/icon.svg'), a)
  assert.equal(await second('https://example.com/missing.svg'), undefined)
  assert.equal(await second('file:///etc/passwd'), undefined)
})

test('badge follows selected session; a late icon never replaces a different tab name', async () => {
  assert.equal(typeof badge.apply, 'function')
  let listener, nodes, finish
  const icon = new Promise(resolve => { finish = resolve })
  const a = { server: 'Alpha', session: { sessionId: 'a', bound: true }, runtime: { command: 'alpha', args: [] } }
  const b = { server: 'Custom', session: { sessionId: 'b', bound: true }, runtime: { command: 'custom', args: [] } }
  const ctx = {
    acpSessionStatus: { current: () => a, subscribe(fn) { listener = fn; return () => {} } },
    tuiSlots: { inject(name, fn) { assert.equal(name, 'conversation.harness'); return fn() }, register(_, initial) { nodes = initial; return { update(next) { nodes = next }, dispose() {} } } },
  }
  const dispose = badge.apply(ctx, { registry: [{ id: 'alpha', label: 'Alpha Harness', icon: 'https://example.com/a.svg' }], entries: [{ id: 'alpha', command: 'alpha', args: [] }], loadIcon: () => icon })
  assert.equal(nodes[0].name, 'Alpha')
  assert.equal(nodes[0].dataBase64, undefined)
  listener(b)
  assert.equal(nodes[0].name, 'Custom')
  finish('png')
  await new Promise(resolve => setImmediate(resolve))
  assert.equal(nodes[0].name, 'Custom')
  assert.equal(nodes[0].dataBase64, undefined)
  listener(a)
  await new Promise(resolve => setImmediate(resolve))
  assert.equal(nodes[0].dataBase64, 'png')
  listener({ session: { bound: false } })
  assert.deepEqual(nodes, [])
  dispose()
})

test('a saved Harness alias resolves the Registry icon through its versioned npm package', async () => {
  let nodes, requested
  const ctx = {
    acpSessionStatus: {
      current: () => ({ server: '@agentclientprotocol/codex-acp', session: { sessionId: 'codex-session', bound: true }, runtime: { command: '/opt/node/bin/npx', args: ['@agentclientprotocol/codex-acp'] } }),
      subscribe: () => () => {},
    },
    tuiSlots: { inject: (_, fn) => fn(), register: (_, initial) => { nodes = initial; return { update: next => { nodes = next }, dispose() {} } } },
  }
  const dispose = badge.apply(ctx, {
    registry: [{ id: 'codex-acp', label: 'Codex', icon: 'https://example.com/codex.svg', distributions: [{ type: 'npx', args: ['@agentclientprotocol/codex-acp@1.10.0'] }] }],
    entries: [{ id: 'codex', label: 'Codex Harness', command: '/opt/node/bin/npx', args: ['@agentclientprotocol/codex-acp'] }],
    loadIcon: async url => { requested = url; return 'png' },
  })
  await new Promise(resolve => setImmediate(resolve))
  assert.equal(requested, 'https://example.com/codex.svg')
  assert.equal(nodes[0].name, 'Codex')
  assert.equal(nodes[0].dataBase64, 'png')
  dispose()
})

test('DeepSeek gets a Lobe fallback without overriding a Registry record', async () => {
  for (const registry of [[], [{ id: 'dsh-acp', label: 'DeepSeek', icon: 'https://registry.example/deepseek.svg' }]]) {
    let nodes, requested
    const ctx = {
      acpSessionStatus: {
        current: () => ({ server: 'dsh-acp', session: { sessionId: 'dsh', bound: true } }),
        subscribe: () => () => {},
      },
      tuiSlots: { inject: (_, fn) => fn(), register: (_, initial) => {
        nodes = initial; return { update: next => { nodes = next }, dispose() {} }
      } },
    }
    const dispose = badge.apply(ctx, { registry, entries: [], loadIcon: async url => { requested = url; return 'png' } })
    await new Promise(resolve => setImmediate(resolve))
    assert.equal(nodes[0].name, 'DeepSeek')
    assert.equal(nodes[0].dataBase64, 'png')
    if (registry.length) assert.equal(requested, registry[0].icon)
    else assert.equal(requested, 'lobe:deepseek')
    dispose()
  }
})

test('Lobe DeepSeek is available offline from the installed library and caches the PNG', async t => {
  const root = mkdtempSync(path.join(tmpdir(), 'martty-lobe-library-'))
  t.after(() => rmSync(root, { recursive: true, force: true }))
  let requests = 0
  const options = { settingsPath: path.join(root, 'settings.json'), fetchImpl: async () => { requests++; throw Error('offline') } }
  const data = await badge.createIconCache(options)('lobe:deepseek')
  assert.ok(data)
  assert.ok(Buffer.from(data, 'base64').subarray(0, 8).equals(Buffer.from([137,80,78,71,13,10,26,10])))
  assert.equal(await badge.createIconCache(options)('lobe:deepseek'), data)
  assert.equal(requests, 0)
})
