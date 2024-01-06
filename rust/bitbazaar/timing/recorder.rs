use std::sync::Arc;

use once_cell::sync::Lazy;
use parking_lot::Mutex;
use tracing::warn;

use crate::{err, errors::TracedErr, timing::format_duration};

/// A global time recorder, used by the timeit! macro.
pub static GLOBAL_TIME_RECORDER: Lazy<TimeRecorder> = Lazy::new(TimeRecorder::new);

/// A struct for recording time spent in various blocks of code.
pub struct TimeRecorder {
    start: chrono::DateTime<chrono::Utc>,
    logs: Arc<Mutex<Vec<(String, std::time::Duration)>>>,
}

impl Default for TimeRecorder {
    fn default() -> Self {
        Self::new()
    }
}

impl TimeRecorder {
    /// Create a new time recorder.
    pub fn new() -> Self {
        Self {
            start: chrono::Utc::now(),
            logs: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Time a block of code and log to the time recorder.
    pub fn timeit<R, F: FnOnce() -> R>(&self, description: &str, f: F) -> R {
        let now = std::time::Instant::now();
        let res = f();
        let elapsed = now.elapsed();

        if let Some(mut logs) = self.logs.try_lock() {
            logs.push((description.to_owned(), elapsed));
        } else {
            warn!("Failed to acquire logs lock, skipping timeit logging. Tried to log '{}' with '{}' elapsed.", description, format_duration(elapsed));
        }

        res
    }

    /// Using from creation time rather than the specific durations recorded, to be sure to cover everything.
    pub fn total_elapsed(&self) -> Result<std::time::Duration, TracedErr> {
        Ok((chrono::Utc::now() - self.start).to_std()?)
    }

    /// Format the logs in a verbose, table format.
    pub fn format_verbose(&self) -> Result<String, TracedErr> {
        use comfy_table::*;

        // Printing should only happen at the end synchronously, shouldn't fail to acquire:
        let logs = self
            .logs
            .try_lock()
            .ok_or_else(|| err!("Failed to acquire logs."))?;

        let mut table = Table::new();
        table
            .load_preset(presets::UTF8_FULL)
            .set_content_arrangement(ContentArrangement::Dynamic);
        table.set_header(vec!["Description", "Elapsed"]);

        for (description, duration) in logs.iter() {
            table.add_row(vec![description, &format_duration(*duration)]);
        }

        table.add_row(vec![
            Cell::new("Elapsed from beginning").add_attribute(Attribute::Bold),
            Cell::new(format_duration(self.total_elapsed()?)).add_attribute(Attribute::Bold),
        ]);

        // Centralize the time column:
        let time_column = table
            .column_mut(1)
            .ok_or_else(|| err!("Failed to get second column of time recorder table"))?;
        time_column.set_cell_alignment(CellAlignment::Center);

        Ok(table.to_string())
    }
}
