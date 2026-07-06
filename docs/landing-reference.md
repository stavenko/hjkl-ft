# Renorma — Landing reference & spec

## 1. Reference we liked — Физикл Клуб (`lp.fizikl.org/club/tg`)

**Design**
- Font: **Mulish** (sans-serif); large headings, weight 500–800.
- Palette: white `#fff` base; near-black navy accent/buttons **`#000729`**; light-grey
  section cards `#F4F5F5` + translucent grey; black text.
- Two-tone headings (dark bold word + muted-grey rest).
- Dark pill buttons, white text, radius 16px; the CTA repeats down the page (→ `#form`).
- Airy, centered column, big rounded section cards, phone mockups with floating UI
  cards (food photo → auto count, KBJU progress ring, chat, video). Logo = outline
  square icon + wordmark. Very long page (~12 000 px).

**Section order (their flow):** sticky header → hero → "для кого" pain quotes → bridge
line → product/features → CTA → "нет куратора" → content library (accordion) → first-month
content (accordion) → community → authority numbers → **founder** → **results (before/after)**
→ pricing → signup form → FAQ → footer. Pattern: *pain → tool → content → community →
proof → price → form → FAQ*, with a repeating CTA.

## 2. Renorma — differences from the reference

- **Positioning:** a **fully autonomous, convenient tool for weight loss & weight control**
  (no curator, no coaching — the instrument does the thinking). This is the core message.
- **Drop:** founder section, community section, before/after results (not published yet).
- **Social proof = science, not people:** scientific data, the math/algorithms behind the
  norms, citations — instead of testimonials/founder authority.
- **Numbers:** "5 years" (not 9); keep an honest, science-flavoured stats block (e.g. years,
  nutrients tracked, parameters considered) — no invented case counts.

### Renorma section order (proposed)
1. **Sticky header** — logo «Renorma» + nav (Как это работает / Что внутри / Почему работает) + CTA.
2. **Hero** — two-tone H1 (positioning), subtext, CTA, phone mockups of the app.
3. **«Для кого»** — pain points (keep as short problem statements; can stay quote-style but
   anonymised, since we have no testimonials).
4. **Product / "Что внутри"** — the autonomous tool: КБЖУ + микронутриенты, AI-распознавание
   еды, индивидуальная норма калорий/белка с еженедельным пересчётом, дневник веса/шагов,
   ежедневная оценка рациона, история-онбординг.
5. **«Почему это работает» = наука + математика** (replaces community/founder/proof):
   - the algorithm: how the first calorie planka is computed (avg intake + weight-trend
     balance), weekly recalculation, energy-balance model;
   - cited science (dose-response steps meta-analysis we already use, protein 1.2 g/kg, etc.);
   - "точность мирового уровня" framed as math, not coaching.
6. **Numbers band** — 5 лет, N+ нутриентов/параметров, 100% автономно (no case counts).
7. **Pricing** — subscription (paywall code/flow already exists in-app).
8. **Signup / CTA → app**.
9. **FAQ** — adapted ("это очередное приложение с калориями?", "справлюсь без куратора?",
   "чем лучше ChatGPT?", "сколько времени занимает?", "нужны ли тренировки?", refund).
10. **Footer** — оферта, политика, контакты.

## 3. Renorma color system (60 / 30 / 10 + complementary)

Different from Fizikl's navy, but same discipline. Health/trust + a calm tech accent.

| Role | Share | Color | Hex | Use |
|------|-------|-------|-----|-----|
| **Base / neutral** | ~60% | Off-white bg + white surfaces; muted grey text | `#F7F9FB` bg · `#FFFFFF` cards · `#5B6675` muted | page background, section cards, secondary text |
| **Secondary / brand-dark** | ~30% | Deep slate-ink | **`#0E1B2A`** | body headings, dark sections, **primary buttons** (white text), strong text |
| **Accent** | ~10% | Teal (calm, medical-tech, trustworthy) | **`#14B8A6`** | links, badges, two-tone heading word, icons, progress, hover states |
| **Complementary spark** | <5% | Warm coral (teal's complement) | `#FB7156` | the big stat numbers / one highlight per screen — used sparingly |

**Rationale / best practice**
- 60-30-10: a neutral canvas dominates, the slate-ink carries structure, teal is the single
  identity accent; coral appears rarely for contrast pop (it's the hue-wheel complement of teal,
  so it harmonises while standing out).
- **Contrast / a11y:** keep primary CTAs **slate-ink `#0E1B2A` + white text** (like the
  reference — strong, accessible). Teal is for accents on light backgrounds, NOT as a
  white-text button fill (teal `#14B8A6` on white text fails WCAG); if a teal button is needed
  use teal-700 `#0F766E`. Teal `#14B8A6` text on white is ~2.9:1 → only for large/bold or
  decorative, not body copy.
- Two-tone heading: dark `#0E1B2A` + the emphasised word in teal `#14B8A6` (or muted
  `#5B6675` for a softer take).
- Semantic states reuse the app's existing Bulma vars: success green, warning amber.

**Typography:** keep **Mulish** (matches the reference feel and is clean/geometric), or
**Inter** as an alternative. Headings 600–800, body 400–500.

**Mapping to the app theme (Bulma):** if we want the app and landing to share identity, set
`--bulma-link` ≈ teal `#14B8A6`/`#0F766E`, dark UI ≈ `#0E1B2A`. (Decide later; the in-app
theme is currently Bulma defaults.)

## 4. Open questions before building
- Where does the landing live — a new static page in this repo, a separate Pages project, or
  `renorma.app` root with the app under a path?
- Sign-up flow: lead form (like Fizikl) vs. straight "Open the app" CTA (PWA install)?
- Final accent hue confirmation (teal proposed; green `#22C55E` or indigo `#6366F1` are
  alternatives if a less "medical" feel is wanted).
