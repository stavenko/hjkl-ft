//! Declarative Story DSL: structs + a pure engine.
//!
//! The story is authored in `story/story.yaml`, converted to JSON by `build.rs`,
//! embedded here, and parsed once into a `&'static Story`. The engine evaluates
//! chapter-open / section-complete / task-active state against an
//! [`EngineSnapshot`] (the `Progress` sensor backend + the persisted flag sets),
//! so all logic is pure and unit-testable. The snapshot is built in
//! `story::engine_snapshot()` off IndexedDB.

use std::collections::HashSet;
use std::sync::OnceLock;

use serde::Deserialize;

use crate::services::story::Progress;

// ── DSL structs ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
pub struct Loc {
    pub en: String,
    pub ru: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Story {
    #[serde(default)]
    pub tasks: Vec<Task>,
    pub chapters: Vec<Chapter>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Task {
    pub id: String,
    pub title: Loc,
    /// When the task becomes active (within its open chapter). Default: always.
    #[serde(default)]
    pub enable: Cond,
    /// When the task is considered done.
    pub close: Cond,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Chapter {
    pub id: String,
    pub title: Loc,
    pub open: Cond,
    pub sections: Vec<Section>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Section {
    pub id: String,
    pub title: Loc,
    #[serde(default)]
    pub tasks: Vec<String>,
    /// Named Rust actions run when the section page is opened.
    #[serde(default)]
    pub on_open: Vec<String>,
    /// Content blocks (sex/lang-filtered prose + named widgets).
    #[serde(default)]
    pub blocks: Vec<Block>,
    /// What makes this section "complete" (unlocking the next). Defaults to
    /// "all of `tasks` closed" (or `always` when there are no tasks).
    #[serde(default)]
    pub complete: Option<Cond>,
    /// While migrating: link the hub to the existing bespoke page instead of the
    /// generic renderer. Removed once the section's prose lives in `blocks`.
    #[serde(default)]
    pub legacy_route: Option<String>,
}

/// One content block. Exactly one of the content fields is set; `sex` optionally
/// restricts the block to one biological sex. Authors order blocks freely.
#[derive(Debug, Clone, Deserialize)]
pub struct Block {
    /// Show only for this biological sex ("male"/"female"); None = everyone.
    #[serde(default)]
    pub sex: Option<String>,
    /// Inline localized paragraph.
    #[serde(default)]
    pub text: Option<Loc>,
    /// Paragraph by i18n key (prose kept in the i18n store during migration).
    #[serde(default)]
    pub text_key: Option<String>,
    /// Bold sub-heading by i18n key.
    #[serde(default)]
    pub heading: Option<String>,
    /// Ordered list, each item an i18n key.
    #[serde(default)]
    pub list: Option<Vec<String>>,
    /// Render the section's task rows (+ a "section complete" line).
    #[serde(default)]
    pub tasks: bool,
    /// A named Rust widget (live data / interactive UI), with params.
    #[serde(default)]
    pub widget: Option<WidgetRef>,
}

/// A widget reference: an id resolved by the renderer's widget match, plus
/// arbitrary params (e.g. `{id: cta, route: /diary, label: story.bones.open_diary}`).
#[derive(Debug, Clone, Deserialize)]
pub struct WidgetRef {
    pub id: String,
    #[serde(flatten, default)]
    pub params: std::collections::BTreeMap<String, serde_json::Value>,
}

impl WidgetRef {
    pub fn param(&self, key: &str) -> Option<&str> {
        self.params.get(key).and_then(|v| v.as_str())
    }
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Word {
    Always,
}

/// A condition over the engine snapshot. Deserialized untagged from the YAML
/// shapes documented in `story.yaml` (the key sets are disjoint, so untagged is
/// unambiguous).
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum Cond {
    Word(Word),
    Event { event: String },
    Sensor {
        sensor: String,
        #[serde(default)]
        gte: Option<u32>,
    },
    SectionOpened { section_opened: String },
    TaskClosed { task: String },
    All { all: Vec<Cond> },
    Any { any: Vec<Cond> },
}

impl Default for Cond {
    fn default() -> Self {
        Cond::Word(Word::Always)
    }
}

// ── Parsing ──────────────────────────────────────────────────────────────────

// build.rs writes story.json into OUT_DIR from story/story.yaml.
const STORY_JSON: &str = include_str!(concat!(env!("OUT_DIR"), "/story.json"));

/// The parsed story (parsed once). Panics loudly on a malformed DSL — that's an
/// authoring bug we want to catch at first access, not silently ignore.
pub fn story() -> &'static Story {
    static CELL: OnceLock<Story> = OnceLock::new();
    CELL.get_or_init(|| {
        serde_json::from_str(STORY_JSON).expect("story_dsl: story.json failed to parse")
    })
}

/// Find a section (and its chapter) by section id.
pub fn find_section(id: &str) -> Option<(&'static Chapter, &'static Section)> {
    for ch in &story().chapters {
        if let Some(sec) = ch.sections.iter().find(|s| s.id == id) {
            return Some((ch, sec));
        }
    }
    None
}

// ── Sensors (numeric / boolean values read from Progress) ────────────────────

#[derive(Debug, Clone, Copy)]
enum SensorVal {
    Num(u32),
    Bool(bool),
}

impl SensorVal {
    fn as_num(self) -> u32 {
        match self {
            SensorVal::Num(n) => n,
            SensorVal::Bool(b) => b as u32,
        }
    }
    fn as_bool(self) -> bool {
        match self {
            SensorVal::Num(n) => n > 0,
            SensorVal::Bool(b) => b,
        }
    }
}

fn sensor(name: &str, p: &Progress) -> SensorVal {
    match name {
        "weight_streak" => SensorVal::Num(p.weight_streak),
        "steps_streak" => SensorVal::Num(p.steps_streak),
        "diary_streak" => SensorVal::Num(p.diary_streak),
        "diary_days" => SensorVal::Num(p.diary_days),
        "calorie_planka_set" => SensorVal::Bool(p.calorie_planka_set),
        "sub_active" => SensorVal::Bool(p.sub_active),
        "sub_paid" => SensorVal::Bool(p.sub_paid),
        "snack_yesterday" => SensorVal::Bool(p.s4_done),
        "no_high_cal_drink_yesterday" => SensorVal::Bool(p.s5_done),
        // Unknown sensor: an authoring bug, guarded by `all_sensors_known` test.
        _ => SensorVal::Bool(false),
    }
}

// ── Engine snapshot + evaluation ─────────────────────────────────────────────

/// A pure snapshot of everything the engine reads. Built off IndexedDB by
/// `story::engine_snapshot()`; pure here so evaluation is testable.
#[derive(Debug, Default, Clone)]
pub struct EngineSnapshot {
    pub progress: Progress,
    /// Section ids whose page has been opened (`opened:<id>` flags).
    pub opened: HashSet<String>,
    /// Task ids closed by an event firing while enabled (`evt_closed:<id>`).
    pub evt_closed: HashSet<String>,
    /// Chapter ids that have ever been open (`chapter_opened:<id>` — sticky).
    pub chapter_opened: HashSet<String>,
}

/// Read-only view bundling the parsed story with a snapshot, exposing the
/// chapter/section/task queries the UI needs.
pub struct Engine<'a> {
    pub story: &'a Story,
    pub snap: &'a EngineSnapshot,
}

impl<'a> Engine<'a> {
    pub fn new(story: &'a Story, snap: &'a EngineSnapshot) -> Self {
        Self { story, snap }
    }

    pub fn task(&self, id: &str) -> Option<&'a Task> {
        self.story.tasks.iter().find(|t| t.id == id)
    }

    fn eval(&self, c: &Cond) -> bool {
        match c {
            Cond::Word(Word::Always) => true,
            // Events are momentary, not standing conditions; a task's event-close
            // is resolved via `evt_closed` in `task_closed`, not here.
            Cond::Event { .. } => false,
            Cond::Sensor { sensor: name, gte } => {
                let v = sensor(name, &self.snap.progress);
                match gte {
                    Some(n) => v.as_num() >= *n,
                    None => v.as_bool(),
                }
            }
            Cond::SectionOpened { section_opened } => self.snap.opened.contains(section_opened),
            Cond::TaskClosed { task } => self.task_closed(task),
            Cond::All { all } => all.iter().all(|c| self.eval(c)),
            Cond::Any { any } => any.iter().any(|c| self.eval(c)),
        }
    }

    /// A task is done. Event-closes are persisted (`evt_closed`); everything else
    /// is evaluated live against sensors / opened sections / other tasks.
    pub fn task_closed(&self, id: &str) -> bool {
        let Some(t) = self.task(id) else { return false };
        match &t.close {
            Cond::Event { .. } => self.snap.evt_closed.contains(id),
            other => self.eval(other),
        }
    }

    /// Chapter is open: sticky once it has ever opened, else its `open` cond.
    pub fn chapter_open(&self, ch: &Chapter) -> bool {
        self.snap.chapter_opened.contains(&ch.id) || self.eval(&ch.open)
    }

    /// Section completes (unlocking the next): explicit `complete`, else all its
    /// tasks closed, else (no tasks) always.
    pub fn section_complete(&self, sec: &Section) -> bool {
        match &sec.complete {
            Some(c) => self.eval(c),
            None if sec.tasks.is_empty() => true,
            None => sec.tasks.iter().all(|t| self.task_closed(t)),
        }
    }

    /// Cumulative chain: a section is unlocked once its chapter is open and every
    /// earlier section in the chapter is complete.
    pub fn section_unlocked(&self, ch: &Chapter, idx: usize) -> bool {
        self.chapter_open(ch) && ch.sections[..idx].iter().all(|s| self.section_complete(s))
    }

    pub fn task_active(&self, t: &Task, chapter_open: bool) -> bool {
        chapter_open && self.eval(&t.enable) && !self.task_closed(&t.id)
    }

    /// The single list of tasks that are active right now (enabled, not closed,
    /// in an open chapter), in chapter→section→task order, deduped.
    pub fn active_tasks(&self) -> Vec<&'a Task> {
        let mut out: Vec<&Task> = Vec::new();
        for ch in &self.story.chapters {
            if !self.chapter_open(ch) {
                continue;
            }
            for sec in &ch.sections {
                for tid in &sec.tasks {
                    if out.iter().any(|t| &t.id == tid) {
                        continue;
                    }
                    if let Some(t) = self.task(tid) {
                        if self.task_active(t, true) {
                            out.push(t);
                        }
                    }
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap() -> EngineSnapshot {
        EngineSnapshot::default()
    }
    fn eng<'a>(s: &'a EngineSnapshot) -> Engine<'a> {
        Engine::new(story(), s)
    }
    fn chapter<'a>(id: &str) -> &'a Chapter {
        story().chapters.iter().find(|c| c.id == id).expect("chapter")
    }

    #[test]
    fn yaml_parses_and_has_three_chapters() {
        assert_eq!(story().chapters.len(), 3);
        assert_eq!(story().tasks.len(), 20);
    }

    #[test]
    fn all_referenced_sensors_are_known() {
        // Walk every Cond; assert each sensor name maps to a real sensor (the
        // `_ => false` fallback would silently break the DSL otherwise).
        const KNOWN: &[&str] = &[
            "weight_streak", "steps_streak", "diary_streak", "diary_days",
            "calorie_planka_set", "sub_active", "sub_paid",
            "snack_yesterday", "no_high_cal_drink_yesterday",
        ];
        fn walk(c: &Cond, known: &[&str]) {
            match c {
                Cond::Sensor { sensor, .. } => {
                    assert!(known.contains(&sensor.as_str()), "unknown sensor: {sensor}");
                }
                Cond::All { all } => all.iter().for_each(|c| walk(c, known)),
                Cond::Any { any } => any.iter().for_each(|c| walk(c, known)),
                _ => {}
            }
        }
        for t in &story().tasks {
            walk(&t.enable, KNOWN);
            walk(&t.close, KNOWN);
        }
        for ch in &story().chapters {
            walk(&ch.open, KNOWN);
            for s in &ch.sections {
                if let Some(c) = &s.complete {
                    walk(c, KNOWN);
                }
            }
        }
    }

    #[test]
    fn ch1_always_open_ch3_needs_diary_days() {
        let s = snap();
        let e = eng(&s);
        assert!(e.chapter_open(chapter("ch1")));
        assert!(!e.chapter_open(chapter("ch3")));

        let mut s2 = snap();
        s2.progress.diary_days = 7;
        assert!(eng(&s2).chapter_open(chapter("ch3")));
    }

    #[test]
    fn ch2_needs_streak_and_subscription() {
        let mut s = snap();
        s.progress.weight_streak = 7;
        assert!(!eng(&s).chapter_open(chapter("ch2"))); // no sub
        s.progress.sub_active = true;
        assert!(eng(&s).chapter_open(chapter("ch2")));
    }

    #[test]
    fn counter_close_at_seven() {
        let mut s = snap();
        assert!(!eng(&s).task_closed("weight_streak"));
        s.progress.weight_streak = 7;
        assert!(eng(&s).task_closed("weight_streak"));
    }

    #[test]
    fn first_food_is_armed_then_event_closed() {
        let s = snap();
        let first_food = story().tasks.iter().find(|t| t.id == "first_food").unwrap();
        // Not enabled until the section is opened (armed).
        assert!(!eng(&s).task_active(first_food, true));

        let mut opened = snap();
        opened.opened.insert("first-food".to_string());
        assert!(eng(&opened).task_active(first_food, true)); // armed → active
        assert!(!eng(&opened).task_closed("first_food"));

        let mut fired = opened.clone();
        fired.evt_closed.insert("first_food".to_string());
        assert!(eng(&fired).task_closed("first_food"));
    }

    #[test]
    fn setup_completes_on_lang_and_notif_not_sex() {
        let ch1 = chapter("ch1");
        let setup = ch1.sections.iter().find(|s| s.id == "setup").unwrap();
        let mut s = snap();
        s.evt_closed.insert("lang".to_string());
        assert!(!eng(&s).section_complete(setup)); // notif still open
        s.evt_closed.insert("notif".to_string());
        assert!(eng(&s).section_complete(setup)); // sex NOT required
    }

    #[test]
    fn active_list_grows_with_open_chapters() {
        // Fresh user: only ch1 open → its tasks are active (minus armed first_food).
        let s = snap();
        let active = eng(&s).active_tasks();
        assert!(active.iter().any(|t| t.id == "photos"));
        assert!(active.iter().all(|t| t.id != "first_food")); // armed
        assert!(active.iter().all(|t| t.id != "diary_streak")); // ch2 closed
    }
}
