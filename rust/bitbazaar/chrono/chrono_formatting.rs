/// Formats a [`chrono::Duration`]` in the present case.
/// Arguments:
/// - `td`: The duration to format.
/// - `precise`: If true, smaller units will be included.
pub fn chrono_format_td(td: chrono::Duration, precise: bool) -> String {
    // The rough formatting does some funkiness with "now" when less than 10 seconds, so just handle simple seconds case manually:
    if !precise && td.num_seconds() <= 59 {
        let secs = td.num_seconds().max(1);
        format!("{} second{}", secs, if secs > 1 { "s" } else { "" })
    } else {
        chrono_humanize::HumanTime::from(td).to_text_en(
            if precise {
                chrono_humanize::Accuracy::Precise
            } else {
                chrono_humanize::Accuracy::Rough
            },
            chrono_humanize::Tense::Present,
        )
    }
}

/// Convert a chrono datetime to the local timezone.
///
/// Arguments:
/// - `dt`: The datetime to convert.
pub fn chrono_dt_to_local(dt: chrono::DateTime<chrono::Utc>) -> chrono::DateTime<chrono::Local> {
    dt.with_timezone(&chrono::Local)
}

/// Formats a [`chrono::DateTime`], also localising it to the user's timezone.
///
/// Arguments:
/// - `dt`: The datetime to format.
pub fn chrono_format_dt(dt: chrono::DateTime<chrono::Utc>) -> String {
    let dt = chrono_dt_to_local(dt);

    let date = dt.date_naive();
    let today = chrono::Local::now().date_naive();
    let yesterday = today.pred_opt();

    // If today then: "Today, 12:34"
    if date == today {
        format!("Today, {}", dt.format("%H:%M"))
    }
    // Same for yesterday:
    else if yesterday.is_some() && date == yesterday.unwrap() {
        format!("Yesterday, {}", dt.format("%H:%M"))
    } else {
        // Otherwise e.g. 15 March 2021, 12:34
        dt.format("%-e %B %Y, %H:%M").to_string()
    }
}
