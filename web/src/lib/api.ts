// Thin fetch wrapper: JSON in/out, CSRF header on writes, typed errors.

export class ApiError extends Error {
  constructor(public status: number, public code: string, message: string) {
    super(message)
  }
}

let csrfToken = ''
export function setCsrf(token: string) {
  csrfToken = token
}

type Handler = (method: string, path: string, body?: unknown) => Promise<unknown>
let override: Handler | null = null
/** Demo builds swap the network for an in-browser backend. */
export function setTransport(h: Handler) {
  override = h
}

export async function request<T>(method: string, path: string, body?: unknown): Promise<T> {
  if (override) return (await override(method, path, body)) as T
  const headers: Record<string, string> = { accept: 'application/json' }
  if (body !== undefined) headers['content-type'] = 'application/json'
  if (method !== 'GET') headers['x-csrf-token'] = csrfToken
  const res = await fetch(`/api${path}`, {
    method,
    headers,
    credentials: 'same-origin',
    body: body === undefined ? undefined : JSON.stringify(body),
  })
  const text = await res.text()
  let data: any = null
  try {
    data = text ? JSON.parse(text) : null
  } catch {
    data = null
  }
  if (!res.ok) {
    const err = data?.error
    throw new ApiError(res.status, err?.code ?? 'error', err?.message ?? (text || res.statusText))
  }
  return data as T
}

export const api = {
  get: <T>(p: string) => request<T>('GET', p),
  post: <T>(p: string, b: unknown = {}) => request<T>('POST', p, b),
  put: <T>(p: string, b: unknown) => request<T>('PUT', p, b),
  patch: <T>(p: string, b: unknown) => request<T>('PATCH', p, b),
  del: <T>(p: string) => request<T>('DELETE', p),
}

export function qs(params: Record<string, string | number | undefined | null>) {
  const s = new URLSearchParams()
  for (const [k, v] of Object.entries(params)) if (v !== undefined && v !== null && v !== '') s.set(k, String(v))
  const str = s.toString()
  return str ? `?${str}` : ''
}
