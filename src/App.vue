<script setup lang="ts">
import type { ServiceName } from "@/features/runtime-setup/types"
import { ref } from "vue"
import { RuntimeHome } from "@/features/runtime-home"
import { RuntimeSetupWizard, useRuntimeSetup } from "@/features/runtime-setup"

const {
  runtimeSnapshot,
  runtimeProgress,
  runtimeEvents,
  runtimeActionRunning,
  services,
  projectCatalog,
  demoMessage,
  setupStarted,
  wizardSteps,
  bootstrapRuntime,
  cancelRuntime,
  callDemoService: callService,
} = useRuntimeSetup()

const showBusinessHome = ref(false)

function openBusinessHome() {
  showBusinessHome.value = true
}

function backToRuntimeCheck() {
  showBusinessHome.value = false
}

function callBusinessService(name: ServiceName) {
  void callService(name)
}
</script>

<template>
  <div class="app-shell flex min-h-screen min-w-0 flex-col bg-[#f8f9fd] text-zinc-950">
    <RuntimeHome
      v-if="showBusinessHome"
      :services="services"
      :projects="projectCatalog"
      :message="demoMessage"
      @back="backToRuntimeCheck"
      @call="callBusinessService"
    />
    <RuntimeSetupWizard
      v-else
      :snapshot="runtimeSnapshot"
      :progress="runtimeProgress"
      :events="runtimeEvents"
      :steps="wizardSteps"
      :action-running="runtimeActionRunning"
      :setup-started="setupStarted"
      :services="services"
      @run="bootstrapRuntime"
      @cancel="cancelRuntime"
      @open-business="openBusinessHome"
    />
  </div>
</template>

<style scoped>
.app-shell {
  height: 100vh;
  min-height: 100vh;
  overflow: hidden;
}
</style>
