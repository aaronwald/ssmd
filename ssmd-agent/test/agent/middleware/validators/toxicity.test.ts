import { assertEquals } from "https://deno.land/std@0.224.0/assert/mod.ts";
import { checkToxicity } from "../../../../src/agent/middleware/validators/toxicity.ts";

Deno.test("checkToxicity - detects profanity", () => {
  const result = checkToxicity("This is damn stupid");
  assertEquals(result.toxic, true);
});

Deno.test("checkToxicity - detects threats", () => {
  const result = checkToxicity("I will kill you");
  assertEquals(result.toxic, true);
  assertEquals(result.category, "threat");
});

Deno.test("checkToxicity - detects hate speech patterns", () => {
  const result = checkToxicity("All those people are worthless");
  assertEquals(result.toxic, true);
});

Deno.test("checkToxicity - allows normal text", () => {
  const result = checkToxicity("The market is trading at $50.");
  assertEquals(result.toxic, false);
});

Deno.test("checkToxicity - allows hedged negative language", () => {
  const result = checkToxicity("This approach might not work well.");
  assertEquals(result.toxic, false);
});

Deno.test("checkToxicity - returns category for matches", () => {
  const result = checkToxicity("You idiot!");
  assertEquals(result.toxic, true);
  assertEquals(result.category !== undefined, true);
});
