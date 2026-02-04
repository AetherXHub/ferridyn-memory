// Stop hook — reflect on the session and persist high-level learnings.

import { runCli, callHaiku, parseJsonFromText, readStdin, readTranscriptTail } from "./config.js";
import type { ExtractedMemory, TranscriptEntry } from "./types.js";

const REFLECT_PROMPT = `You are a session reflection assistant. Given a conversation transcript, extract the most important HIGH-LEVEL learnings that should be remembered for future sessions.

Focus on:
- Architectural and design decisions made (with rationale)
- User preferences and workflow patterns revealed
- Project conventions discovered or established
- Non-obvious gotchas or patterns that future sessions should know
- Bug patterns and their root causes

Do NOT extract:
- Granular facts (those are handled by the commit hook)
- Step-by-step task progress
- Trivial exchanges or status updates
- Information that's already in CLAUDE.md or AGENTS.md

Existing memory categories: {categories}

Return a JSON array of memory entries to store:
[
  {
    "category": "category-name",
    "input": "Natural language description — include the WHY, not just the WHAT"
  }
]

Maximum 5 entries. Quality over quantity. Return [] if nothing worth reflecting on.`;

async function main(): Promise<void> {
  const input = await readStdin();

  // Don't reflect if another stop hook is already handling things
  if (input.stop_hook_active) {
    process.exit(0);
  }

  const transcriptPath = input.transcript_path;
  if (!transcriptPath) {
    process.exit(0);
  }

  // Read more of the transcript than commit (100 vs 50) for fuller context
  const entries: TranscriptEntry[] = await readTranscriptTail(transcriptPath, 100);
  if (entries.length === 0) {
    process.exit(0);
  }

  // Get existing categories for context
  let categories: string[] = [];
  try {
    const cats = await runCli(["discover"]) as Array<{ name: string }>;
    if (Array.isArray(cats)) {
      categories = cats.map((c) => c.name);
    }
  } catch {
    // No existing categories — fine
  }

  // Extract conversation text
  const conversationText = entries
    .map((entry) => {
      if (entry.role && entry.content) {
        const text = typeof entry.content === "string"
          ? entry.content
          : JSON.stringify(entry.content);
        return `[${entry.role}]: ${text.slice(0, 500)}`;
      }
      if (entry.type === "message" && entry.message) {
        const role = entry.message.role || "unknown";
        const content = entry.message.content;
        const text = typeof content === "string" ? content : JSON.stringify(content);
        return `[${role}]: ${text.slice(0, 500)}`;
      }
      return null;
    })
    .filter(Boolean)
    .join("\n");

  if (!conversationText.trim()) {
    process.exit(0);
  }

  // Use Haiku to extract reflections
  const systemPrompt = REFLECT_PROMPT.replace(
    "{categories}",
    categories.length > 0 ? categories.join(", ") : "(none yet)",
  );

  const llmResponse = await callHaiku(systemPrompt, `Session transcript:\n${conversationText}`);
  const memories = parseJsonFromText(llmResponse) as ExtractedMemory[] | null;

  if (!Array.isArray(memories) || memories.length === 0) {
    process.stderr.write("memory-reflect: no reflections extracted\n");
    process.exit(0);
  }

  // Store each reflection
  let stored = 0;
  for (const mem of memories.slice(0, 5)) {
    if (!mem.category || !mem.input) continue;
    try {
      await runCli(["remember", "--category", mem.category, mem.input]);
      stored++;
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err);
      process.stderr.write(`memory-reflect: failed to store in ${mem.category}: ${message}\n`);
    }
  }

  process.stderr.write(`memory-reflect: stored ${stored} reflections\n`);
  // Don't output decision/reason — let Claude stop normally
}

main().catch((err: unknown) => {
  const message = err instanceof Error ? err.message : String(err);
  process.stderr.write(`memory-reflect error: ${message}\n`);
  process.exit(0);
});
