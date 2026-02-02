import { defineConfig } from "tsup";

export default defineConfig({
  entry: {
    "memory-retrieval": "scripts/src/memory-retrieval.ts",
    "memory-commit": "scripts/src/memory-commit.ts",
    "memory-reflect": "scripts/src/memory-reflect.ts",
    "memory-health": "scripts/src/memory-health.ts",
    "memory-stats": "scripts/src/memory-stats.ts",
  },
  format: ["esm"],
  target: "node20",
  outDir: "scripts/dist",
  outExtension: () => ({ js: ".mjs" }),
  clean: true,
  bundle: true,
  splitting: false,
});
