import { useQuery } from '@tanstack/react-query'
import { api } from './api'
import type { Grant, Unit } from './types'

export function useUnits() {
  return useQuery({ queryKey: ['org-tree'], queryFn: () => api.get<Unit[]>('/org/tree'), staleTime: 300_000 })
}

const covers = (a: string, b: string) => b === a || b.startsWith(a + '.')

/** Units where the user holds `perm` (the grant's node and everything under it). */
export function unitsWithPerm(units: Unit[] | undefined, grants: Grant[], perm: string) {
  const roots = grants.filter((g) => g.perms.includes(perm)).map((g) => g.path)
  return (units ?? []).filter((u) => roots.some((r) => covers(r, u.path)))
}

export function unitLabel(u: Unit) {
  return `${' '.repeat(Math.max(0, u.depth - 1))}${u.name}`
}
