// ssmd-agent/src/agent/skills.ts
import { config } from "../config.ts";

export interface Skill {
  name: string;
  description: string;
  content: string;
}

interface Frontmatter {
  name: string;
  description: string;
}

function parseFrontmatter(text: string): { frontmatter: Frontmatter; body: string } {
  const match = text.match(/^---\n([\s\S]*?)\n---\n([\s\S]*)$/);
  if (!match) {
    return {
      frontmatter: { name: "unknown", description: "" },
      body: text,
    };
  }

  const yamlStr = match[1];
  const body = match[2];

  // Simple YAML parsing for name/description
  const lines = yamlStr.split("\n");
  const frontmatter: Frontmatter = { name: "", description: "" };

  for (const line of lines) {
    const [key, ...rest] = line.split(":");
    const value = rest.join(":").trim();
    if (key.trim() === "name") frontmatter.name = value;
    if (key.trim() === "description") frontmatter.description = value;
  }

  return { frontmatter, body };
}

export async function loadSkills(): Promise<Skill[]> {
  const skills: Skill[] = [];

  try {
    for await (const entry of Deno.readDir(config.skillsPath)) {
      if (entry.isFile && entry.name.endsWith(".md")) {
        const content = await Deno.readTextFile(`${config.skillsPath}/${entry.name}`);
        const { frontmatter, body } = parseFrontmatter(content);
        skills.push({
          name: frontmatter.name,
          description: frontmatter.description,
          content: body,
        });
      }
    }
  } catch (e) {
    if (!(e instanceof Deno.errors.NotFound)) {
      throw e;
    }
    // Skills directory doesn't exist yet, return empty
  }

  return skills;
}
