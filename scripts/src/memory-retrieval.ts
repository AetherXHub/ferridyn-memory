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

const RETRIEVAL_PROMPT = `You are a memory retrieval assistant. Given a user prompt and a memory index, select which memory categories are most relevant to the prompt.

Return a JSON array of objects: [{"category": "..."}]
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
- Syntax: \`fmemory remember --category <cat> "Natural language description"\`
- Or let the system infer the category: \`fmemory remember "Natural language description"\`
- Or use \`fmemory -p "Natural language"\` for intent-classified prompt mode

**RETRIEVE** — Run \`fmemory recall --query "..."\` when:
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
    // Step 1: Discover all categories (enriched objects).
    const discoverResult = (await runCli(["discover"])) as Array<{
      name: string;
      description?: string;
      attribute_count?: number;
      index_count?: number;
    }>;

    if (Array.isArray(discoverResult) && discoverResult.length > 0) {
      // Step 2: Build memory index (categories + their keys).
      const indexText = discoverResult
        .map((c) => `- ${c.name}: ${c.description || "(no description)"}`)
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
        // Fallback: fetch from all categories (limited to 5).
        selections = discoverResult
          .slice(0, 5)
          .map((c) => ({ category: c.name }));
      }

      // Step 4: Fetch selected memories.
      for (const sel of selections.slice(0, 5)) {
        try {
          const args = ["recall", "--category", sel.category, "--limit", "10"];
          const items = (await runCli(args)) as MemoryItem[];
          if (Array.isArray(items) && items.length > 0) {
            memories.push({ category: sel.category, items });
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
    const contextParts = memories.map(({ category, items }) => {
      const entries = items
        .map((item) => {
          const key = item.key || "?";
          const attrs = Object.entries(item)
            .filter(([k]) => k !== "category" && k !== "key")
            .map(([k, v]) => `${k}: ${v}`)
            .join(", ");
          return `  - [${key}]: ${attrs || "(empty)"}`;
        })
        .join("\n");
      return `## ${category}\n${entries}`;
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
