import antfu from "@antfu/eslint-config"

export default antfu(
  {
    vue: true,
    typescript: true,
    stylistic: {
      quotes: "double",
      semi: false,
      commaDangle: "always-multiline",
    },
    ignores: [
      ".mypy_cache/**/*",
      "src-tauri/gen/schemas/**/*",
      "src-tauri/target/**/*",
    ],
  },
  {
    files: ["**/*.vue"],
    rules: {
      "vue/one-component-per-file": 0,
      "vue/no-reserved-component-names": 0,
      "vue/no-useless-v-bind": 0,
    },
  },
  {
    rules: {
      "no-console": 1,
      "symbol-description": 0,
      "node/prefer-global/process": 0,
      "unused-imports/no-unused-vars": 0,
      "e18e/prefer-array-at": 0,
      "e18e/prefer-static-regex": 0,
      "ts/no-empty-object-type": 0,
    },
  },
)
