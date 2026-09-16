<script setup lang="ts">
import logoUrl from "@/assets/tauri-embedded-runtime-logo.png"
</script>

<template>
  <header class="runtime-page-header">
    <div class="runtime-page-header-inner">
      <div class="flex min-w-0 items-center gap-2.5">
        <img :src="logoUrl" alt="Tauri Embedded Runtime Logo" class="size-6 shrink-0 rounded-md xl:size-7">
        <span class="truncate text-xs font-medium tracking-[-0.03em] xl:text-sm">Tauri Embedded Runtime</span>
      </div>
      <div class="flex shrink-0 items-center gap-2 sm:gap-4">
        <slot />
      </div>
    </div>
  </header>
</template>
