// ssmd-agent/src/agent/prompt.ts
import type { Skill } from "./skills.ts";
import { config } from "../config.ts";

export async function loadSystemPrompt(): Promise<string> {
  const promptPath = `${config.promptsPath}/system.md`;
  try {
    const content = await Deno.readTextFile(promptPath);
    // Strip frontmatter if present
    const frontmatterMatch = content.match(/^---\n[\s\S]*?\n---\n/);
    if (frontmatterMatch) {
      return content.slice(frontmatterMatch[0].length).trim();
    }
    return content.trim();
  } catch (error) {
    if (error instanceof Deno.errors.NotFound) {
      throw new Error(`System prompt not found: ${promptPath}`);
    }
    throw error;
  }
}

export function formatSkills(skills: Skill[]): string {
  if (skills.length === 0) {
    return "No skills loaded.";
  }
  return skills
    .map((s) => `### ${s.name}\n${s.description}\n\n${s.content}`)
    .join("\n\n---\n\n");
}

export async function buildSystemPrompt(skills: Skill[]): Promise<string> {
  const template = await loadSystemPrompt();
  const skillsSection = formatSkills(skills);
  return template.replace("{{skills}}", skillsSection);
}
