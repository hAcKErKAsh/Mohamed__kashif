import { useMemo } from 'react'
import type { Graph, FlowNode } from '../lib/types'

export type NodeState = 'active' | 'done' | 'bad' | 'skip'

const W = 150
const H = 52
const GX = 70
const GY = 20
const PAD = 24

function subLabel(n: FlowNode) {
  switch (n.type) {
    case 'start': return 'Start'
    case 'end': return n.outcome === 'rejected' ? 'Ends · declined' : 'Ends · approved'
    case 'condition': return 'Decision'
    case 'parallel': return 'Parallel split'
    case 'join': return 'Wait for all'
    case 'approval': {
      const mode = n.mode === 'all' ? 'everyone' : n.mode === 'quorum' ? `${n.quorum ?? 1} must agree` : 'any one'
      return `Approval · ${mode}`
    }
  }
}

function clip(s: string, n = 22) {
  return s.length > n ? `${s.slice(0, n - 1)}…` : s
}

/** Layered left-to-right layout of a workflow DAG, rendered as SVG. */
export function FlowDiagram({ graph, states, selected, onSelect, taken }: {
  graph: Graph
  states?: Record<string, NodeState>
  selected?: string | null
  onSelect?: (id: string) => void
  taken?: Set<string>
}) {
  const layout = useMemo(() => {
    const ids = graph.nodes.map((n) => n.id)
    const out = new Map<string, string[]>()
    const indeg = new Map<string, number>(ids.map((i) => [i, 0]))
    for (const e of graph.edges) {
      if (!indeg.has(e.from) || !indeg.has(e.to)) continue
      out.set(e.from, [...(out.get(e.from) ?? []), e.to])
      indeg.set(e.to, (indeg.get(e.to) ?? 0) + 1)
    }
    // Longest-path layering over a topological order (cycles are tolerated).
    const layer = new Map<string, number>(ids.map((i) => [i, 0]))
    const deg = new Map(indeg)
    const queue = ids.filter((i) => (deg.get(i) ?? 0) === 0)
    const order: string[] = []
    while (queue.length) {
      const id = queue.shift()!
      order.push(id)
      for (const t of out.get(id) ?? []) {
        layer.set(t, Math.max(layer.get(t) ?? 0, (layer.get(id) ?? 0) + 1))
        deg.set(t, (deg.get(t) ?? 1) - 1)
        if (deg.get(t) === 0) queue.push(t)
      }
    }
    for (const id of ids) if (!order.includes(id)) order.push(id)
    // Declined ends live in a lane under the diagram so rejection paths don't
    // cut across the happy path.
    const isRejectEnd = (id: string) => graph.nodes.find((n) => n.id === id)?.outcome === 'rejected'
    const rejectEnds = order.filter(isRejectEnd)
    const mainOrder = order.filter((id) => !isRejectEnd(id))
    const cols = new Map<number, string[]>()
    for (const id of mainOrder) {
      const l = layer.get(id) ?? 0
      cols.set(l, [...(cols.get(l) ?? []), id])
    }
    const pos = new Map<string, { x: number; y: number }>()
    const maxRows = Math.max(1, ...[...cols.values()].map((c) => c.length))
    const totalH = maxRows * (H + GY) - GY
    for (const [l, col] of [...cols.entries()].sort((a, b) => a[0] - b[0])) {
      const parentsY = (id: string) => {
        const ys = graph.edges.filter((e) => e.to === id && pos.has(e.from)).map((e) => pos.get(e.from)!.y)
        return ys.length ? ys.reduce((a, b) => a + b, 0) / ys.length : 0
      }
      col.sort((a, b) => parentsY(a) - parentsY(b))
      const colH = col.length * (H + GY) - GY
      col.forEach((id, i) => pos.set(id, { x: PAD + l * (W + GX), y: PAD + (totalH - colH) / 2 + i * (H + GY) }))
    }
    const lastLayer = Math.max(0, ...mainOrder.map((id) => layer.get(id) ?? 0))
    const laneY = PAD + totalH + 46
    rejectEnds.forEach((id, i) => pos.set(id, { x: PAD + (lastLayer - i) * (W + GX), y: laneY }))
    const width = PAD * 2 + (lastLayer + 1) * (W + GX) - GX
    const height = rejectEnds.length ? laneY + H + PAD : PAD * 2 + totalH
    return { pos, width, height, laneY, rejectEnds: new Set(rejectEnds) }
  }, [graph])

  return (
    <div className="flow-canvas">
      <svg width={layout.width} height={layout.height} viewBox={`0 0 ${layout.width} ${layout.height}`} role="img" aria-label="Workflow diagram"
        style={{ width: '100%', maxWidth: layout.width, minWidth: layout.width * 0.82, height: 'auto' }}>
        <defs>
          <marker id="arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto-start-reverse">
            <path d="M0,0 L10,5 L0,10 z" fill="var(--rule-strong)" />
          </marker>
          <marker id="arrow-on" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto-start-reverse">
            <path d="M0,0 L10,5 L0,10 z" fill="var(--accent)" />
          </marker>
        </defs>
        {graph.edges.map((e, i) => {
          const a = layout.pos.get(e.from)
          const b = layout.pos.get(e.to)
          if (!a || !b) return null
          const toLane = layout.rejectEnds.has(e.to)
          let x1 = a.x + W, y1 = a.y + H / 2, x2 = b.x - 2, y2 = b.y + H / 2
          let mx = (x1 + x2) / 2
          let d: string
          if (toLane) {
            // Drop from the stage's bottom edge into the lane, then run along it.
            // Leave from the right edge, drop through the column gap (never
            // through another stage), then run along the bus to the end.
            const gx = a.x + W + 14
            x1 = a.x + W; y1 = a.y + H / 2 + 8; x2 = b.x + W / 2; y2 = b.y - 2
            const busY = layout.laneY - 18
            const dir = x2 >= gx ? 1 : -1
            d = `M${x1},${y1} H${gx - 6} Q${gx},${y1} ${gx},${y1 + 6} V${busY - 6} Q${gx},${busY} ${gx + 6 * dir},${busY} H${x2 - 6 * dir} Q${x2},${busY} ${x2},${busY + 6} V${y2}`
            mx = x1
          } else if (x2 <= x1) {
            d = `M${x1},${y1} C${x1 + 60},${y1 + 80} ${x2 - 60},${y2 + 80} ${x2},${y2}`
          } else {
            d = `M${x1},${y1} C${mx},${y1} ${mx},${y2} ${x2},${y2}`
          }
          const on = taken?.has(`${e.from}>${e.to}`)
          const label = toLane ? undefined : e.label ?? (e.on === 'rejected' ? 'Declined' : e.default ? 'Otherwise' : undefined)
          return (
            <g key={i}>
              <path d={d} className={`flow-edge ${e.on === 'rejected' ? 'rejected' : ''} ${on ? 'taken' : ''}`} markerEnd={on ? 'url(#arrow-on)' : 'url(#arrow)'} />
              {label && (
                <g transform={`translate(${mx}, ${(y1 + y2) / 2})`}>
                  <title>{label}</title>
                  <rect className="flow-label-bg" x={-clip(label, 13).length * 3.1 - 5} y={-8} width={clip(label, 13).length * 6.2 + 10} height={16} rx={3} />
                  <text className="flow-label" textAnchor="middle" dy={3.5}>{clip(label, 13)}</text>
                </g>
              )}
            </g>
          )
        })}
        {graph.nodes.map((n) => {
          const p = layout.pos.get(n.id)
          if (!p) return null
          const st = states?.[n.id]
          const pill = n.type === 'start' || n.type === 'end'
          const name = n.name || n.id
          return (
            <g
              key={n.id}
              transform={`translate(${p.x},${p.y})`}
              className={`flow-node ${n.type} ${st ? `state-${st}` : ''} ${selected === n.id ? 'selected' : ''} ${onSelect ? 'clickable' : ''}`}
              onClick={() => onSelect?.(n.id)}
              tabIndex={onSelect ? 0 : undefined}
              onKeyDown={(e) => e.key === 'Enter' && onSelect?.(n.id)}
            >
              <title>{name}</title>
              <rect width={W} height={H} rx={pill ? H / 2 : 6} />
              {n.type === 'condition' && <path d={`M14,${H / 2 - 6} l6,6 l-6,6 l-6,-6z`} fill="var(--warn)" />}
              {n.type === 'parallel' || n.type === 'join' ? <path d={`M10,${H / 2 - 7} v14 M15,${H / 2 - 7} v14`} stroke="var(--info)" strokeWidth={2} /> : null}
              {n.type === 'end' && <circle cx={16} cy={H / 2} r={5} fill={n.outcome === 'rejected' ? 'var(--bad)' : 'var(--ok)'} />}
              <text x={pill || n.type === 'condition' || n.type === 'parallel' || n.type === 'join' ? 28 : 12} y={22}>{clip(name, pill ? 18 : 20)}</text>
              <text className="sub" x={pill || n.type === 'condition' || n.type === 'parallel' || n.type === 'join' ? 28 : 12} y={39}>{subLabel(n)}</text>
            </g>
          )
        })}
      </svg>
    </div>
  )
}
