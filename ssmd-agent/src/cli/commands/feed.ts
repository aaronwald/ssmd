// Feed commands: list, show, create
import { parse as parseYaml, stringify as stringifyYaml } from "yaml";
import { join } from "https://deno.land/std@0.224.0/path/mod.ts";
import {
  FeedSchema,
  type Feed,
  type FeedType,
  getLatestVersion,
} from "../../lib/types/feed.ts";
import { TablePrinter } from "../utils/table.ts";

export interface CreateFeedOptions {
  type: FeedType;
  displayName?: string;
  endpoint?: string;
  authMethod?: string;
  rateLimit?: number;
}

/**
 * List all feeds in a directory
 */
export async function listFeeds(feedsDir: string): Promise<Feed[]> {
  const feeds: Feed[] = [];

  try {
    for await (const entry of Deno.readDir(feedsDir)) {
      if (entry.isFile && entry.name.endsWith(".yaml")) {
        try {
          const content = await Deno.readTextFile(join(feedsDir, entry.name));
          const data = parseYaml(content);
          const feed = FeedSchema.parse(data);
          feeds.push(feed);
        } catch (e) {
          console.error(`Warning: Failed to parse ${entry.name}: ${(e as Error).message}`);
        }
      }
    }
  } catch (e) {
    if (!(e instanceof Deno.errors.NotFound)) throw e;
  }

  return feeds.sort((a, b) => a.name.localeCompare(b.name));
}

/**
 * Get a specific feed by name
 */
export async function showFeed(feedsDir: string, name: string): Promise<Feed | null> {
  const path = join(feedsDir, `${name}.yaml`);

  try {
    const content = await Deno.readTextFile(path);
    const data = parseYaml(content);
    return FeedSchema.parse(data);
  } catch (e) {
    if (e instanceof Deno.errors.NotFound) return null;
    throw e;
  }
}

/**
 * Create a new feed
 */
export async function createFeed(
  feedsDir: string,
  name: string,
  options: CreateFeedOptions
): Promise<void> {
  const path = join(feedsDir, `${name}.yaml`);

  // Check if exists
  try {
    await Deno.stat(path);
    throw new Error(`Feed '${name}' already exists`);
  } catch (e) {
    if (!(e instanceof Deno.errors.NotFound)) throw e;
  }

  const today = new Date().toISOString().split("T")[0];
  const endpoint = options.endpoint ?? `wss://${name}.example.com/api`;

  const feed: Feed = {
    name,
    display_name: options.displayName,
    type: options.type,
    status: "active",
    versions: [
      {
        version: "v1",
        effective_from: today,
        endpoint,
        protocol: { transport: "wss", message: "json" },
        auth_method: options.authMethod as Feed["versions"][0]["auth_method"],
        rate_limit_per_second: options.rateLimit,
      },
    ],
  };

  // Validate before writing
  FeedSchema.parse(feed);

  // Ensure directory exists
  await Deno.mkdir(feedsDir, { recursive: true });

  const yaml = stringifyYaml(feed, { indent: 2 });
  await Deno.writeTextFile(path, yaml);
}

/**
 * Print feed list as table
 */
export function printFeedList(feeds: Feed[]): void {
  if (feeds.length === 0) {
    console.log("No feeds registered.");
    return;
  }

  const t = new TablePrinter();
  t.header("NAME", "TYPE", "STATUS", "ENDPOINT");

  for (const f of feeds) {
    const latest = getLatestVersion(f);
    const endpoint = latest?.endpoint
      ? latest.endpoint.replace(/^wss?:\/\//, "").split("/")[0]
      : "-";
    t.row(f.name, f.type, f.status, endpoint);
  }

  t.flush();
}

/**
 * Print detailed feed info
 */
export function printFeed(feed: Feed): void {
  console.log(`Name:         ${feed.name}`);
  if (feed.display_name) {
    console.log(`Display Name: ${feed.display_name}`);
  }
  console.log(`Type:         ${feed.type}`);
  console.log(`Status:       ${feed.status}`);
  console.log();

  console.log("Versions:");
  for (const v of feed.versions) {
    const current = v === getLatestVersion(feed) ? " (current)" : "";
    console.log(`  ${v.version}${current}`);
    console.log(`    Effective: ${v.effective_from}${v.effective_to ? ` to ${v.effective_to}` : ""}`);
    console.log(`    Endpoint:  ${v.endpoint}`);
    console.log(`    Protocol:  ${v.protocol.transport}/${v.protocol.message}`);
    if (v.auth_method) {
      console.log(`    Auth:      ${v.auth_method}`);
    }
    if (v.rate_limit_per_second) {
      console.log(`    Rate Limit: ${v.rate_limit_per_second}/sec`);
    }
  }

  if (feed.calendar) {
    console.log();
    console.log("Calendar:");
    if (feed.calendar.timezone) {
      console.log(`  Timezone: ${feed.calendar.timezone}`);
    }
    if (feed.calendar.open_time && feed.calendar.close_time) {
      console.log(`  Hours:    ${feed.calendar.open_time} - ${feed.calendar.close_time}`);
    }
  }
}
