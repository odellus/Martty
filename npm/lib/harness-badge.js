/** Registry-owned Harness marks. Only the Rust painter touches the terminal. */
import { createHash, randomUUID } from 'node:crypto'
import { promises as fs } from 'node:fs'
import path from 'node:path'
import { createRequire } from 'node:module'
import { readAcpRegistrySnapshot } from './harness-registry.js'
import { discoverHarnesses } from './harnesses.js'

export const name = 'harness-badge'
export const inject = ['acpSessionStatus', 'tuiSlots']
// ACP Registry does not list DeepSeek yet. Use Lobe's MIT-licensed mark,
// supplied by the installed SVG library. Registry always takes priority.
const deepseekBadge = {
  label: 'DeepSeek',
  icon: 'lobe:deepseek',
}
const require = createRequire(import.meta.url)
const MAX_BYTES = 512 * 1024
const png = bytes => bytes.length <= MAX_BYTES && bytes.subarray(0, 8).equals(Buffer.from([137,80,78,71,13,10,26,10]))

export function createIconCache(options = {}) {
  const pending = new Map()
  return function load(url) {
    if (typeof url !== 'string' || (url !== 'lobe:deepseek' && !url.startsWith('https://'))) return Promise.resolve(undefined)
    if (pending.has(url)) return pending.get(url)
    const promise = (async () => {
      const local = url === 'lobe:deepseek'
      const version = local ? require('@lobehub/icons-static-svg/package.json').version : ''
      const key = createHash('sha256').update(local ? `${url}@${version}` : url).digest('hex')
      const file = options.settingsPath && path.join(path.dirname(options.settingsPath), 'cache', 'harness-icons', `${key}.png`)
      if (file) {
        try {
          if ((await fs.stat(file)).size <= MAX_BYTES) {
            const cached = await fs.readFile(file)
            if (png(cached)) return cached.toString('base64')
          }
        } catch { /* Missing cache: fetch below. */ }
      }
      let source
      if (local) {
        source = await fs.readFile(require.resolve('@lobehub/icons-static-svg/icons/deepseek.svg'))
        if (source.length > MAX_BYTES) throw new Error('Icon too large')
      } else {
        const response = await (options.fetchImpl ?? fetch)(url, { signal: AbortSignal.timeout(5000) })
        if (!response.ok) throw new Error('Icon unavailable')
        if (Number(response.headers.get('content-length')) > MAX_BYTES) throw new Error('Icon too large')
        const chunks = []
        let size = 0
        for await (const chunk of response.body) {
          size += chunk.length
          if (size > MAX_BYTES) throw new Error('Icon too large')
          chunks.push(chunk)
        }
        source = Buffer.concat(chunks)
      }
      // Lazy import: an unavailable optional native renderer still leaves the name usable.
      const { Resvg } = await import('@resvg/resvg-js')
      let bytes
      if (png(source)) {
        bytes = source
      } else {
        const renderer = new Resvg(source.toString('utf8').replaceAll('currentColor', '#a0a0a0'), {
          fitTo: { mode: 'width', value: 64 }, font: { loadSystemFonts: false },
        })
        if (renderer.width <= 0 || renderer.height <= 0 || renderer.height / renderer.width > 4) throw new Error('Invalid icon dimensions')
        bytes = renderer.render().asPng()
      }
      if (!png(bytes)) throw new Error('Invalid icon')
      if (file) {
        const temporary = `${file}.${randomUUID()}.tmp`
        try {
          await fs.mkdir(path.dirname(file), { recursive: true })
          await fs.writeFile(temporary, bytes)
          await fs.rename(temporary, file)
        } catch { /* A read-only cache must not hide a downloaded icon. */ }
        finally { await fs.rm(temporary, { force: true }).catch(() => {}) }
      }
      return bytes.toString('base64')
    })().catch(() => undefined)
    pending.set(url, promise)
    return promise
  }
}

function packageName(spec) {
  if (typeof spec !== 'string') return undefined
  return /^(?:(@[^/\s]+\/[^@/\s]+)|([^@/\s:]+))(?:@[^\s]+)?$/.exec(spec)?.slice(1).find(Boolean)
}

function runtimePackage(runtime) {
  if (!runtime || !/(?:^|[/\\])npx(?:\.cmd|\.exe)?$/i.test(runtime.command ?? '')) return undefined
  const args = runtime.args ?? []
  const explicit = args.findIndex(arg => arg === '--package' || arg === '-p')
  return packageName(explicit >= 0 ? args[explicit + 1] : args.find(arg => !arg.startsWith('-')))
}

export function apply(ctx, options = {}) {
  const loadIcon = options.loadIcon ?? createIconCache(options)
  let panel, disposed = false, generation = 0, lastKey
  let nodes = []
  const stopSlot = ctx.tuiSlots.inject('conversation.harness', () => {
    panel = ctx.tuiSlots.register({ name: 'conversation.harness', id: 'harness' }, nodes)
    return () => panel.dispose()
  })
  function update(status) {
    const session = status.session?.bound ? status.session.sessionId : undefined
    const key = JSON.stringify([session, status.server, status.runtime])
    if (key === lastKey) return
    lastKey = key
    const token = ++generation
    if (!session) { nodes = []; panel?.update(nodes); return }
    const registry = options.registry ?? readAcpRegistrySnapshot(options)
    const runtime = status.runtime
    const entries = options.entries ?? discoverHarnesses(options.settingsPath, { ...options, registry, pathValue: '' })
    const entry = runtime && entries.find(candidate => candidate.command === runtime.command
      && JSON.stringify(candidate.args ?? []) === JSON.stringify(runtime.args ?? []))
    const npmPackage = runtimePackage(runtime) ?? packageName(status.server)
    const record = registry.find(candidate => candidate.id === entry?.id)
      ?? registry.find(candidate => candidate.id === status.server || candidate.label === status.server)
      ?? (npmPackage && registry.find(candidate => candidate.distributions?.some(distribution =>
        distribution.type === 'npx' && packageName(distribution.args?.[0]) === npmPackage)))
      ?? (['dsh-acp', '@deepseek-ai/dsh-acp', '@openma/deepseek-harness-acp'].includes(status.server)
        || ['@deepseek-ai/dsh-acp', '@openma/deepseek-harness-acp'].includes(npmPackage)
        ? deepseekBadge : undefined)
    const label = (record?.label ?? entry?.label ?? status.server ?? runtime?.command)?.replace(/\s+harness$/i, '')
    nodes = label ? [{ id: session, kind: 'image', name: label, mime: 'image/png' }] : []
    panel?.update(nodes)
    if (record?.icon && nodes.length) void loadIcon(record.icon).then(dataBase64 => {
      if (disposed || token !== generation || !dataBase64) return
      nodes = [{ ...nodes[0], dataBase64 }]
      panel?.update(nodes)
    })
  }
  update(ctx.acpSessionStatus.current())
  const stopStatus = ctx.acpSessionStatus.subscribe(update)
  return () => { disposed = true; ++generation; stopStatus?.(); stopSlot?.() }
}
