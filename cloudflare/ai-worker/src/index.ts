interface Env {
  AI: Ai;
  JWT_SECRET: string;
}

interface CustomNutrient {
  key: string;
  name: string;
  unit_label: string;
}

interface NutrientRange {
  min_value: { value: number; unit: string };
  max_value: { value: number; unit: string };
  recommended: { value: number; unit: string };
  comment: string;
}

interface LookupRequest {
  name: string;
  custom_nutrients?: CustomNutrient[];
}

interface VisionRequest {
  images: string[];
  custom_nutrients?: CustomNutrient[];
}

interface LookupResponse {
  name: string | null;
  kcal: NutrientRange;
  protein: NutrientRange;
  fat: NutrientRange;
  carbs: NutrientRange;
  nutrients: Record<string, NutrientRange>;
  package_weight: number | null;
}

const TEXT_MODEL = "@cf/zai-org/glm-4.7-flash" as const;
const VISION_MODEL = "@cf/meta/llama-3.2-11b-vision-instruct" as const;

const CORS_HEADERS: Record<string, string> = {
  "Access-Control-Allow-Origin": "*",
  "Access-Control-Allow-Methods": "GET, POST, OPTIONS",
  "Access-Control-Allow-Headers": "Content-Type, Authorization",
};

async function verifyJwt(token: string, secret: string): Promise<boolean> {
  const parts = token.split(".");
  if (parts.length !== 3) return false;

  const key = await crypto.subtle.importKey(
    "raw",
    new TextEncoder().encode(secret),
    { name: "HMAC", hash: "SHA-256" },
    false,
    ["verify"],
  );

  const sigBuf = base64UrlDecode(parts[2]);
  const data = new TextEncoder().encode(`${parts[0]}.${parts[1]}`);
  return crypto.subtle.verify("HMAC", key, sigBuf, data);
}

function base64UrlDecode(s: string): ArrayBuffer {
  const padded = s.replace(/-/g, "+").replace(/_/g, "/");
  const binary = atob(padded);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i++) bytes[i] = binary.charCodeAt(i);
  return bytes.buffer;
}

function jsonResponse(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { "Content-Type": "application/json", ...CORS_HEADERS },
  });
}

function errorResponse(message: string, status: number): Response {
  return jsonResponse({ error: message }, status);
}

function buildLookupPrompt(name: string, customNutrients?: CustomNutrient[]): string {
  let prompt =
    `You are a nutritional database. For the food item "${name}", provide nutritional values per 100 grams.\n` +
    "For each nutrient (kcal, protein, fat, carbs + any custom nutrients), provide:\n" +
    "- min_value: lowest reasonable value for this food\n" +
    "- max_value: highest reasonable value for this food\n" +
    "- recommended: the most likely value to select\n" +
    "- comment: brief explanation why this value is appropriate\n" +
    "Use these units: kcal for calories, g/mg/mkg/kg for weights.\n" +
    "All values are per 100g of the product.";

  if (customNutrients && customNutrients.length > 0) {
    const keys = customNutrients.map((n) => n.key).join(", ");
    const descriptions = customNutrients
      .map((n) => `${n.key}: ${n.name} (${n.unit_label})`)
      .join(", ");
    prompt +=
      `\nAlso provide values for these custom nutrients in custom_nutrients map. ` +
      `Use ONLY these strings as keys: ${keys}. Reference: ${descriptions}.`;
  }

  return prompt;
}

function buildNutritionSchema(customNutrients?: CustomNutrient[]): Record<string, unknown> {
  const nutrientRangeSchema = {
    type: "object" as const,
    properties: {
      min_value: {
        type: "object" as const,
        properties: {
          value: { type: "number" as const },
          unit: { type: "string" as const },
        },
        required: ["value", "unit"],
      },
      max_value: {
        type: "object" as const,
        properties: {
          value: { type: "number" as const },
          unit: { type: "string" as const },
        },
        required: ["value", "unit"],
      },
      recommended: {
        type: "object" as const,
        properties: {
          value: { type: "number" as const },
          unit: { type: "string" as const },
        },
        required: ["value", "unit"],
      },
      comment: { type: "string" as const },
    },
    required: ["min_value", "max_value", "recommended", "comment"],
  };

  const customNutrientProperties: Record<string, typeof nutrientRangeSchema> = {};
  if (customNutrients && customNutrients.length > 0) {
    for (const n of customNutrients) {
      customNutrientProperties[n.key] = nutrientRangeSchema;
    }
  }

  return {
    type: "object",
    properties: {
      name: { type: ["string", "null"] },
      kcal: nutrientRangeSchema,
      protein: nutrientRangeSchema,
      fat: nutrientRangeSchema,
      carbs: nutrientRangeSchema,
      nutrients: {
        type: "object",
        properties: customNutrientProperties,
        required: Object.keys(customNutrientProperties),
      },
      package_weight: { type: ["number", "null"] },
    },
    required: ["name", "kcal", "protein", "fat", "carbs", "nutrients", "package_weight"],
  };
}

async function runTextModel(
  ai: Ai,
  name: string,
  customNutrients?: CustomNutrient[],
): Promise<LookupResponse> {
  const prompt = buildLookupPrompt(name, customNutrients);
  const schema = buildNutritionSchema(customNutrients);

  const messages: ChatCompletionMessageParam[] = [
    { role: "system", content: "You are a nutritional database assistant. Respond ONLY with valid JSON." },
    { role: "user", content: prompt },
  ];

  let result: ChatCompletionsOutput;
  try {
    result = await ai.run(TEXT_MODEL, {
      messages,
      response_format: {
        type: "json_schema",
        json_schema: { name: "nutrition", schema, strict: true },
      },
    });
  } catch {
    const schemaDescription = JSON.stringify(schema);
    messages[0] = {
      role: "system",
      content:
        `You are a nutritional database assistant. Respond ONLY with valid JSON matching this schema: ${schemaDescription}`,
    };
    result = await ai.run(TEXT_MODEL, { messages });
  }

  const text = result.choices[0]?.message?.content;
  if (!text) {
    throw new Error("Empty response from text model");
  }
  return JSON.parse(text) as LookupResponse;
}

function convertToKcal(value: number, unit: string): number {
  const normalized = unit.toLowerCase().trim();
  if (normalized === "kj" || normalized === "кДж" || normalized === "кдж") {
    return value * 0.239;
  }
  return value;
}

function convertToGrams(value: number, unit: string): number {
  const normalized = unit.toLowerCase().trim();
  if (normalized === "mg" || normalized === "мг") {
    return value / 1000;
  }
  if (
    normalized === "kg" ||
    normalized === "кг" ||
    normalized === "l" ||
    normalized === "л"
  ) {
    return value * 1000;
  }
  return value;
}

interface VisionNutrientValue {
  value: number;
  unit: string;
}

interface VisionLabelData {
  product_name: string;
  energy: VisionNutrientValue;
  protein: VisionNutrientValue;
  fat: VisionNutrientValue;
  carbs: VisionNutrientValue;
  package_weight: VisionNutrientValue | null;
}

function makeExactRange(value: number, unit: string): NutrientRange {
  const v = { value, unit };
  return { min_value: v, max_value: v, recommended: v, comment: "Extracted from label" };
}

async function handleLookup(request: Request, env: Env): Promise<Response> {
  const body = (await request.json()) as LookupRequest;

  if (!body.name || typeof body.name !== "string") {
    return errorResponse("Missing or invalid 'name' field", 400);
  }

  const result = await runTextModel(env.AI, body.name, body.custom_nutrients);
  return jsonResponse(result);
}

async function handleVision(request: Request, env: Env): Promise<Response> {
  const body = (await request.json()) as VisionRequest;

  if (!body.images || !Array.isArray(body.images) || body.images.length === 0) {
    return errorResponse("Missing or invalid 'images' field", 400);
  }

  const imageContent = body.images.map((img) => ({
    type: "image_url" as const,
    image_url: { url: img.startsWith("data:") ? img : `data:image/jpeg;base64,${img}` },
  }));

  const visionPrompt =
    "Look at the nutrition label(s) in these images. Extract ONLY:\n" +
    "- product_name\n" +
    '- energy: value and unit EXACTLY as on the label ("kcal", "kJ", "кДж", "ккал"). Do NOT convert.\n' +
    '- protein: value and unit ("g", "г")\n' +
    '- fat: value and unit ("g", "г")\n' +
    '- carbs: value and unit ("g", "г")\n' +
    "All nutrition values must be per 100g.\n" +
    "- package_weight: the weight of the EDIBLE product only, with value and unit. " +
    "For products in brine/marinade/syrup/oil, use the DRAINED weight. If not found, return null.\n" +
    "If values on the label are per serving, convert to per 100g.\n" +
    "Do NOT try to estimate any nutrients beyond what is on the label.\n" +
    "Respond ONLY with valid JSON matching this structure:\n" +
    '{"product_name": "...", "energy": {"value": 0, "unit": "kcal"}, ' +
    '"protein": {"value": 0, "unit": "g"}, "fat": {"value": 0, "unit": "g"}, ' +
    '"carbs": {"value": 0, "unit": "g"}, "package_weight": {"value": 0, "unit": "g"} or null}';

  const visionMessages: Ai_Cf_Meta_Llama_3_2_11B_Vision_Instruct_Messages["messages"] = [
    {
      role: "user",
      content: [...imageContent, { type: "text" as const, text: visionPrompt }],
    },
  ];

  const visionResult = await env.AI.run(VISION_MODEL, { messages: visionMessages });

  const visionText = visionResult.response;
  if (!visionText) {
    throw new Error("Empty response from vision model");
  }

  const jsonMatch = visionText.match(/\{[\s\S]*\}/);
  if (!jsonMatch) {
    throw new Error("Vision model did not return valid JSON");
  }
  const labelData = JSON.parse(jsonMatch[0]) as VisionLabelData;

  const kcalValue = convertToKcal(labelData.energy.value, labelData.energy.unit);
  const proteinValue = convertToGrams(labelData.protein.value, labelData.protein.unit);
  const fatValue = convertToGrams(labelData.fat.value, labelData.fat.unit);
  const carbsValue = convertToGrams(labelData.carbs.value, labelData.carbs.unit);

  let packageWeight: number | null = null;
  if (labelData.package_weight) {
    packageWeight = convertToGrams(labelData.package_weight.value, labelData.package_weight.unit);
  }

  let nutrients: Record<string, NutrientRange> = {};
  if (body.custom_nutrients && body.custom_nutrients.length > 0 && labelData.product_name) {
    const customResult = await runTextModel(env.AI, labelData.product_name, body.custom_nutrients);
    nutrients = customResult.nutrients;
  }

  const response: LookupResponse = {
    name: labelData.product_name ?? null,
    kcal: makeExactRange(kcalValue, "kcal"),
    protein: makeExactRange(proteinValue, "g"),
    fat: makeExactRange(fatValue, "g"),
    carbs: makeExactRange(carbsValue, "g"),
    nutrients,
    package_weight: packageWeight,
  };

  return jsonResponse(response);
}

interface ChatCompletionRequest {
  model: string;
  messages: ChatCompletionMessageParam[];
  response_format?: {
    type: string;
    json_schema?: { name: string; schema: Record<string, unknown>; strict: boolean };
  };
  stream?: boolean;
  think?: boolean;
}

function resolveRefs(node: unknown, defs: Record<string, unknown>): unknown {
  if (node === null || typeof node !== "object") return node;
  if (Array.isArray(node)) return node.map((item) => resolveRefs(item, defs));

  const obj = node as Record<string, unknown>;
  if (typeof obj["$ref"] === "string") {
    const refPath = obj["$ref"] as string;
    const defName = refPath.replace("#/$defs/", "").replace("#/definitions/", "");
    const resolved = defs[defName];
    if (resolved) return resolveRefs(resolved, defs);
    return obj;
  }

  const result: Record<string, unknown> = {};
  for (const [key, value] of Object.entries(obj)) {
    if (key === "$defs" || key === "definitions" || key === "$schema" || key === "title") continue;
    result[key] = resolveRefs(value, defs);
  }
  return result;
}

function inlineSchema(schema: Record<string, unknown>): Record<string, unknown> {
  const defs = (schema["$defs"] ?? schema["definitions"] ?? {}) as Record<string, unknown>;
  return resolveRefs(schema, defs) as Record<string, unknown>;
}

async function handleChatCompletions(request: Request, env: Env): Promise<Response> {
  const body = (await request.json()) as ChatCompletionRequest;

  const messages = [...body.messages];
  if (body.response_format?.json_schema?.schema) {
    const schema = inlineSchema(body.response_format.json_schema.schema);
    const schemaJson = JSON.stringify(schema);
    const jsonInstruction =
      `\n\nYou MUST respond with ONLY valid JSON (no markdown, no explanation, no code fences). ` +
      `The JSON MUST conform to this exact schema:\n${schemaJson}`;
    const sysIdx = messages.findIndex((m) => m.role === "system");
    if (sysIdx >= 0) {
      messages[sysIdx] = { ...messages[sysIdx], content: messages[sysIdx].content + jsonInstruction };
    } else {
      messages.unshift({ role: "system", content: `You are a helpful assistant.${jsonInstruction}` });
    }
  }

  const runParams: Record<string, unknown> = { messages, stream: true };
  const aiStream = await env.AI.run(body.model as BaseAiTextGenerationModels, runParams);

  const { readable, writable } = new TransformStream();
  const writer = writable.getWriter();
  const encoder = new TextEncoder();

  (async () => {
    const reader = (aiStream as ReadableStream).getReader();
    const decoder = new TextDecoder();
    let buffer = "";
    try {
      while (true) {
        const { done, value } = await reader.read();
        if (done) break;
        buffer += decoder.decode(value, { stream: true });
        const lines = buffer.split("\n");
        buffer = lines.pop() ?? "";
        for (const line of lines) {
          const trimmed = line.trim();
          if (trimmed === "data: [DONE]") {
            await writer.write(encoder.encode("data: [DONE]\n\n"));
            continue;
          }
          const data = trimmed.startsWith("data: ") ? trimmed.slice(6) : "";
          if (!data) continue;
          try {
            const parsed = JSON.parse(data);
            const token = parsed.response ?? parsed.choices?.[0]?.delta?.content ?? "";
            if (token) {
              const chunk = JSON.stringify({
                choices: [{ index: 0, delta: { content: token }, finish_reason: null }],
              });
              await writer.write(encoder.encode(`data: ${chunk}\n\n`));
            }
          } catch { /* skip malformed lines */ }
        }
      }
      await writer.write(encoder.encode("data: [DONE]\n\n"));
    } finally {
      await writer.close();
    }
  })();

  return new Response(readable, {
    headers: { "Content-Type": "text/event-stream", "Cache-Control": "no-cache", ...CORS_HEADERS },
  });
}

export default {
  async fetch(request: Request, env: Env): Promise<Response> {
    if (request.method === "OPTIONS") {
      return new Response(null, { status: 204, headers: CORS_HEADERS });
    }

    const authHeader = request.headers.get("Authorization") ?? "";
    const token = authHeader.startsWith("Bearer ") ? authHeader.slice(7) : "";
    if (!token || !(await verifyJwt(token, env.JWT_SECRET))) {
      return errorResponse("Unauthorized", 401);
    }

    const url = new URL(request.url);

    if (request.method !== "POST") {
      return errorResponse("Not found", 404);
    }

    switch (url.pathname) {
      case "/food/ai-lookup":
        return handleLookup(request, env);
      case "/food/ai-vision":
        return handleVision(request, env);
      case "/chat/completions":
        return handleChatCompletions(request, env);
      default:
        return errorResponse("Not found", 404);
    }
  },
} satisfies ExportedHandler<Env>;
