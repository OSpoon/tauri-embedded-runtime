<script setup lang="ts">
import type {
  ProjectInfo,
  RuntimeEvent,
  RuntimeSnapshot,
  ServiceName,
  ServiceSnapshot,
} from "@/features/runtime-setup/types"
import {
  Activity,
  ArrowRight,
  Braces,
  Check,
  CircleAlert,
  CircleDot,
  Code2,
  Gauge,
  Settings2,
} from "@lucide/vue"
import { computed } from "vue"
import logoUrl from "@/assets/tauri-embedded-runtime-logo.png"
import { Button } from "@/components/ui/button"
import { ScrollArea } from "@/components/ui/scroll-area"

const props = defineProps<{
  events: RuntimeEvent[]
  snapshot: RuntimeSnapshot
  services: ServiceSnapshot[]
  projects: ProjectInfo[]
  message: string
}>()

const emit = defineEmits<{
  back: []
  call: [name: ServiceName]
}>()

const activeProject = computed(() => props.projects[0])
const activeService = computed(() =>
  activeProject.value
    ? props.services.find(
        service => service.name === activeProject.value?.project_id,
      )
    : undefined,
)
const runtimeLabel = computed(() =>
  props.snapshot.selected_runtime === "node" ? "Node.js" : "Python",
)
const runtimeVersion = computed(() =>
  props.snapshot.selected_runtime === "node"
    ? props.snapshot.node.version
    : props.snapshot.python.version,
)
const runtimeIcon = computed(() =>
  props.snapshot.selected_runtime === "node" ? Braces : Code2,
)
const serviceEndpoint = computed(() =>
  activeService.value
    ? `http://127.0.0.1:${activeService.value.port}`
    : "等待服务启动",
)
const serviceStatus = computed(() =>
  activeService.value?.running ? "健康运行" : "等待启动",
)
const canCallService = computed(() =>
  Boolean(
    activeProject.value
    && activeService.value?.running
    && activeProject.value.demo_path,
  ),
)
const lastCheckLabel = computed(() =>
  props.snapshot.checked_at
    ? new Date(props.snapshot.checked_at * 1000).toLocaleTimeString([], {
        hour12: false,
      })
    : "刚刚",
)

function isTerminalEvent(event: RuntimeEvent) {
  return (
    event.status === "completed"
    || event.status === "ready"
    || event.status === "cached"
    || event.status === "failed"
    || event.status === "cancelled"
  )
}

const recentEvents = computed(() => {
  const visibleEvents = props.events.filter((event, index, events) => {
    if (event.status !== "running")
      return true
    return !events
      .slice(index + 1)
      .some(
        nextEvent =>
          nextEvent.phase === event.phase
          && nextEvent.operation_id === event.operation_id
          && isTerminalEvent(nextEvent),
      )
  })
  return visibleEvents.slice(-6).reverse()
})

function eventStatusLabel(event: RuntimeEvent) {
  if (event.status === "failed" || event.status === "cancelled")
    return "失败"
  if (
    event.status === "completed"
    || event.status === "ready"
    || event.status === "cached"
  ) {
    return "完成"
  }
  return "处理中"
}

function eventClass(event: RuntimeEvent) {
  if (event.status === "failed" || event.status === "cancelled")
    return "text-red-600"
  if (
    event.status === "completed"
    || event.status === "ready"
    || event.status === "cached"
  ) {
    return "text-emerald-600"
  }
  return "text-muted-foreground"
}

function eventDotClass(event: RuntimeEvent) {
  if (event.status === "failed" || event.status === "cancelled")
    return "border-red-500 bg-red-500"
  if (
    event.status === "completed"
    || event.status === "ready"
    || event.status === "cached"
  ) {
    return "border-emerald-500 bg-emerald-500"
  }
  return "border-primary bg-background"
}

function eventTime(event: RuntimeEvent) {
  return new Date(event.timestamp * 1000).toLocaleTimeString([], {
    hour12: false,
  })
}

function callActiveService() {
  if (canCallService.value && activeProject.value)
    emit("call", activeProject.value.project_id)
}
</script>

<template>
  <main class="flex min-h-0 flex-1 flex-col bg-background text-foreground">
    <header class="shrink-0 border-b border-border bg-background">
      <div
        class="mx-auto flex h-16 w-full max-w-6xl items-center justify-between gap-4 px-5 sm:px-8 lg:px-10"
      >
        <div class="flex min-w-0 items-center gap-3">
          <img
            :src="logoUrl"
            alt="Tauri Embedded Runtime Logo"
            class="size-8 shrink-0 rounded-lg"
          >
          <div class="min-w-0">
            <p
              class="truncate text-sm font-semibold tracking-[-0.03em] text-foreground"
            >
              Tauri Embedded Runtime
            </p>
            <p class="truncate text-[10px] text-muted-foreground">
              Local business workspace
            </p>
          </div>
        </div>

        <div class="flex shrink-0 items-center gap-2 sm:gap-4">
          <div class="hidden items-center gap-2 sm:flex">
            <component :is="runtimeIcon" class="size-4 text-primary" />
            <span class="text-xs text-foreground">{{ runtimeLabel }} {{ runtimeVersion ?? "runtime" }}</span>
            <span class="flex items-center gap-1.5 text-xs text-emerald-600">
              <span class="size-2 rounded-full bg-emerald-500" />
              运行中
            </span>
          </div>
          <Button
            variant="ghost"
            size="sm"
            class="h-8 gap-1.5 px-2.5 text-xs text-muted-foreground"
            @click="emit('back')"
          >
            <Settings2 class="size-3.5" />
            <span class="hidden sm:inline">运行检测</span>
          </Button>
        </div>
      </div>
    </header>

    <ScrollArea class="min-h-0 flex-1">
      <div class="mx-auto w-full max-w-6xl px-5 py-8 sm:px-8 sm:py-10 lg:px-10">
        <section class="overflow-hidden rounded-2xl border border-border">
          <div
            class="grid sm:grid-cols-[1.15fr_0.8fr_1fr_auto] sm:divide-x sm:divide-y-0"
          >
            <div class="px-5 py-5 sm:px-7">
              <p
                class="text-[10px] font-medium uppercase tracking-[0.16em] text-muted-foreground"
              >
                服务地址
              </p>
              <p
                class="mt-2 truncate font-mono text-sm text-foreground"
                :title="serviceEndpoint"
              >
                {{ serviceEndpoint }}
              </p>
            </div>
            <div class="px-5 py-5 sm:px-6">
              <p
                class="text-[10px] font-medium uppercase tracking-[0.16em] text-muted-foreground"
              >
                端口
              </p>
              <p class="mt-2 font-mono text-sm text-foreground">
                {{ activeService?.port ?? "—" }}
              </p>
            </div>
            <div class="flex items-center gap-3 px-5 py-5 sm:px-6">
              <component :is="runtimeIcon" class="size-5 text-primary" />
              <div>
                <p
                  class="text-[10px] font-medium uppercase tracking-[0.16em] text-muted-foreground"
                >
                  运行时
                </p>
                <p class="mt-1 text-sm text-foreground">
                  {{ runtimeLabel }} {{ runtimeVersion ?? "—" }}
                </p>
              </div>
            </div>
            <div class="flex items-center gap-3 px-5 py-5 sm:px-7">
              <span
                class="flex size-8 shrink-0 items-center justify-center rounded-full bg-emerald-500/10 text-emerald-600"
              >
                <Check class="size-4" />
              </span>
              <div>
                <p
                  class="text-[10px] font-medium uppercase tracking-[0.16em] text-muted-foreground"
                >
                  健康检查
                </p>
                <p class="mt-1 text-sm text-foreground">
                  {{ activeProject?.health_path ?? "/health" }} · 200 OK
                </p>
              </div>
            </div>
          </div>

          <div
            class="flex flex-col items-start justify-between gap-3 border-t border-border bg-muted/20 px-5 py-4 sm:flex-row sm:items-center sm:px-7"
          >
            <div
              class="flex min-w-0 items-center gap-2 text-xs text-muted-foreground"
            >
              <Gauge class="size-3.5 shrink-0 text-primary" />
              <span class="truncate">{{ props.message }}</span>
            </div>
            <Button
              class="h-10 shrink-0 gap-2 px-5 shadow-sm"
              :disabled="!canCallService"
              @click="callActiveService"
            >
              调用服务
              <ArrowRight class="size-4" />
            </Button>
          </div>
        </section>

        <section class="mt-8">
          <div class="flex items-center justify-between gap-3">
            <div class="flex items-center gap-2">
              <Activity class="size-4 text-primary" />
              <h2 class="text-sm font-semibold text-foreground">
                最近活动
              </h2>
            </div>
            <span class="text-xs text-muted-foreground">{{ props.events.length }} 条记录</span>
          </div>

          <div
            class="mt-3 overflow-hidden rounded-xl border border-border bg-card"
          >
            <div v-if="recentEvents.length" class="divide-y divide-border">
              <div
                v-for="(event, index) in recentEvents"
                :key="`${event.timestamp}-${index}`"
                class="grid grid-cols-[auto_1fr_auto] items-center gap-3 px-4 py-3 sm:grid-cols-[auto_100px_72px_1fr_auto] sm:px-5"
              >
                <span class="flex size-5 items-center justify-center">
                  <span
                    class="size-2 rounded-full border-2"
                    :class="eventDotClass(event)"
                  />
                </span>
                <span
                  class="hidden font-mono text-[10px] text-muted-foreground sm:block"
                >{{ eventTime(event) }}</span>
                <span
                  class="text-[10px] font-medium"
                  :class="eventClass(event)"
                >{{ eventStatusLabel(event) }}</span>
                <span
                  class="truncate text-xs text-foreground"
                  :title="event.message"
                >{{ event.message }}</span>
                <span
                  class="hidden max-w-48 truncate text-[10px] text-muted-foreground sm:block"
                  :title="event.output ?? undefined"
                >{{ event.output ?? "—" }}</span>
              </div>
            </div>
            <div
              v-else
              class="flex items-center gap-3 px-5 py-6 text-xs text-muted-foreground"
            >
              <CircleDot class="size-4" />
              暂无最近活动记录。
            </div>
          </div>
        </section>

        <footer
          class="mt-6 flex items-center gap-2 text-[10px] text-muted-foreground"
        >
          <CircleAlert class="size-3.5 shrink-0" />
          服务仅监听 127.0.0.1，由应用负责端口分配、健康检查和进程回收。
        </footer>
      </div>
    </ScrollArea>
  </main>
</template>
