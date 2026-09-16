export type RuntimeStatus = "checking" | "ready" | "missing" | "corrupted" | "outdated" | "failed"
export type StepStatus = "pending" | "active" | "complete" | "failed"
export type RuntimeKind = "python" | "node"
// Service names come from registered project modules; the demo currently uses
// python/node, but the reusable setup module must not restrict future modules.
export type ServiceName = string

export interface RuntimeComponent {
  required: boolean
  present: boolean
  status: "disabled" | "ready" | "missing" | "outdated" | "corrupted" | string
  path: string | null
  version: string | null
  issues: string[]
}

export interface ProjectSnapshot {
  project_id: string
  status: string
  active_generation: string | null
  dependency_revision: string
  python_dependencies: string[]
  node_dependencies: string[]
  tools: string[]
  issues: string[]
  message: string
}

export interface ProjectInfo {
  project_id: string
  service: string
  service_label: string
  framework: string
  display_name: string
  health_path: string
  demo_path: string | null
}

export interface RuntimeSnapshot {
  status: RuntimeStatus
  platform: string
  arch: string
  runtime_root: string
  active_generation: string | null
  runtime_revision: string
  requirements: { python: boolean, node: boolean }
  selected_runtime: RuntimeKind
  python: RuntimeComponent
  node: RuntimeComponent
  projects: ProjectSnapshot[]
  issues: string[]
  message: string
  checked_at: number
  operation_id: string | null
  operation_status: string
}

export interface RuntimeEvent {
  phase: string
  status: string
  message: string
  progress: number
  timestamp: number
  output?: string | null
  downloaded_bytes?: number | null
  total_bytes?: number | null
  operation_id?: string | null
  sequence?: number
  artifact?: string | null
  retry_count?: number
  error_code?: string | null
  started_at?: number | null
  updated_at?: number
}

export interface ServiceSnapshot {
  name: ServiceName
  running: boolean
  port: number
}

export interface WizardStep {
  id: string
  phase: string
  title: string
  description: string
  status: StepStatus
}

export interface SetupPlanStep {
  id: string
  phase: string
  title: string
  description: string
}
