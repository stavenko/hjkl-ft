use std::collections::BTreeMap;

use api_types::*;
use arti_pipes::executor::PromptExecutor;
use arti_pipes::llm_executors::qwen::Qwen;
use futures::StreamExt;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use wasm_bindgen::JsCast;
use wasm_bindgen::JsValue;
use wasm_bindgen_futures::JsFuture;

use super::{auth, config, local};

#[derive(Debug, Deserialize, JsonSchema)]
struct NutritionResponse {
    kcal: NutrientDetail,
    protein: NutrientDetail,
    fat: NutrientDetail,
    carbs: NutrientDetail,
    custom_nutrients: BTreeMap<String, NutrientDetail>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct NutrientDetail {
    min_value: ValueUnit,
    max_value: ValueUnit,
    recommended: ValueUnit,
    /// Why this value is appropriate for this food
    comment: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct ValueUnit {
    value: f64,
    /// One of: kcal, kg, g, mg, mkg
    unit: String,
}

impl NutrientDetail {
    fn into_api(self) -> AiNutrientDetail {
        AiNutrientDetail {
            min_value: AiValueWithUnit { value: self.min_value.value, unit: self.min_value.unit },
            max_value: AiValueWithUnit { value: self.max_value.value, unit: self.max_value.unit },
            recommended: AiValueWithUnit { value: self.recommended.value, unit: self.recommended.unit },
            comment: self.comment,
        }
    }
}

fn strip_code_fences(s: &str) -> &str {
    let s = s.trim();
    if let Some(rest) = s.strip_prefix("```") {
        let rest = rest.trim_start_matches(|c: char| c.is_alphanumeric());
        let rest = rest.trim_start_matches('\n');
        rest.strip_suffix("```").unwrap_or(rest).trim()
    } else {
        s
    }
}

fn unwrap_schema_envelope(s: &str) -> &str {
    const PREFIX: &str = r#""properties":"#;
    if let Some(idx) = s.find(PREFIX) {
        let start = idx + PREFIX.len();
        if let Some(obj_start) = s[start..].find('{') {
            let inner_start = start + obj_start;
            let mut depth = 0i32;
            let mut end = inner_start;
            for (i, c) in s[inner_start..].char_indices() {
                match c {
                    '{' => depth += 1,
                    '}' => {
                        depth -= 1;
                        if depth == 0 {
                            end = inner_start + i + 1;
                            break;
                        }
                    }
                    _ => {}
                }
            }
            if depth == 0 {
                return &s[inner_start..end];
            }
        }
    }
    s
}

/// Which stream a token belongs to, reported to the caller as lookup progresses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AiPhase {
    Thinking,
    Answer,
}

fn build_executor() -> Result<Qwen, String> {
    let cfg = config::get();
    let token = auth::get_token().ok_or_else(|| "not authenticated".to_string())?;
    let executor = Qwen::builder()
        .api_base(&cfg.ai_base_url)
        .api_key(token)
        .model("@cf/qwen/qwen3-30b-a3b-fp8")
        // Reasoning is on; emit reasoning tokens to the thinking stream so the
        // caller can show a "thinking" phase.
        .think(true)
        // Workers AI defaults output to 2000 tokens; qwen3's reasoning alone can
        // eat all of it and truncate the answer (empty content → parse error).
        // Lift the ceiling so reasoning + answer always fit. (Needs the ai-worker
        // to forward max_tokens, which it now does.)
        .max_tokens(8000)
        .build();
    Ok(executor)
}

pub async fn lookup(
    input: &AiLookupInput,
    on_token: impl Fn(AiPhase) + Clone + 'static,
) -> Result<AiLookupOutput, String> {
    let executor = build_executor()?;

    let custom_part = if input.custom_nutrients.is_empty() {
        String::new()
    } else {
        let keys: Vec<String> = input
            .custom_nutrients
            .iter()
            .map(|s| format!("\"{}\"", s.key))
            .collect();
        let descriptions: Vec<String> = input
            .custom_nutrients
            .iter()
            .map(|s| format!("{} = {}", s.key, s.name))
            .collect();
        format!(
            "\n\nAlso provide values for these custom nutrients in custom_nutrients map. \
             Use ONLY these strings as keys: {}. \
             Reference: {}.",
            keys.join(", "),
            descriptions.join(", "),
        )
    };

    let prompt = format!(
        "You are a nutritional database. For the food item \"{name}\", provide nutritional \
         values per 100 grams.\n\n\
         For each nutrient (kcal, protein, fat, carbs{custom}), provide:\n\
         - min_value: lowest reasonable value for this food\n\
         - max_value: highest reasonable value for this food\n\
         - recommended: the most likely value to select\n\
         - comment: brief explanation why this value is appropriate\n\n\
         Use these units: kcal for calories, g/mg/mkg/kg for weights.\n\
         All values are per 100g of the product.\n\n\
         Respond with ONLY a single minified JSON object and nothing else — no markdown, no \
         prose before or after. EVERY key and EVERY string value MUST be wrapped in double \
         quotes. EVERY `value` MUST be a real number (e.g. 12.5), never empty or null. Custom \
         nutrients go in the \"custom_nutrients\" object (use {{}} if none). Follow exactly this \
         shape:\n\
         {{\"kcal\":{{\"min_value\":{{\"value\":143,\"unit\":\"kcal\"}},\"max_value\":{{\"value\":160,\"unit\":\"kcal\"}},\"recommended\":{{\"value\":155,\"unit\":\"kcal\"}},\"comment\":\"...\"}},\"protein\":{{...}},\"fat\":{{...}},\"carbs\":{{...}},\"custom_nutrients\":{{}}}}",
        name = input.name,
        custom = custom_part,
    );

    // Use `execute::<T>` so the ai-worker injects the JSON schema as an
    // instruction — that reliably keeps the model in JSON mode (without it the
    // model often replies in Markdown). The earlier "schema corrupts output" and
    // "stochastic garbage" theories were both wrong: the real bug was in the
    // ai-worker's SSE relay dropping numeric tokens (now fixed). The strict
    // example in the prompt above is belt-and-suspenders.
    let result = executor
        .execute::<NutritionResponse>(prompt)
        .await
        .map_err(|e| format!("LLM execute error: {e:?}"))?;

    let mut thinking_stream = result.thinking_stream;
    let on_think = on_token.clone();
    wasm_bindgen_futures::spawn_local(async move {
        while let Some(token) = thinking_stream.next().await {
            if let Ok(t) = token {
                leptos::logging::log!("[think] {}", t.content);
                on_think(AiPhase::Thinking);
            }
        }
    });

    let mut content_stream = result.content_stream;
    let on_answer = on_token.clone();
    wasm_bindgen_futures::spawn_local(async move {
        while let Some(token) = content_stream.next().await {
            if let Ok(t) = token {
                leptos::logging::log!("[content] {}", t.content);
                on_answer(AiPhase::Answer);
            }
        }
    });

    let output = result.output.await.map_err(|e| format!("LLM output error: {e:?}"))?;

    let raw = output.result.trim();
    let json_str = strip_code_fences(raw);

    let response: NutritionResponse = serde_json::from_str(json_str)
        .or_else(|_| serde_json::from_str(unwrap_schema_envelope(json_str)))
        .map_err(|e| format!("parse error: {e}, raw: {raw}"))?;

    let key_to_name: BTreeMap<String, String> = input
        .custom_nutrients
        .iter()
        .map(|s| (s.key.clone(), s.name.clone()))
        .collect();

    let nutrients: BTreeMap<String, AiNutrientDetail> = response
        .custom_nutrients
        .into_iter()
        .filter_map(|(ai_key, v)| {
            let display_name = key_to_name.get(&ai_key)?;
            Some((display_name.clone(), v.into_api()))
        })
        .collect();

    Ok(AiLookupOutput {
        name: None,
        kcal: response.kcal.into_api(),
        protein: response.protein.into_api(),
        fat: response.fat.into_api(),
        carbs: response.carbs.into_api(),
        nutrients,
        package_weight: None,
    })
}

/// Stream a single JSON object of type `T` from the model. Same plumbing as
/// `lookup` (schema-injected JSON mode, thinking/answer token streams reported
/// via `on_token`), but generic over the response shape — used by the day
/// assessment. Reports each streamed token through `on_token` so callers can
/// drive the live "thinking/answer (N tok)" UI.
pub async fn generate<T>(
    prompt: String,
    on_token: impl Fn(AiPhase) + Clone + 'static,
) -> Result<T, String>
where
    T: serde::de::DeserializeOwned + JsonSchema,
{
    // qwen3 occasionally returns an EMPTY content stream (all budget spent on
    // reasoning, or a dropped relay) — that surfaced as a confusing
    // "parse error: EOF ... raw:" and forced the user to retry by hand. Empty /
    // unparseable output is transient, so retry a couple of times before giving
    // up with a clear message.
    const ATTEMPTS: usize = 3;
    let mut last_err = String::new();
    for _ in 0..ATTEMPTS {
        let executor = build_executor()?;
        let result = executor
            .execute::<T>(prompt.clone())
            .await
            .map_err(|e| format!("LLM execute error: {e:?}"))?;

        let mut thinking_stream = result.thinking_stream;
        let on_think = on_token.clone();
        wasm_bindgen_futures::spawn_local(async move {
            while let Some(token) = thinking_stream.next().await {
                if token.is_ok() {
                    on_think(AiPhase::Thinking);
                }
            }
        });

        let mut content_stream = result.content_stream;
        let on_answer = on_token.clone();
        wasm_bindgen_futures::spawn_local(async move {
            while let Some(token) = content_stream.next().await {
                if token.is_ok() {
                    on_answer(AiPhase::Answer);
                }
            }
        });

        let output = result.output.await.map_err(|e| format!("LLM output error: {e:?}"))?;
        let raw = output.result.trim();
        if raw.is_empty() {
            last_err = "model returned an empty response".to_string();
            continue;
        }
        let json_str = strip_code_fences(raw);
        match serde_json::from_str::<T>(json_str)
            .or_else(|_| serde_json::from_str::<T>(unwrap_schema_envelope(json_str)))
        {
            Ok(v) => return Ok(v),
            Err(e) => last_err = format!("parse error: {e}, raw: {raw}"),
        }
    }
    Err(last_err)
}

// ── Support-chat: tools + agentic tool-use loop ──
//
// The chat is a real tool-use loop. The model can either call a registered
// `arti_pipes` tool or end the turn with a final answer. Because the pinned
// Qwen executor is a plain text-completion executor (its `PromptExecutor` only
// exposes `execute_raw`/`execute`, the request carries no `tools` field, and it
// never surfaces `PromptExecutionEvent::ToolCallsRequested`), there is no native
// OpenAI `tool_calls` transport. We bridge it with an explicit text protocol the
// model is instructed to follow, parsed into real `arti_pipes::tool::ToolCall`s
// dispatched through a `ToolRegistry`:
//
//   * To CALL a tool, output on its OWN line, nothing after it:
//         [[tool]] <name> <json-arguments>
//     e.g. `[[tool]] read_progress {"days": 30}`
//   * To FINISH, output the marker then the user-facing answer:
//         [[final]] <answer text…>
//
// `[[final]]` is the explicit, unambiguous end-of-loop signal (see `chat_agent`).
// If a future executor gains native tool-calling, the same registry transfers
// unchanged.

/// Prefix the model emits (line-leading) to request a tool call.
pub const TOOL_PREFIX: &str = "[[tool]]";
/// Prefix the model emits to mark its final answer — the loop's end signal.
pub const FINAL_PREFIX: &str = "[[final]]";

#[derive(Debug, Deserialize, JsonSchema)]
pub struct EscalateInput {
    /// Short human-readable reason the conversation needs a live operator.
    pub reason: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct EscalateOutput {
    pub ack: String,
}

pub struct EscalateToHuman;

#[async_trait::async_trait]
impl arti_pipes::tool::Tool for EscalateToHuman {
    type Input = EscalateInput;
    type Output = EscalateOutput;

    fn name(&self) -> &str {
        "escalate_to_human"
    }

    fn description(&self) -> &str {
        "Escalate the conversation to a real human support operator."
    }

    async fn call(
        &self,
        input: Self::Input,
    ) -> Result<Self::Output, arti_pipes::error::ExecutionError> {
        // STUB. TODO: call the real support worker here (enqueue the conversation
        // for a live operator). For now just acknowledge so the LLM/UI can react.
        Ok(EscalateOutput { ack: format!("Escalation requested: {}", input.reason) })
    }
}

// ── read_progress tool: weight / steps / goal-fulfilment over N days ──

fn default_days() -> u32 {
    30
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ReadProgressInput {
    /// How many past days to summarise (capped to the stored window).
    #[serde(default = "default_days")]
    pub days: u32,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct WeightPoint {
    pub date: String,
    pub kg: f64,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct StepPoint {
    pub date: String,
    pub steps: u32,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct GoalProgress {
    pub nutrient: String,
    pub direction: String,
    pub target: f64,
    pub unit: String,
    pub days_met: u32,
    pub days_logged: u32,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ReadProgressOutput {
    pub period_days: u32,
    pub weight_change_kg: Option<f64>,
    pub weight: Vec<WeightPoint>,
    pub steps_avg: Option<u32>,
    pub steps: Vec<StepPoint>,
    pub goals: Vec<GoalProgress>,
}

/// One logged day's totals, pre-computed in the snapshot so the tool's `call`
/// is pure (no IndexedDB access — its future must stay `Send`).
#[derive(Clone)]
struct DayTotals {
    date: String,
    values: BTreeMap<String, f64>, // "Calories"/"Protein"/"Fat"/"Carbs"/custom-name
}

#[derive(Clone)]
struct GoalSpec {
    nutrient: String,
    direction: GoalDirection,
    amount: f64,
    unit: String,
}

/// Plain, `Send` data snapshot the `read_progress` tool reads from. Built once
/// (off the IndexedDB) BEFORE the chat loop, so the tool itself never awaits a
/// non-`Send` future inside `Tool::call`.
#[derive(Clone)]
pub struct ProgressSnapshot {
    weight: Vec<WeightPoint>,    // ascending by date
    steps: Vec<StepPoint>,       // ascending by date
    days: Vec<DayTotals>,        // ascending by date, only days with entries
    goals: Vec<GoalSpec>,        // current daily goals
}

/// Maximum window we pre-load; the tool slices to the requested `days`.
const SNAPSHOT_WINDOW_DAYS: i64 = 120;

/// Build the progress snapshot off local IndexedDB. Call this in a wasm-local
/// context (its future is NOT `Send`); the resulting `ProgressSnapshot` is.
pub async fn build_progress_snapshot() -> ProgressSnapshot {
    let mut weight: Vec<WeightPoint> = local::list_weight_entries()
        .await
        .into_iter()
        .map(|e| WeightPoint { date: e.date, kg: e.weight_kg })
        .collect();
    weight.sort_by(|a, b| a.date.cmp(&b.date));

    let mut steps: Vec<StepPoint> = local::list_step_entries()
        .await
        .into_iter()
        .map(|e| StepPoint { date: e.date, steps: e.steps })
        .collect();
    steps.sort_by(|a, b| a.date.cmp(&b.date));

    let goals: Vec<GoalSpec> = local::list_goals()
        .await
        .into_iter()
        .filter(|g| g.period == GoalPeriod::Day && g.amount > 0.0)
        .map(|g| GoalSpec {
            nutrient: g.nutrient,
            direction: g.direction,
            amount: g.amount,
            unit: match g.unit {
                GoalUnit::Kcal => "kcal",
                GoalUnit::G => "g",
                GoalUnit::Mg => "mg",
                GoalUnit::Mcg => "mcg",
            }
            .to_string(),
        })
        .collect();

    // Per-day nutrient totals over the window (only days that have diary entries).
    let today = chrono::Local::now().date_naive();
    let dates: Vec<String> = (0..SNAPSHOT_WINDOW_DAYS)
        .map(|i| (today - chrono::Duration::days(i)).format("%Y-%m-%d").to_string())
        .collect();
    let foods = local::list_foods().await;
    let fmap: BTreeMap<String, Food> = foods.into_iter().map(|f| (f.id.clone(), f)).collect();
    let entries = local::list_diary_range(&dates).await;

    let mut per_day: BTreeMap<String, BTreeMap<String, f64>> = BTreeMap::new();
    for e in &entries {
        if let Some(food) = fmap.get(&e.food_id) {
            let factor = (e.grams - e.waste_grams).max(0.0) / 100.0;
            let day = per_day.entry(e.date.clone()).or_default();
            *day.entry("Calories".into()).or_default() += food.effective_kcal() * factor;
            *day.entry("Protein".into()).or_default() += food.protein * factor;
            *day.entry("Fat".into()).or_default() += food.fat * factor;
            *day.entry("Carbs".into()).or_default() += food.carbs * factor;
            for (k, v) in &food.nutrients {
                *day.entry(k.clone()).or_default() += v * factor;
            }
        }
    }
    let mut days: Vec<DayTotals> = per_day
        .into_iter()
        .map(|(date, values)| DayTotals { date, values })
        .collect();
    days.sort_by(|a, b| a.date.cmp(&b.date));

    ProgressSnapshot { weight, steps, days, goals }
}

pub struct ReadProgress {
    snapshot: ProgressSnapshot,
}

#[async_trait::async_trait]
impl arti_pipes::tool::Tool for ReadProgress {
    type Input = ReadProgressInput;
    type Output = ReadProgressOutput;

    fn name(&self) -> &str {
        "read_progress"
    }

    fn description(&self) -> &str {
        "Read the user's weight trend, daily steps, and how well their daily \
         nutrition goals were met over the last N days. Use it before giving \
         progress feedback so you cite real numbers instead of guessing."
    }

    async fn call(
        &self,
        input: Self::Input,
    ) -> Result<Self::Output, arti_pipes::error::ExecutionError> {
        // Pure: slice the pre-fetched snapshot to the requested window. No await
        // of a non-Send future here.
        let days = input.days.max(1);
        let cutoff = chrono::Local::now().date_naive() - chrono::Duration::days(days as i64 - 1);
        let cutoff = cutoff.format("%Y-%m-%d").to_string();

        let weight: Vec<WeightPoint> = self
            .snapshot
            .weight
            .iter()
            .filter(|p| p.date >= cutoff)
            .map(|p| WeightPoint { date: p.date.clone(), kg: p.kg })
            .collect();
        let weight_change_kg = match (weight.first(), weight.last()) {
            (Some(a), Some(b)) if a.date != b.date => Some(b.kg - a.kg),
            _ => None,
        };

        let steps: Vec<StepPoint> = self
            .snapshot
            .steps
            .iter()
            .filter(|p| p.date >= cutoff)
            .map(|p| StepPoint { date: p.date.clone(), steps: p.steps })
            .collect();
        let steps_avg = if steps.is_empty() {
            None
        } else {
            Some((steps.iter().map(|s| s.steps as u64).sum::<u64>() / steps.len() as u64) as u32)
        };

        let in_window: Vec<&DayTotals> =
            self.snapshot.days.iter().filter(|d| d.date >= cutoff).collect();
        let goals: Vec<GoalProgress> = self
            .snapshot
            .goals
            .iter()
            .map(|g| {
                let mut days_met = 0u32;
                let mut days_logged = 0u32;
                for d in &in_window {
                    let val = d.values.get(&g.nutrient).copied().unwrap_or(0.0);
                    // A day "counts" toward a goal only if anything was logged.
                    if d.values.values().any(|v| *v > 0.0) {
                        days_logged += 1;
                        let met = match g.direction {
                            GoalDirection::AtLeast => val >= g.amount,
                            GoalDirection::AtMost => val <= g.amount,
                        };
                        if met {
                            days_met += 1;
                        }
                    }
                }
                GoalProgress {
                    nutrient: g.nutrient.clone(),
                    direction: match g.direction {
                        GoalDirection::AtLeast => "at_least".into(),
                        GoalDirection::AtMost => "at_most".into(),
                    },
                    target: g.amount,
                    unit: g.unit.clone(),
                    days_met,
                    days_logged,
                }
            })
            .collect();

        Ok(ReadProgressOutput {
            period_days: days,
            weight_change_kg,
            weight,
            steps_avg,
            steps,
            goals,
        })
    }
}

/// The tool registry exposed to support chat: `escalate_to_human` (stub) and
/// `read_progress` (reads the pre-built snapshot). Descriptors come from these
/// real registrations, and calls are dispatched via `ToolRegistry::execute`.
pub fn chat_registry(snapshot: ProgressSnapshot) -> arti_pipes::tool_registry::ToolRegistry {
    arti_pipes::tool_registry::ToolRegistry::new()
        .register(EscalateToHuman)
        .register(ReadProgress { snapshot })
}

/// A one-line, model-facing description of the registered tools, derived from
/// the actual registry descriptors (name + description + input schema). The
/// prompt embeds this so the advertised tool is the registered one, not a
/// hand-written copy that could drift.
pub fn tool_descriptions(registry: &arti_pipes::tool_registry::ToolRegistry) -> String {
    registry
        .descriptors()
        .into_iter()
        .map(|d| {
            let schema = serde_json::to_string(&d.input_schema)
                .unwrap_or_else(|_| "{}".to_string());
            format!("- {} — {} (input JSON schema: {})", d.name, d.description, schema)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Roles in the running chat transcript fed back into each turn's prompt.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ChatRole {
    User,
    Assistant,
    Tool,
}

/// One rendered conversation turn. `Tool` turns carry a tool's JSON result so
/// the next LLM turn can read what the tool returned.
#[derive(Clone)]
pub struct ChatTurn {
    pub role: ChatRole,
    pub text: String,
}

/// Live events emitted during the agentic loop, for the UI.
pub enum ChatEvent<'a> {
    /// A new LLM turn started (waiting on the first token).
    Requesting,
    /// A reasoning token arrived (count it).
    Thinking,
    /// A visible answer chunk (already stripped of control markers).
    Answer(&'a str),
    /// A tool is about to run.
    ToolCall(&'a str),
    /// A tool finished.
    ToolDone(&'a str),
}

/// Outcome of the chat loop: the final user-facing answer and whether the
/// `escalate_to_human` tool fired during it.
pub struct ChatOutcome {
    pub answer: String,
    pub escalated: bool,
}

/// Hard cap on LLM↔tool round-trips, so a misbehaving model cannot loop forever.
const MAX_TOOL_ITERS: usize = 6;
/// Only the most recent N turns (user + assistant + tool) are sent to the model.
const CONTEXT_WINDOW: usize = 20;

/// Run the support-chat agentic loop. Each iteration renders the last
/// `CONTEXT_WINDOW` turns into a prompt, streams the model's output (filtered of
/// control markers), then either:
///   * dispatches a `[[tool]]` call through the registry and feeds the JSON
///     result back as a `Tool` turn (loop continues), or
///   * stops on the explicit `[[final]]` marker — the end-of-loop signal — or on
///     a turn with no tool call, treated as a final answer.
/// `MAX_TOOL_ITERS` is the safety backstop against infinite looping.
///
/// `system` is the fixed preamble (assistant role, language, tool protocol).
/// FAIL LOUDLY: executor and tool-dispatch errors propagate as `Err`.
///
/// TODO: support chat inherits the ai-worker subscription 402 gate (surfaced as
/// `Err("HTTP 402: …")`); it may later need an ungated path.
pub async fn chat_agent(
    system: String,
    mut transcript: Vec<ChatTurn>,
    snapshot: ProgressSnapshot,
    on_event: impl Fn(ChatEvent) + Clone + 'static,
) -> Result<ChatOutcome, String> {
    let registry = chat_registry(snapshot);
    let mut escalated = false;

    for _ in 0..MAX_TOOL_ITERS {
        on_event(ChatEvent::Requesting);

        let prompt = render_prompt(&system, &transcript);
        let executor = build_executor()?;
        let result = executor
            .execute_raw(prompt)
            .await
            .map_err(|e| format!("LLM execute error: {e:?}"))?;

        // Thinking stream: count only.
        let mut thinking_stream = result.thinking_stream;
        let on_think = on_event.clone();
        wasm_bindgen_futures::spawn_local(async move {
            while let Some(token) = thinking_stream.next().await {
                if token.is_ok() {
                    on_think(ChatEvent::Thinking);
                }
            }
        });

        // Answer stream: filter control markers so only visible answer text
        // reaches the UI. Authoritative parsing happens on the final `raw` below.
        let mut content_stream = result.content_stream;
        let on_answer = on_event.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let mut filter = ControlFilter::default();
            while let Some(token) = content_stream.next().await {
                if let Ok(t) = token {
                    let visible = filter.push(&t.content);
                    if !visible.is_empty() {
                        on_answer(ChatEvent::Answer(&visible));
                    }
                }
            }
        });

        let output = result.output.await.map_err(|e| format!("LLM output error: {e:?}"))?;
        let raw = output.result.trim().to_string();

        // 1) Explicit tool call? Dispatch through the registry and loop.
        if let Some(call) = parse_tool_call(&raw) {
            let name = call.name.clone();
            on_event(ChatEvent::ToolCall(&name));
            if name == "escalate_to_human" {
                escalated = true;
            }
            let res = registry
                .execute(&call)
                .await
                .map_err(|e| format!("tool {name} failed: {e:?}"))?;
            let result_json =
                serde_json::to_string(&res.output).unwrap_or_else(|_| "{}".to_string());
            // Record the model's request + the tool's result so the next turn can
            // read them.
            transcript.push(ChatTurn { role: ChatRole::Assistant, text: raw });
            transcript.push(ChatTurn {
                role: ChatRole::Tool,
                text: format!("{name} -> {result_json}"),
            });
            on_event(ChatEvent::ToolDone(&name));
            continue;
        }

        // 2) Explicit `[[final]]` marker (clean end signal) or no marker at all
        //    (graceful fallback) → this is the answer. End the loop.
        let answer = strip_final_marker(&raw);
        return Ok(ChatOutcome { answer, escalated });
    }

    // Hit the iteration cap with the model still asking for tools.
    Err(format!("chat loop did not finish within {MAX_TOOL_ITERS} tool steps"))
}

/// Render the system preamble + the last `CONTEXT_WINDOW` turns into a single
/// prompt string ending with `Assistant:` for the next completion.
fn render_prompt(system: &str, transcript: &[ChatTurn]) -> String {
    let start = transcript.len().saturating_sub(CONTEXT_WINDOW);
    let mut p = String::from(system);
    p.push_str("\n\nConversation:\n");
    for turn in &transcript[start..] {
        let speaker = match turn.role {
            ChatRole::User => "User",
            ChatRole::Assistant => "Assistant",
            ChatRole::Tool => "Tool result",
        };
        p.push_str(&format!("{speaker}: {}\n", turn.text));
    }
    p.push_str("Assistant:");
    p
}

/// Parse a `[[tool]] <name> <json>` directive anywhere in the model output into
/// a real `ToolCall`. Returns None if there is no tool directive.
fn parse_tool_call(raw: &str) -> Option<arti_pipes::tool::ToolCall> {
    let idx = raw.find(TOOL_PREFIX)?;
    let after = raw[idx + TOOL_PREFIX.len()..].trim_start();
    let line_end = after.find('\n').unwrap_or(after.len());
    let line = after[..line_end].trim();
    // `<name> <json>` — name is the first whitespace-delimited token.
    let (name, args_str) = match line.split_once(char::is_whitespace) {
        Some((n, rest)) => (n.trim(), rest.trim()),
        None => (line, ""),
    };
    if name.is_empty() {
        return None;
    }
    let arguments = if args_str.is_empty() {
        serde_json::json!({})
    } else {
        serde_json::from_str(args_str).unwrap_or_else(|_| serde_json::json!({}))
    };
    Some(arti_pipes::tool::ToolCall {
        id: uuid::Uuid::now_v7().to_string(),
        name: name.to_string(),
        arguments,
    })
}

/// Strip a leading `[[final]]` marker (and following whitespace) if present;
/// otherwise return the text as-is.
fn strip_final_marker(raw: &str) -> String {
    match raw.find(FINAL_PREFIX) {
        Some(idx) => {
            let before = raw[..idx].trim_end();
            let after = raw[idx + FINAL_PREFIX.len()..].trim_start();
            if before.is_empty() {
                after.to_string()
            } else {
                format!("{before}\n{after}")
            }
        }
        None => raw.to_string(),
    }
}

/// Incremental display filter for the streamed answer. It hides control
/// directives so the user never sees the protocol mid-stream:
///   * a turn whose (trimmed) start is `[[tool]]` is suppressed whole;
///   * a leading `[[final]]` marker is stripped, the rest shown live;
///   * any other turn is shown live (plain final answer).
/// Until it can classify (the markers share the `[[` prefix), it buffers.
#[derive(Default)]
struct ControlFilter {
    buffer: String,
    decided: Option<FilterMode>,
}

#[derive(Clone, Copy)]
enum FilterMode {
    /// Show everything from here on.
    Pass,
    /// Suppress everything (a tool-call turn).
    Suppress,
}

impl ControlFilter {
    fn push(&mut self, chunk: &str) -> String {
        match self.decided {
            Some(FilterMode::Pass) => chunk.to_string(),
            Some(FilterMode::Suppress) => String::new(),
            None => {
                self.buffer.push_str(chunk);
                self.classify()
            }
        }
    }

    /// Try to classify the turn from the buffer so far, emitting any text that is
    /// safe to show once decided.
    fn classify(&mut self) -> String {
        let trimmed = self.buffer.trim_start();

        if trimmed.starts_with(TOOL_PREFIX) {
            self.decided = Some(FilterMode::Suppress);
            self.buffer.clear();
            return String::new();
        }
        if let Some(rest) = trimmed.strip_prefix(FINAL_PREFIX) {
            self.decided = Some(FilterMode::Pass);
            let out = rest.trim_start().to_string();
            self.buffer.clear();
            return out;
        }
        // Still possibly the start of a marker? Keep buffering.
        if TOOL_PREFIX.starts_with(trimmed) || FINAL_PREFIX.starts_with(trimmed) {
            return String::new();
        }
        // Definitely neither marker — plain answer. Flush the buffer.
        self.decided = Some(FilterMode::Pass);
        std::mem::take(&mut self.buffer)
    }
}

fn exact_range(value: f64, unit: &str) -> AiNutrientDetail {
    let v = AiValueWithUnit { value, unit: unit.to_string() };
    AiNutrientDetail {
        min_value: v.clone(),
        max_value: v.clone(),
        recommended: v,
        comment: "Extracted from label".to_string(),
    }
}

/// One job's status as returned by the ocr-queue worker.
#[derive(Deserialize)]
struct QueueJob {
    status: String,
    #[serde(default)]
    position: u32,
    #[serde(default)]
    created_at: f64,
    #[serde(default)]
    started_at: Option<f64>,
    #[serde(default)]
    phase: Option<String>,
    #[serde(default)]
    thinking_tokens: u32,
    #[serde(default)]
    answer_tokens: u32,
    #[serde(default)]
    result: Option<QueueResult>,
    #[serde(default)]
    error: Option<String>,
}

/// Queue state from the status poller. `since_ms` is the epoch-ms start of the
/// current phase (queued-at / processing-at) for the seconds counter.
pub enum QueuePhase {
    Queued { position: u32, since_ms: f64 },
    Processing { since_ms: f64 },
    Done(AiLookupOutput),
    Error(String),
}

/// The per-100g recognition result the on-prem poller posts back.
#[derive(Deserialize)]
struct QueueResult {
    #[serde(default)]
    product_name: Option<String>,
    #[serde(default)]
    energy_kcal: Option<f64>,
    #[serde(default)]
    protein_g: Option<f64>,
    #[serde(default)]
    fat_g: Option<f64>,
    #[serde(default)]
    carbs_g: Option<f64>,
    #[serde(default)]
    package_weight_g: Option<f64>,
    #[serde(default)]
    custom_nutrients: BTreeMap<String, serde_json::Value>,
}

/// Await `ms` milliseconds via setTimeout (no extra crate needed).
pub async fn sleep_ms(ms: i32) {
    let promise = js_sys::Promise::new(&mut |resolve, _reject| {
        let _ = web_sys::window()
            .expect("no window")
            .set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, ms);
    });
    let _ = JsFuture::from(promise).await;
}

/// Authenticated request to the ocr-queue worker; returns (status, body text).
async fn queue_request(
    method: &str,
    path: &str,
    body: Option<&serde_json::Value>,
) -> Result<(u16, String), String> {
    let base = &config::get().ocr_queue_base_url;
    let url = format!("{base}{path}");
    let token = auth::get_token().ok_or_else(|| "not authenticated".to_string())?;

    let opts = web_sys::RequestInit::new();
    opts.set_method(method);
    if let Some(b) = body {
        let s = serde_json::to_string(b).map_err(|e| e.to_string())?;
        opts.set_body(&JsValue::from_str(&s));
    }
    let headers = web_sys::Headers::new().map_err(|e| format!("{e:?}"))?;
    headers.set("Content-Type", "application/json").map_err(|e| format!("{e:?}"))?;
    headers.set("Authorization", &format!("Bearer {token}")).map_err(|e| format!("{e:?}"))?;
    opts.set_headers(&headers);

    let request = web_sys::Request::new_with_str_and_init(&url, &opts).map_err(|e| format!("{e:?}"))?;
    let window = web_sys::window().expect("no window");
    let resp_val = JsFuture::from(window.fetch_with_request(&request)).await.map_err(|e| format!("{e:?}"))?;
    let resp: web_sys::Response = resp_val.dyn_into().map_err(|_| "not a Response".to_string())?;
    let text = JsFuture::from(resp.text().map_err(|e| format!("{e:?}"))?).await.map_err(|e| format!("{e:?}"))?;
    let text = text.as_string().ok_or("response not string")?;
    Ok((resp.status(), text))
}

/// Short non-streaming text completion via ai-worker (qwen3, thinking off).
/// Used for daily/weekly summaries. Subscription-gated by the worker (402 → Err).
pub async fn summarize(prompt: &str) -> Result<String, String> {
    let base = &config::get().ai_base_url;
    let url = format!("{base}/chat/completions");
    let token = auth::get_token().ok_or_else(|| "not authenticated".to_string())?;
    let body = serde_json::json!({
        "model": "@cf/qwen/qwen3-30b-a3b-fp8",
        "stream": false,
        "chat_template_kwargs": { "enable_thinking": false },
        "messages": [{ "role": "user", "content": prompt }],
    });

    let opts = web_sys::RequestInit::new();
    opts.set_method("POST");
    opts.set_body(&JsValue::from_str(&serde_json::to_string(&body).map_err(|e| e.to_string())?));
    let headers = web_sys::Headers::new().map_err(|e| format!("{e:?}"))?;
    headers.set("Content-Type", "application/json").map_err(|e| format!("{e:?}"))?;
    headers.set("Authorization", &format!("Bearer {token}")).map_err(|e| format!("{e:?}"))?;
    opts.set_headers(&headers);
    let request = web_sys::Request::new_with_str_and_init(&url, &opts).map_err(|e| format!("{e:?}"))?;
    let window = web_sys::window().expect("no window");
    let resp_val = JsFuture::from(window.fetch_with_request(&request)).await.map_err(|e| format!("{e:?}"))?;
    let resp: web_sys::Response = resp_val.dyn_into().map_err(|_| "not a Response".to_string())?;
    let text = JsFuture::from(resp.text().map_err(|e| format!("{e:?}"))?).await.map_err(|e| format!("{e:?}"))?;
    let text = text.as_string().ok_or("response not string")?;
    if !resp.ok() {
        return Err(format!("HTTP {}: {}", resp.status(), text));
    }
    // The worker passes the raw Workers AI response through untouched, so parse
    // it here. Shape varies: OpenAI `choices[].message.content` (or `reasoning`),
    // or Workers AI's native top-level `response`.
    let v: serde_json::Value = serde_json::from_str(&text).map_err(|e| format!("parse: {e}"))?;
    let msg = v.get("choices").and_then(|c| c.get(0)).and_then(|c| c.get("message"));
    let out = msg
        .and_then(|m| m.get("content").and_then(|s| s.as_str()))
        .or_else(|| msg.and_then(|m| m.get("reasoning").and_then(|s| s.as_str())))
        .or_else(|| v.get("response").and_then(|r| r.as_str()))
        .unwrap_or("");
    Ok(out.trim().to_string())
}

/// Submit label image(s) to the ocr-queue; returns the job id immediately.
/// The job is then processed asynchronously on-prem — poll it via `poll_vision`.
pub async fn submit_vision(input: &AiVisionInput) -> Result<String, String> {
    let submit_body = serde_json::json!({
        "images": input.images,
        "custom_nutrients": input.custom_nutrients,
    });
    let (status, text) = queue_request("POST", "/submit", Some(&submit_body)).await?;
    if status == 402 {
        return Err("HTTP 402: subscription_required".to_string());
    }
    if status != 200 {
        return Err(format!("submit HTTP {status}: {text}"));
    }
    serde_json::from_str::<serde_json::Value>(&text)
        .ok()
        .and_then(|v| v.get("job_id").and_then(|j| j.as_str()).map(String::from))
        .ok_or_else(|| format!("no job_id in response: {text}"))
}

/// THE POLLER. One queue-status poll. Used while the job is `queued` (to show
/// the position) and to detect the transition to `processing` / `done`.
/// Transient network/5xx errors report as still-`Queued` so we keep waiting.
pub async fn poll_queue(job_id: &str, input: &AiVisionInput) -> Result<QueuePhase, String> {
    let (st, body) = queue_request("GET", &format!("/job/{job_id}"), None).await?;
    if st != 200 {
        return Ok(QueuePhase::Queued { position: 0, since_ms: 0.0 });
    }
    let job: QueueJob = serde_json::from_str(&body).map_err(|e| format!("job parse: {e}, raw: {body}"))?;
    Ok(match job.status.as_str() {
        "done" => QueuePhase::Done(map_result(input, job.result.ok_or_else(|| "job done but no result".to_string())?)),
        "error" => QueuePhase::Error(job.error.unwrap_or_else(|| "recognition failed".to_string())),
        "processing" => QueuePhase::Processing { since_ms: job.started_at.unwrap_or(job.created_at) },
        _ => QueuePhase::Queued { position: job.position, since_ms: job.created_at },
    })
}

/// THE STREAMING. Used while the job is `processing`: opens the worker's SSE
/// stream and invokes `on_progress(llm_phase, thinking_tokens, answer_tokens)`
/// live (phase: 0=none, 1=thinking, 2=answer). Returns the final result.
pub async fn stream_vision(
    job_id: &str,
    input: &AiVisionInput,
    on_progress: impl Fn(u8, u32, u32),
) -> Result<AiLookupOutput, String> {
    let base = &config::get().ocr_queue_base_url;
    let url = format!("{base}/stream/{job_id}");
    let token = auth::get_token().ok_or_else(|| "not authenticated".to_string())?;

    let opts = web_sys::RequestInit::new();
    opts.set_method("GET");
    let headers = web_sys::Headers::new().map_err(|e| format!("{e:?}"))?;
    headers.set("Authorization", &format!("Bearer {token}")).map_err(|e| format!("{e:?}"))?;
    opts.set_headers(&headers);
    let request = web_sys::Request::new_with_str_and_init(&url, &opts).map_err(|e| format!("{e:?}"))?;
    let window = web_sys::window().expect("no window");
    let resp_val = JsFuture::from(window.fetch_with_request(&request)).await.map_err(|e| format!("{e:?}"))?;
    let resp: web_sys::Response = resp_val.dyn_into().map_err(|_| "not a Response".to_string())?;
    if !resp.ok() {
        return Err(format!("stream HTTP {}", resp.status()));
    }
    let body = resp.body().ok_or_else(|| "no stream body".to_string())?;
    let reader: web_sys::ReadableStreamDefaultReader =
        body.get_reader().dyn_into().map_err(|_| "no stream reader".to_string())?;

    let mut buf = String::new();
    loop {
        let chunk = JsFuture::from(reader.read()).await.map_err(|e| format!("stream read: {e:?}"))?;
        let done = js_sys::Reflect::get(&chunk, &JsValue::from_str("done"))
            .ok().and_then(|d| d.as_bool()).unwrap_or(false);
        if done {
            break;
        }
        let value = js_sys::Reflect::get(&chunk, &JsValue::from_str("value")).map_err(|e| format!("{e:?}"))?;
        let bytes = js_sys::Uint8Array::new(&value).to_vec();
        buf.push_str(&String::from_utf8_lossy(&bytes));

        while let Some(idx) = buf.find("\n\n") {
            let event = buf[..idx].to_string();
            buf.replace_range(..idx + 2, "");
            for line in event.lines() {
                let Some(data) = line.strip_prefix("data: ") else { continue };
                let Ok(v) = serde_json::from_str::<serde_json::Value>(data) else { continue };
                match v.get("type").and_then(|t| t.as_str()) {
                    Some("progress") => {
                        let phase = match v.get("phase").and_then(|p| p.as_str()) {
                            Some("thinking") => 1u8,
                            Some("answer") => 2u8,
                            _ => 0u8,
                        };
                        let tt = v.get("thinking_tokens").and_then(|x| x.as_u64()).unwrap_or(0) as u32;
                        let at = v.get("answer_tokens").and_then(|x| x.as_u64()).unwrap_or(0) as u32;
                        on_progress(phase, tt, at);
                    }
                    Some("done") => {
                        let rv = v.get("result").cloned().unwrap_or(serde_json::Value::Null);
                        let r: QueueResult = serde_json::from_value(rv).map_err(|e| format!("result parse: {e}"))?;
                        return Ok(map_result(input, r));
                    }
                    Some("error") => {
                        return Err(v.get("error").and_then(|e| e.as_str()).unwrap_or("recognition failed").to_string());
                    }
                    _ => {}
                }
            }
        }
    }
    Err("stream ended without result".to_string())
}

fn map_result(input: &AiVisionInput, r: QueueResult) -> AiLookupOutput {
    // Custom nutrients come back keyed by the requested key; remap to the
    // display name and wrap each scalar as an exact range.
    let mut nutrients = BTreeMap::new();
    for spec in &input.custom_nutrients {
        if let Some(val) = r.custom_nutrients.get(&spec.key).and_then(|v| v.as_f64()) {
            nutrients.insert(spec.name.clone(), exact_range(val, &spec.unit_label));
        }
    }
    AiLookupOutput {
        name: r.product_name,
        kcal: exact_range(r.energy_kcal.unwrap_or(0.0), "kcal"),
        protein: exact_range(r.protein_g.unwrap_or(0.0), "g"),
        fat: exact_range(r.fat_g.unwrap_or(0.0), "g"),
        carbs: exact_range(r.carbs_g.unwrap_or(0.0), "g"),
        nutrients,
        package_weight: r.package_weight_g,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn print_schema() {
        let schema = schemars::schema_for!(NutritionResponse);
        println!("{}", serde_json::to_string_pretty(&schema).unwrap());
    }

    #[test]
    fn parses_tool_call() {
        let c = parse_tool_call("[[tool]] read_progress {\"days\": 30}").unwrap();
        assert_eq!(c.name, "read_progress");
        assert_eq!(c.arguments, serde_json::json!({ "days": 30 }));
    }

    #[test]
    fn tool_call_without_args_defaults_empty_object() {
        let c = parse_tool_call("[[tool]] escalate_to_human").unwrap();
        assert_eq!(c.name, "escalate_to_human");
        assert_eq!(c.arguments, serde_json::json!({}));
    }

    #[test]
    fn no_tool_call_in_plain_answer() {
        assert!(parse_tool_call("[[final]] You are doing great.").is_none());
        assert!(parse_tool_call("Just a plain answer.").is_none());
    }

    #[test]
    fn strips_final_marker() {
        assert_eq!(strip_final_marker("[[final]] hello"), "hello");
        assert_eq!(strip_final_marker("plain, no marker"), "plain, no marker");
    }

    /// Feed a marker split across arbitrary chunk boundaries; the marker itself
    /// must never leak, only the answer after it.
    #[test]
    fn control_filter_hides_final_marker_across_chunks() {
        let mut f = ControlFilter::default();
        let mut out = String::new();
        for chunk in ["[", "[fin", "al]] He", "llo!"] {
            out.push_str(&f.push(chunk));
        }
        assert_eq!(out, "Hello!");
    }

    /// A whole tool-call turn must be fully suppressed from the visible stream.
    #[test]
    fn control_filter_suppresses_tool_turn() {
        let mut f = ControlFilter::default();
        let mut out = String::new();
        for chunk in ["[[tool]] read_pro", "gress {\"days\": 7}"] {
            out.push_str(&f.push(chunk));
        }
        assert_eq!(out, "");
    }

    /// A plain answer (no markers) streams through unchanged.
    #[test]
    fn control_filter_passes_plain_answer() {
        let mut f = ControlFilter::default();
        let mut out = String::new();
        for chunk in ["Your weight ", "is trending down."] {
            out.push_str(&f.push(chunk));
        }
        assert_eq!(out, "Your weight is trending down.");
    }
}
