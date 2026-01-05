// scripts/feed-to-configmap.ts
// Converts feed YAML files to Kubernetes ConfigMap manifests
import {
  parse as parseYaml,
  stringify as stringifyYaml,
} from "https://deno.land/std@0.224.0/yaml/mod.ts";

interface FeedConfig {
  name: string;
  display_name: string;
  type: string;
  status: string;
  versions: unknown[];
  defaults?: {
    connector?: Record<string, unknown>;
    archiver?: Record<string, unknown>;
    signal?: Record<string, unknown>;
  };
}

async function main() {
  const feedsDir = Deno.args[0] || "exchanges/feeds";
  const outputDir = Deno.args[1] || "generated/k8s";

  await Deno.mkdir(outputDir, { recursive: true });

  let count = 0;
  for await (const entry of Deno.readDir(feedsDir)) {
    if (!entry.name.endsWith(".yaml")) continue;

    const feedPath = `${feedsDir}/${entry.name}`;
    const content = await Deno.readTextFile(feedPath);
    const feed = parseYaml(content) as FeedConfig;

    const configMap = {
      apiVersion: "v1",
      kind: "ConfigMap",
      metadata: {
        name: `feed-${feed.name}`,
        namespace: "ssmd",
        labels: {
          "ssmd.io/feed": feed.name,
          "ssmd.io/generated": "true",
        },
      },
      data: {
        "feed.yaml": content,
      },
    };

    const outputPath = `${outputDir}/feed-${feed.name}.yaml`;
    await Deno.writeTextFile(outputPath, stringifyYaml(configMap));
    console.log(`Generated: ${outputPath}`);
    count++;
  }

  console.log(`\nGenerated ${count} ConfigMap(s)`);
}

main();
