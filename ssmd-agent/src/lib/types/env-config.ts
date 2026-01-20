// Environment configuration types and Zod schemas
// Config file: ~/.ssmd/config.yaml
import { z } from "zod";

// NATS configuration per environment
export const NatsConfigSchema = z.object({
  url: z.string().default("nats://nats.nats.svc.cluster.local:4222"),
  stream_prefix: z.string(), // DEV, STAGING, PROD
});

// Storage configuration (S3 or local)
export const StorageConfigSchema = z.discriminatedUnion("type", [
  z.object({
    type: z.literal("s3"),
    bucket: z.string(),
    region: z.string().optional(),
  }),
  z.object({
    type: z.literal("gcs"),
    bucket: z.string(),
  }),
  z.object({
    type: z.literal("local"),
    path: z.string(),
  }),
]);

// Secrets references (namespace/name format)
export const SecretsConfigSchema = z.object({
  kalshi: z.string().optional(), // e.g., "ssmd/kalshi-credentials"
});

// Single environment configuration
export const EnvironmentSchema = z.object({
  cluster: z.string(), // kubeconfig context name
  namespace: z.string(), // kubernetes namespace
  nats: NatsConfigSchema,
  storage: StorageConfigSchema.optional(),
  secrets: SecretsConfigSchema.optional(),
});

// Root config file schema
export const EnvConfigSchema = z.object({
  "current-env": z.string(),
  environments: z.record(z.string(), EnvironmentSchema),
});

// Type exports
export type NatsConfig = z.infer<typeof NatsConfigSchema>;
export type StorageConfig = z.infer<typeof StorageConfigSchema>;
export type SecretsConfig = z.infer<typeof SecretsConfigSchema>;
export type Environment = z.infer<typeof EnvironmentSchema>;
export type EnvConfig = z.infer<typeof EnvConfigSchema>;

// Default config with prod and dev environments
export function getDefaultConfig(): EnvConfig {
  return {
    "current-env": "prod",
    environments: {
      prod: {
        cluster: "homelab",
        namespace: "ssmd",
        nats: {
          url: "nats://nats.nats.svc.cluster.local:4222",
          stream_prefix: "PROD",
        },
        storage: {
          type: "local",
          path: "/mnt/ssmd-data",
        },
      },
      dev: {
        cluster: "gke-ssmd-dev",
        namespace: "ssmd-dev",
        nats: {
          url: "nats://nats.nats.svc.cluster.local:4222",
          stream_prefix: "DEV",
        },
        storage: {
          type: "gcs",
          bucket: "ssmd-dev-archives",
        },
      },
    },
  };
}
