import { defineConfig } from "bumpp"

export default defineConfig({
  files: [
    "package.json",
    "src-tauri/Cargo.toml",
    "src-tauri/tauri.conf.json",
  ],
  commit: "release: v%s",
  tag: "v%s",
  push: true,
  install: true,
  execute: "cargo check --manifest-path src-tauri/Cargo.toml",
  all: true,
})
