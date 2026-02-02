// Shared type definitions for ferridyn-memory hook scripts

// Hook input - the JSON received from stdin by all hooks
export interface HookInput {
  session_id?: string;
  transcript_path?: string;
  cwd?: string;
  permission_mode?: string;
  hook_event_name?: string;
  // UserPromptSubmit-specific
  prompt?: string;
  // Stop-specific
  stop_hook_active?: boolean;
}

// UserPromptSubmit hook output format
export interface PromptHookOutput {
  hookSpecificOutput: {
    hookEventName: string;
    additionalContext: string;
  };
}

// Memory item as returned by the CLI
export interface MemoryItem {
  category: string;
  key: string;
  content: string;
  metadata?: string;
}

// Selection for retrieval (from Haiku)
export interface MemorySelection {
  category: string;
  prefix?: string;
}

// Memory to extract and store (from Haiku)
export interface ExtractedMemory {
  category: string;
  key: string;
  content: string;
  metadata?: string;
}

// Transcript entry from JSONL file
export interface TranscriptEntry {
  role?: string;
  content?: string | object;
  type?: string;
  message?: {
    role?: string;
    content?: string | object;
  };
  raw?: string;
}

// Fetched memory group (category + items)
export interface MemoryGroup {
  category: string;
  prefix?: string;
  items: MemoryItem[];
}

// Health report
export interface HealthIssue {
  severity: "error" | "warning" | "info";
  category?: string;
  issue: string;
}

export interface CategoryHealth {
  name: string;
  entries: number;
  has_schema: boolean;
  prefixes: string[];
}

export interface HealthReport {
  total_categories: number;
  total_entries: number;
  schema_coverage: string;
  issues: HealthIssue[];
  categories: CategoryHealth[];
}

// Stats report
export interface CategoryStats {
  name: string;
  description: string;
  key_format: string;
  entry_count: number;
  prefixes: string[];
  sample_keys: string[];
}

export interface StatsReport {
  total_categories: number;
  total_entries: number;
  categories: CategoryStats[];
}
