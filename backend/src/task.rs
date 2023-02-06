use std::{
    collections::HashMap,
    fmt,
    time::{Duration, SystemTime},
};

use console_api::{tasks::Stats, Location};
use hdrhistogram::Histogram;
use serde::{Deserialize, Serialize};

use crate::{
    metadata::{MetaId, Metadata},
    util::Percentage,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Ord, PartialOrd, Deserialize, Serialize)]
pub struct TaskId(pub u64);

impl fmt::Display for TaskId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug)]
pub struct Task {
    pub id: TaskId,
    pub fields: Vec<Field>,
    pub stats: TaskStats,
    pub target: String,
    pub name: Option<String>,
    /// Currently active warnings for this task.
    // pub warnings: Vec<Linter<Task>>,
    pub location: String,
    pub histogram: Option<Histogram<u64>>,
}

impl Task {
    pub fn from_proto(
        metas: &HashMap<MetaId, Metadata>,
        stats_update: &mut HashMap<u64, Stats>,
        mut task: console_api::tasks::Task,
    ) -> Option<Self> {
        let id = match task.id {
            Some(id) => TaskId(id.id),
            None => {
                tracing::warn!(?task, "skipping task with no id");
                return None;
            }
        };
        let meta_id = match task.metadata.as_ref() {
            Some(id) => id.id,
            None => {
                tracing::warn!(?task, "task has no metadata ID, skipping");
                return None;
            }
        };
        let meta = match metas.get(&MetaId(meta_id)) {
            Some(meta) => meta,
            None => {
                tracing::warn!(?task, meta_id, "no metadata for task, skipping");
                return None;
            }
        };
        let stats = match stats_update.remove(&id.0) {
            Some(s) => s.into(),
            None => {
                tracing::warn!(?task, meta_id, "no stats for task, skipping");
                return None;
            }
        };
        let location = format_location(task.location);

        let mut name = None;
        let fields = task
            .fields
            .drain(..)
            .filter_map(|pb| {
                let field = Field::from_proto(pb, meta)?;
                // the `task.name` field gets its own column, if it's present.
                if &*field.name == Field::NAME {
                    name = Some(field.value.to_string());
                    return None;
                }
                Some(field)
            })
            .collect::<Vec<_>>();
        let task = Self {
            name,
            id,
            fields,
            stats,
            target: meta.target.clone(),
            location,
            histogram: None,
        };
        Some(task)
    }

    /// Returns `true` if this task is currently being polled.
    pub(crate) fn is_running(&self) -> bool {
        self.stats.last_poll_started > self.stats.last_poll_ended
    }

    pub(crate) fn is_completed(&self) -> bool {
        self.stats.total.is_some()
    }

    pub(crate) fn state(&self) -> TaskState {
        if self.is_completed() {
            return TaskState::Completed;
        }

        if self.is_running() {
            return TaskState::Running;
        }

        TaskState::Idle
    }

    pub(crate) fn total(&self, since: SystemTime) -> Duration {
        self.stats
            .total
            .ok_or(|| since.duration_since(self.stats.created_at).ok())
            .unwrap_or_default()
    }

    pub(crate) fn busy(&self, since: SystemTime) -> Duration {
        if let (Some(last_poll_started), None) =
            (self.stats.last_poll_started, self.stats.last_poll_ended)
        {
            // in this case the task is being polled at the moment
            let current_time_in_poll = since.duration_since(last_poll_started).unwrap_or_default();
            return self.stats.busy + current_time_in_poll;
        }
        self.stats.busy
    }

    pub(crate) fn idle(&self, since: SystemTime) -> Duration {
        self.stats
            .idle
            .or_else(|| self.total(since).checked_sub(self.busy(since)))
            .unwrap_or_default()
    }

    /// Returns the total number of times the task has been polled.
    pub(crate) fn total_polls(&self) -> u64 {
        self.stats.polls
    }

    /// Returns the elapsed time since the task was last woken, relative to
    /// given `now` timestamp.
    ///
    /// Returns `None` if the task has never been woken, or if it was last woken
    /// more recently than `now` (which *shouldn't* happen as long as `now` is the
    /// timestamp of the last stats update...)
    pub(crate) fn since_wake(&self, now: SystemTime) -> Option<Duration> {
        now.duration_since(self.last_wake()?).ok()
    }

    pub(crate) fn last_wake(&self) -> Option<SystemTime> {
        self.stats.last_wake
    }

    /// Returns the current number of wakers for this task.
    pub(crate) fn waker_count(&self) -> u64 {
        self.waker_clones().saturating_sub(self.waker_drops())
    }

    /// Returns the total number of times this task's waker has been cloned.
    pub(crate) fn waker_clones(&self) -> u64 {
        self.stats.waker_clones
    }

    /// Returns the total number of times this task's waker has been dropped.
    pub(crate) fn waker_drops(&self) -> u64 {
        self.stats.waker_drops
    }

    /// Returns the total number of times this task has been woken.
    pub(crate) fn wakes(&self) -> u64 {
        self.stats.wakes
    }

    /// Returns the total number of times this task has woken itself.
    pub(crate) fn self_wakes(&self) -> u64 {
        self.stats.self_wakes
    }

    /// Returns the percentage of this task's total wakeups that were self-wakes.
    pub(crate) fn self_wake_percent(&self) -> u64 {
        self.self_wakes().percent_of(self.wakes())
    }

    /// From the histogram, build a visual representation by trying to make as
    // many buckets as the width of the render area.
    pub(crate) fn make_chart_data(&self, width: u16) -> (Vec<u64>, HistogramMetadata) {
        self.histogram
            .as_ref()
            .map(|histogram| {
                let step_size =
                    ((histogram.max() - histogram.min()) as f64 / width as f64).ceil() as u64 + 1;
                // `iter_linear` panics if step_size is 0
                let data = if step_size > 0 {
                    let mut found_first_nonzero = false;
                    let data: Vec<u64> = histogram
                        .iter_linear(step_size)
                        .filter_map(|value| {
                            let count = value.count_since_last_iteration();
                            // Remove the 0s from the leading side of the buckets.
                            // Because HdrHistogram can return empty buckets depending
                            // on its internal state, as it approximates values.
                            if count == 0 && !found_first_nonzero {
                                None
                            } else {
                                found_first_nonzero = true;
                                Some(count)
                            }
                        })
                        .collect();
                    data
                } else {
                    Vec::new()
                };
                (
                    data,
                    HistogramMetadata {
                        max_value: histogram.max(),
                        min_value: histogram.min(),
                    },
                )
            })
            .unwrap_or_default()
    }
}

fn truncate_registry_path(s: String) -> String {
    use once_cell::sync::OnceCell;
    use regex::Regex;
    use std::borrow::Cow;

    static REGEX: OnceCell<Regex> = OnceCell::new();
    let regex = REGEX.get_or_init(|| {
        Regex::new(r#".*/\.cargo(/registry/src/[^/]*/|/git/checkouts/)"#)
            .expect("failed to compile regex")
    });

    return match regex.replace(&s, "<cargo>/") {
        Cow::Owned(s) => s,
        // String was not modified, return the original.
        Cow::Borrowed(_) => s.to_string(),
    };
}

fn format_location(loc: Option<Location>) -> String {
    loc.map(|mut l| {
        if let Some(file) = l.file.take() {
            let truncated = truncate_registry_path(file);
            l.file = Some(truncated);
        }
        format!("{l} ")
    })
    .unwrap_or_else(|| "<unknown location>".to_string())
}

#[derive(Debug)]
pub struct TaskStats {
    polls: u64,
    pub created_at: SystemTime,
    pub dropped_at: Option<SystemTime>,
    busy: Duration,
    pub last_poll_started: Option<SystemTime>,
    pub last_poll_ended: Option<SystemTime>,
    idle: Option<Duration>,
    total: Option<Duration>,

    // === waker stats ===
    /// Total number of times the task has been woken over its lifetime.
    wakes: u64,
    /// Total number of times the task's waker has been cloned
    waker_clones: u64,

    /// Total number of times the task's waker has been dropped.
    waker_drops: u64,

    /// The timestamp of when the task was last woken.
    last_wake: Option<SystemTime>,
    /// Total number of times the task has woken itself.
    self_wakes: u64,
}

impl From<console_api::tasks::Stats> for TaskStats {
    fn from(pb: console_api::tasks::Stats) -> Self {
        fn pb_duration(dur: prost_types::Duration) -> Duration {
            let secs =
                u64::try_from(dur.seconds).expect("a task should not have a negative duration!");
            let nanos =
                u64::try_from(dur.nanos).expect("a task should not have a negative duration!");
            Duration::from_secs(secs) + Duration::from_nanos(nanos)
        }

        let created_at = pb
            .created_at
            .expect("task span was never created")
            .try_into()
            .unwrap();

        let dropped_at: Option<SystemTime> = pb.dropped_at.map(|v| v.try_into().unwrap());
        let total = dropped_at.map(|d| d.duration_since(created_at).unwrap());

        let poll_stats = pb.poll_stats.expect("task should have poll stats");
        let busy = poll_stats.busy_time.map(pb_duration).unwrap_or_default();
        let idle = total.map(|total| total.checked_sub(busy).unwrap_or_default());
        Self {
            total,
            idle,
            busy,
            last_poll_started: poll_stats.last_poll_started.map(|v| v.try_into().unwrap()),
            last_poll_ended: poll_stats.last_poll_ended.map(|v| v.try_into().unwrap()),
            polls: poll_stats.polls,
            created_at,
            dropped_at,
            wakes: pb.wakes,
            waker_clones: pb.waker_clones,
            waker_drops: pb.waker_drops,
            last_wake: pb.last_wake.map(|v| v.try_into().unwrap()),
            self_wakes: pb.self_wakes,
        }
    }
}

#[derive(Debug)]
pub struct Field {
    pub(crate) name: String,
    pub(crate) value: FieldValue,
}

#[derive(Debug)]
pub enum FieldValue {
    Bool(bool),
    Str(String),
    U64(u64),
    I64(i64),
    Debug(String),
}

impl Field {
    const SPAWN_LOCATION: &'static str = "spawn.location";
    const NAME: &'static str = "task.name";

    /// Converts a wire-format `Field` into an internal `Field` representation,
    /// using the provided `Metadata` for the task span that the field came
    /// from.
    ///
    /// If the field is invalid or it has a string value which is empty, this
    /// returns `None`.
    fn from_proto(
        console_api::Field {
            name,
            metadata_id,
            value,
        }: console_api::Field,
        meta: &Metadata,
    ) -> Option<Self> {
        use console_api::field::Name;
        let name = match name? {
            Name::StrName(n) => n,
            Name::NameIdx(idx) => {
                let meta_id = metadata_id.map(|m| m.id);
                if meta_id != Some(meta.id.0) {
                    tracing::warn!(
                        task.meta_id = meta.id.0,
                        field.meta.id = ?meta_id,
                        field.name_index = idx,
                        ?meta,
                        "skipping malformed field name (metadata id mismatch)"
                    );
                    debug_assert_eq!(
                        meta_id,
                        Some(meta.id.0),
                        "malformed field name: metadata ID mismatch! (name idx={idx}; metadata={meta:#?})",
                    );
                    return None;
                }
                match meta.field_names.get(idx as usize).cloned() {
                    Some(name) => name,
                    None => {
                        tracing::warn!(
                            task.meta_id = meta.id.0,
                            field.meta.id = ?meta_id,
                            field.name_index = idx,
                            ?meta,
                            "missing field name for index"
                        );
                        return None;
                    }
                }
            }
        };

        debug_assert!(
            value.is_some(),
            "missing field value for field `{name:?}` (metadata={meta:#?})",
        );
        let mut value = FieldValue::from(value?)
            // if the value is an empty string, just skip it.
            .ensure_nonempty()?;

        if &*name == Self::SPAWN_LOCATION {
            value = value.truncate_registry_path();
        }

        Some(Self { name, value })
    }
}

impl fmt::Display for FieldValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FieldValue::Bool(v) => fmt::Display::fmt(v, f)?,
            FieldValue::Str(v) => fmt::Display::fmt(v, f)?,
            FieldValue::U64(v) => fmt::Display::fmt(v, f)?,
            FieldValue::Debug(v) => fmt::Display::fmt(v, f)?,
            FieldValue::I64(v) => fmt::Display::fmt(v, f)?,
        }

        Ok(())
    }
}

impl FieldValue {
    /// Truncates paths including `.cargo/registry`.
    fn truncate_registry_path(self) -> Self {
        match self {
            FieldValue::Str(s) | FieldValue::Debug(s) => Self::Debug(truncate_registry_path(s)),

            f => f,
        }
    }

    /// If `self` is an empty string, returns `None`. Otherwise, returns `Some(self)`.
    fn ensure_nonempty(self) -> Option<Self> {
        match self {
            FieldValue::Debug(s) | FieldValue::Str(s) if s.is_empty() => None,
            val => Some(val),
        }
    }
}

impl From<console_api::field::Value> for FieldValue {
    fn from(pb: console_api::field::Value) -> Self {
        match pb {
            console_api::field::Value::BoolVal(v) => Self::Bool(v),
            console_api::field::Value::StrVal(v) => Self::Str(v),
            console_api::field::Value::I64Val(v) => Self::I64(v),
            console_api::field::Value::U64Val(v) => Self::U64(v),
            console_api::field::Value::DebugVal(v) => Self::Debug(v),
        }
    }
}

#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub enum TaskState {
    Completed,
    Idle,
    Running,
}

impl TaskState {
    pub(crate) fn icon(self) -> &'static str {
        const RUNNING_UTF8: &str = "\u{25B6}";
        const IDLE_UTF8: &str = "\u{23F8}";
        const COMPLETED_UTF8: &str = "\u{23F9}";
        match self {
            Self::Running => RUNNING_UTF8,
            Self::Idle => IDLE_UTF8,
            Self::Completed => COMPLETED_UTF8,
        }
    }
}

#[derive(Debug, Default)]
pub(crate) struct HistogramMetadata {
    /// The max recorded value in the histogram. This is the label for the bottom-right in the chart
    pub(crate) max_value: u64,
    /// The min recorded value in the histogram.
    pub(crate) min_value: u64,
}
