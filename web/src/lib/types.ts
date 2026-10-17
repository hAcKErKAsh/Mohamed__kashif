export interface Grant {
  membership_id: string
  org_unit_id: string
  org_unit_name: string
  path: string
  role_key: string
  role_name: string
  perms: string[]
}

export interface Me {
  user: {
    id: string
    email: string
    full_name: string
    must_change_password: boolean
    totp_enabled: boolean
    mfa_setup_required: boolean
    profile: Record<string, unknown>
  }
  csrf_token: string
  roles: string[]
  permissions: string[]
  grants: Grant[]
  children: { id: string; full_name: string }[]
  unread_notifications: number
  settings: {
    branding: { name: string; tagline: string; accent: string; motto?: string }
    modules: Record<string, boolean>
    profile_fields: FieldDef[]
  }
}

export interface FieldDef {
  key: string
  label: string
  type: 'text' | 'textarea' | 'number' | 'date' | 'select' | 'checkbox' | 'email'
  required?: boolean
  options?: string[]
  help?: string
  min?: number
  max?: number
}

export interface Unit {
  id: string
  parent_id: string | null
  kind: string
  name: string
  code: string
  path: string
  depth: number
  sort_order: number
  member_count: number
}

export interface Role {
  id: string
  key: string
  name: string
  description: string
  color: string
  is_system: boolean
  permissions: string[]
  member_count: number
}

export interface PermissionDef {
  key: string
  group: string
  label: string
}

export type NodeType = 'start' | 'approval' | 'condition' | 'parallel' | 'join' | 'end'

export type Cond =
  | { all: Cond[] }
  | { any: Cond[] }
  | { not: Cond }
  | { field: string; op: string; value?: unknown }

export type ApproverRule =
  | { kind: 'role'; role: string; search?: string }
  | { kind: 'roles'; roles: string[]; search?: string }
  | { kind: 'users'; ids: string[] }
  | { kind: 'guardians' }

export interface FlowNode {
  id: string
  type: NodeType
  name?: string
  approvers?: ApproverRule
  mode?: 'any' | 'all' | 'quorum'
  quorum?: number | null
  sla_hours?: number | null
  escalate_to?: string | null
  outcome?: 'approved' | 'rejected'
}

export interface FlowEdge {
  from: string
  to: string
  on?: 'approved' | 'rejected'
  when?: Cond
  default?: boolean
  label?: string
}

export interface Graph {
  nodes: FlowNode[]
  edges: FlowEdge[]
  fallback?: 'admins' | 'skip'
}
