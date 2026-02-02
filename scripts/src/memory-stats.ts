// Memory stats — produce structured statistics for the status skill.

import { runCli } from "./config.js";
import type { StatsReport, CategoryStats } from "./types.js";

async function main(): Promise<void> {
  const categoryStats: CategoryStats[] = [];
  let totalEntries = 0;

  // Step 1: Discover all categories with descriptions
  let categories: unknown[];
  try {
    const result = await runCli(["discover"]);
    categories = Array.isArray(result) ? result : [];
  } catch {
    const report: StatsReport = {
      total_categories: 0,
      total_entries: 0,
      categories: [],
    };
    process.stdout.write(JSON.stringify(report, null, 2));
    return;
  }

  // Step 2: Get details per category
  for (const cat of categories) {
    const catName = typeof cat === "string" ? cat : String(cat);

    // Get prefixes
    let prefixes: string[] = [];
    try {
      const result = await runCli(["discover", "--category", catName]);
      prefixes = Array.isArray(result) ? result.map((p) => typeof p === "string" ? p : String(p)) : [];
    } catch {
      // Skip
    }

    // Get schema info
    let description = "";
    let keyFormat = "";
    try {
      const schemaResult = await runCli(["schema", "--category", catName]) as Record<string, unknown> | null;
      if (schemaResult && typeof schemaResult === "object") {
        description = String(schemaResult.description || "");
        keyFormat = String(schemaResult.sort_key_format || "");
      }
    } catch {
      // No schema
    }

    // Count entries and get sample keys
    let entryCount = 0;
    let sampleKeys: string[] = [];
    try {
      const items = await runCli(["recall", "--category", catName, "--limit", "1000"]) as Array<{ key?: string }>;
      if (Array.isArray(items)) {
        entryCount = items.length;
        sampleKeys = items
          .slice(0, 5)
          .map((item) => item.key || "?")
          .filter((k) => k !== "?");
      }
    } catch {
      // Skip
    }

    totalEntries += entryCount;

    categoryStats.push({
      name: catName,
      description,
      key_format: keyFormat,
      entry_count: entryCount,
      prefixes,
      sample_keys: sampleKeys,
    });
  }

  const report: StatsReport = {
    total_categories: categories.length,
    total_entries: totalEntries,
    categories: categoryStats,
  };

  process.stdout.write(JSON.stringify(report, null, 2));
}

main().catch((err: unknown) => {
  const message = err instanceof Error ? err.message : String(err);
  process.stderr.write(`memory-stats error: ${message}\n`);
  process.exit(1);
});
