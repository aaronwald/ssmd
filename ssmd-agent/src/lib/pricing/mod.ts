import { getRedis } from "../redis/mod.ts";

const OPENROUTER_API_URL = "https://openrouter.ai/api/v1/models";
const CACHE_TTL_SECONDS = 60 * 60; // 1 hour
const CACHE_KEY_PREFIX = "openrouter:pricing:";

export interface ModelPricing {
  prompt: number;      // USD per token
  completion: number;  // USD per token
}

interface OpenRouterModel {
  id: string;
  pricing: {
    prompt: string;
    completion: string;
  };
}

interface OpenRouterModelsResponse {
  data: OpenRouterModel[];
}

/**
 * Get pricing for a specific model from cache or API.
 */
export async function getModelPricing(model: string): Promise<ModelPricing | null> {
  try {
    const redis = await getRedis();
    const cacheKey = `${CACHE_KEY_PREFIX}${model}`;

    // Try cache first
    const cached = await redis.get(cacheKey);
    if (cached) {
      return JSON.parse(cached) as ModelPricing;
    }

    // Fetch from API and cache
    await refreshPricingCache();

    // Try cache again
    const refreshed = await redis.get(cacheKey);
    if (refreshed) {
      return JSON.parse(refreshed) as ModelPricing;
    }

    return null;
  } catch (error) {
    console.error(`Failed to get pricing for model ${model}:`, error);
    return null;
  }
}

/**
 * Fetch all model pricing from OpenRouter and cache in Redis.
 */
export async function refreshPricingCache(): Promise<void> {
  try {
    const response = await fetch(OPENROUTER_API_URL);
    if (!response.ok) {
      throw new Error(`OpenRouter API returned ${response.status}`);
    }

    const data = await response.json() as OpenRouterModelsResponse;
    const redis = await getRedis();

    // Cache each model's pricing
    for (const model of data.data) {
      const pricing: ModelPricing = {
        prompt: parseFloat(model.pricing.prompt) || 0,
        completion: parseFloat(model.pricing.completion) || 0,
      };

      const cacheKey = `${CACHE_KEY_PREFIX}${model.id}`;
      await redis.setex(cacheKey, CACHE_TTL_SECONDS, JSON.stringify(pricing));
    }

    console.log(`Cached pricing for ${data.data.length} models`);
  } catch (error) {
    console.error("Failed to refresh pricing cache:", error);
    throw error;
  }
}

/**
 * Calculate cost in USD for token usage.
 * Returns 0 if pricing not available.
 */
export async function calculateCost(
  model: string,
  promptTokens: number,
  completionTokens: number
): Promise<number> {
  const pricing = await getModelPricing(model);
  if (!pricing) {
    return 0;
  }

  const promptCost = promptTokens * pricing.prompt;
  const completionCost = completionTokens * pricing.completion;

  // Round to 6 decimal places (micro-dollars precision)
  return Math.round((promptCost + completionCost) * 1_000_000) / 1_000_000;
}

/**
 * Get all cached pricing (for debugging/admin).
 */
export async function getAllCachedPricing(): Promise<Record<string, ModelPricing>> {
  try {
    const redis = await getRedis();
    const result: Record<string, ModelPricing> = {};

    // Scan for all pricing keys
    let cursor = 0;
    do {
      const [nextCursor, keys] = await redis.scan(cursor, {
        pattern: `${CACHE_KEY_PREFIX}*`,
        count: 100,
      });
      cursor = parseInt(nextCursor);

      for (const key of keys) {
        const value = await redis.get(key);
        if (value) {
          const model = key.replace(CACHE_KEY_PREFIX, "");
          result[model] = JSON.parse(value) as ModelPricing;
        }
      }
    } while (cursor !== 0);

    return result;
  } catch (error) {
    console.error("Failed to get all cached pricing:", error);
    return {};
  }
}
