// Shared configuration and utilities for ferridyn-memory hook scripts

import { execFile } from "node:child_process";
import { readFile } from "node:fs/promises";
import { resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import type { HookInput, TranscriptEntry } from "./types.js";

// Resolve plugin root directory
// Use CLAUDE_PLUGIN_ROOT env var if available, otherwise resolve from built file location
const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);
// Built files are in scripts/dist/, so go up twice to reach plugin root
const PLUGIN_ROOT = process.env.CLAUDE_PLUGIN_ROOT || resolve(__dirname, "../..");

// CLI binary location
const CLI_BIN = process.env.FERRIDYN_MEMORY_CLI || resolve(PLUGIN_ROOT, "target", "release", "fmemory");

/**
 * Execute the ferridyn-memory CLI with the given arguments
 * Returns parsed JSON if possible, otherwise raw stdout string
 */
export function runCli(args: string[], options: { timeout?: number } = {}): Promise<unknown> {
  const { timeout = 10_000 } = options;

  return new Promise((resolve, reject) => {
    execFile(CLI_BIN, ["--json", ...args], { timeout }, (err, stdout, stderr) => {
      if (err) {
        reject(new Error(`CLI error: ${err.message}\nstderr: ${stderr}`));
        return;
      }
      try {
        resolve(JSON.parse(stdout));
      } catch {
        resolve(stdout.trim());
      }
    });
  });
}

/**
 * Call Claude Haiku via Anthropic API or local claude CLI
 * Returns the response text, or null if both methods fail
 */
export async function callHaiku(systemPrompt: string, userMessage: string): Promise<string | null> {
  const apiKey = process.env.ANTHROPIC_API_KEY;

  // Try Anthropic API first
  if (apiKey) {
    try {
      const res = await fetch("https://api.anthropic.com/v1/messages", {
        method: "POST",
        headers: {
          "x-api-key": apiKey,
          "anthropic-version": "2023-06-01",
          "content-type": "application/json",
        },
        body: JSON.stringify({
          model: "claude-haiku-4-5",
          max_tokens: 2048,
          system: systemPrompt,
          messages: [{ role: "user", content: userMessage }],
        }),
      });

      if (res.ok) {
        const data = await res.json();
        const text = data.content?.[0]?.text;
        if (text) return text;
      }
    } catch {
      // Fall through to CLI fallback
    }
  }

  // Try local claude CLI fallback
  try {
    const text = await new Promise<string>((resolve, reject) => {
      const input = `${systemPrompt}\n\n${userMessage}`;
      const child = execFile("claude", ["-p", "--model", "haiku", "--no-input"],
        { timeout: 30_000 },
        (err, stdout) => {
          if (err) reject(err);
          else resolve(stdout.trim());
        }
      );

      // Write the prompt to stdin
      if (child.stdin) {
        child.stdin.write(input);
        child.stdin.end();
      }
    });

    if (text) return text;
  } catch {
    // Both methods failed
  }

  return null;
}

/**
 * Extract JSON from text that may contain markdown code fences or surrounding text
 */
export function parseJsonFromText(text: string): unknown {
  if (!text) return null;

  // Try parsing directly
  try {
    return JSON.parse(text);
  } catch {
    /* continue */
  }

  // Try extracting from markdown code fence
  const fenceMatch = text.match(/```(?:json)?\s*\n?([\s\S]*?)\n?```/);
  if (fenceMatch) {
    try {
      return JSON.parse(fenceMatch[1]);
    } catch {
      /* continue */
    }
  }

  // Try finding JSON object/array in text
  const jsonMatch = text.match(/[\[{][\s\S]*[}\]]/);
  if (jsonMatch) {
    try {
      return JSON.parse(jsonMatch[0]);
    } catch {
      /* give up */
    }
  }

  return null;
}

/**
 * Read and parse JSON input from stdin
 */
export async function readStdin(): Promise<HookInput> {
  const chunks: Buffer[] = [];
  for await (const chunk of process.stdin) {
    chunks.push(chunk);
  }
  const raw = Buffer.concat(chunks).toString("utf-8");
  return JSON.parse(raw);
}

/**
 * Read the last N entries from a JSONL transcript file
 */
export async function readTranscriptTail(transcriptPath: string, maxEntries = 50): Promise<TranscriptEntry[]> {
  try {
    const content = await readFile(transcriptPath, "utf-8");
    const lines = content.trim().split("\n").filter(Boolean);
    const tail = lines.slice(-maxEntries);

    return tail.map((line) => {
      try {
        return JSON.parse(line);
      } catch {
        return { raw: line };
      }
    });
  } catch {
    return [];
  }
}
