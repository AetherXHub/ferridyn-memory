#!/usr/bin/env node
// UserPromptSubmit hook — inject relevant memories into Claude's context.

import { runCli, callHaiku, parseJsonFromText, readStdin } from "./config.js";
import type {
  HookInput,
  PromptHookOutput,
  MemorySelection,
  MemoryGroup,
  MemoryItem,
} from "./types.js";

// ---------------------------------------------------------------------------
// Prompts
// ---------------------------------------------------------------------------

const RETRIEVAL_PROMPT = `You are a memory retrieval assistant. Given a user prompt and a memory index, select which memory categories and optional prefixes are most relevant to the prompt.

Return a JSON array of objects: [{"category": "...", "prefix": "..."}]
- "prefix" is optional — omit it to fetch all entries in the category.
- Return an empty array [] if no memories are relevant.
- Be selective: only return categories that are clearly related to the prompt.
- Maximum 5 entries.`;

const MEMORY_PROTOCOL = `# Memory Protocol

You have access to persistent memory via the fmemory CLI. Use it proactively:

**COMMIT** — Run \`fmemory remember\` when:
- You make or discuss a significant decision (architecture, technology, design)
- You learn something important about the project, codebase, or user preferences
- The user says "remember that...", "note that...", "keep in mind...", or "from now on..."
- You fix a tricky bug (store the pattern and fix)
- You discover a convention, gotcha, or non-obvious behavior

**RETRIEVE** — Run \`fmemory recall\` when:
- You're starting complex work and need background context
- You need to know project conventions, architecture decisions, or user preferences
- The user asks about something that might be stored in memory
- You're making a decision that might conflict with a previous one

**ASK & STORE** — When you expect information to be in memory but it's missing:
- Ask the user for the information
- Store their answer so future sessions have it

**CORRECT** — Run \`fmemory forget\` then \`fmemory remember\` when:
- Stored information contradicts what you now know
- The user corrects previously stored information
- Project structure has changed (after refactors, renames, upgrades)

Available skills: /ferridyn-memory:teach, /ferridyn-memory:reflect, /ferridyn-memory:context, /ferridyn-memory:update, /ferridyn-memory:decide, /ferridyn-memory:status`;

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

async function main(): Promise<void> {
  const input: HookInput = await readStdin();
  const prompt = input.prompt;

  if (!prompt) {
    // No prompt text — nothing to do.
    process.exit(0);
  }

  // Step 1-4: Try to discover and fetch relevant memories.
  const memories: MemoryGroup[] = [];
  try {
    // Step 1: Discover all categories.
    const categories = (await runCli(["discover"])) as string[];

    if (Array.isArray(categories) && categories.length > 0) {
      // Step 2: Build memory index (categories + their prefixes).
      const index: Record<string, string[]> = {};
      for (const cat of categories) {
        const catName = typeof cat === "string" ? cat : String(cat);
        try {
          const prefixes = (await runCli([
            "discover",
            "--category",
            catName,
          ])) as string[];
          index[catName] = Array.isArray(prefixes) ? prefixes : [];
        } catch {
          index[catName] = [];
        }
      }

      const indexText = Object.entries(index)
        .map(
          ([cat, pfxs]) =>
            `- ${cat}: [${pfxs.map((p) => (typeof p === "string" ? p : String(p))).join(", ")}]`,
        )
        .join("\n");

      // Step 3: Select relevant memories.
      let selections: MemorySelection[];

      // Try LLM-based selection first.
      const llmResponse = await callHaiku(
        RETRIEVAL_PROMPT,
        `Memory index:\n${indexText}\n\nUser prompt:\n${prompt}`,
      );
      selections = parseJsonFromText(llmResponse ?? "") as MemorySelection[];

      if (!Array.isArray(selections) || selections.length === 0) {
        // Fallback: fetch from all categories (limited to 5 per category).
        selections = categories
          .slice(0, 5)
          .map((c) => ({ category: typeof c === "string" ? c : String(c) }));
      }

      // Step 4: Fetch selected memories.
      for (const sel of selections.slice(0, 5)) {
        try {
          const args = ["recall", "--category", sel.category, "--limit", "10"];
          if (sel.prefix) {
            args.push("--prefix", sel.prefix);
          }
          const items = (await runCli(args)) as MemoryItem[];
          if (Array.isArray(items) && items.length > 0) {
            memories.push({ category: sel.category, prefix: sel.prefix, items });
          }
        } catch {
          // Skip failures.
        }
      }
    }
  } catch {
    // CLI not available or other errors — proceed with protocol only.
  }

  // Step 5: Build output — always include protocol, optionally include memories.
  let context: string;
  if (memories.length > 0) {
    const contextParts = memories.map(({ category, prefix, items }) => {
      const header = prefix ? `${category} (${prefix})` : category;
      const entries = items
        .map((item) => {
          const key = item.key || "?";
          const content = item.content || JSON.stringify(item);
          return `  - [${key}]: ${content}`;
        })
        .join("\n");
      return `## ${header}\n${entries}`;
    });

    context = `# Recalled Memories\n\n${contextParts.join("\n\n")}\n\n${MEMORY_PROTOCOL}`;
  } else {
    // No memories found — output protocol only.
    context = MEMORY_PROTOCOL;
  }

  const output: PromptHookOutput = {
    hookSpecificOutput: {
      hookEventName: "UserPromptSubmit",
      additionalContext: context,
    },
  };

  process.stdout.write(JSON.stringify(output));
}

main().catch((err: Error) => {
  process.stderr.write(`memory-retrieval error: ${err.message}\n`);
  // Exit 0 so we don't block the prompt.
  process.exit(0);
});
