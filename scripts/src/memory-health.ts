// Memory health diagnostic — check integrity of stored memories.

import { runCli } from "./config.js";
import type { HealthReport, HealthIssue, CategoryHealth } from "./types.js";

async function main(): Promise<void> {
  const issues: HealthIssue[] = [];
  const categoryDetails: CategoryHealth[] = [];
  let totalEntries = 0;
  let schemasFound = 0;

  // Step 1: Discover all categories (enriched objects)
  let categories: Array<{ name: string; description?: string; attribute_count?: number; index_count?: number }>;
  try {
    const result = await runCli(["discover"]);
    categories = Array.isArray(result) ? result as Array<{ name: string; description?: string; attribute_count?: number; index_count?: number }> : [];
  } catch {
    // CLI not available — output error report
    const report: HealthReport = {
      total_categories: 0,
      total_entries: 0,
      schema_coverage: "0/0",
      issues: [{ severity: "error", issue: "Cannot connect to memory system (CLI unavailable)" }],
      categories: [],
    };
    process.stdout.write(JSON.stringify(report, null, 2));
    return;
  }

  if (categories.length === 0) {
    const report: HealthReport = {
      total_categories: 0,
      total_entries: 0,
      schema_coverage: "0/0",
      issues: [{ severity: "info", issue: "No memories stored yet" }],
      categories: [],
    };
    process.stdout.write(JSON.stringify(report, null, 2));
    return;
  }

  // Step 2: Check each category
  for (const cat of categories) {
    const catName = cat.name;

    // Get keys, schema, and indexes via discover --category
    let catResult: { category: string; keys: string[]; schema?: unknown; indexes?: unknown[] } = {
      category: catName,
      keys: [],
    };
    try {
      const result = await runCli(["discover", "--category", catName]);
      catResult = result as typeof catResult;
    } catch {
      issues.push({ severity: "warning", category: catName, issue: "Failed to discover category details" });
    }

    const hasSchema = !!catResult.schema;
    const hasIndexes = Array.isArray(catResult.indexes) && catResult.indexes.length > 0;
    if (hasSchema) schemasFound++;

    // Count entries by recalling with high limit
    let entryCount = 0;
    try {
      const items = await runCli(["recall", "--category", catName, "--limit", "1000"]);
      entryCount = Array.isArray(items) ? items.length : 0;
    } catch {
      issues.push({ severity: "warning", category: catName, issue: "Failed to count entries" });
    }

    totalEntries += entryCount;

    // Check for issues
    if (!hasSchema) {
      issues.push({ severity: "warning", category: catName, issue: "No schema defined" });
    }
    if (entryCount === 0) {
      issues.push({ severity: "info", category: catName, issue: "Empty category (no entries)" });
    }
    if (entryCount > 100) {
      issues.push({ severity: "info", category: catName, issue: `Large category: ${entryCount} entries` });
    }

    categoryDetails.push({
      name: catName,
      entries: entryCount,
      has_schema: hasSchema,
      has_indexes: hasIndexes,
    });
  }

  const report: HealthReport = {
    total_categories: categories.length,
    total_entries: totalEntries,
    schema_coverage: `${schemasFound}/${categories.length} (${Math.round(schemasFound / categories.length * 100)}%)`,
    issues,
    categories: categoryDetails,
  };

  process.stdout.write(JSON.stringify(report, null, 2));
}

main().catch((err: unknown) => {
  const message = err instanceof Error ? err.message : String(err);
  process.stderr.write(`memory-health error: ${message}\n`);
  process.exit(1);
});
