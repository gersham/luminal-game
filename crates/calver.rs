// Game stamp: `YY.MM.SERIAL` (UTC). Release tags are `vYY.MM.N`.
// SERIAL starts at 1 each month. Any other tree is `YY.MM.0-dev`.

pub fn utc_year_month(unix_secs: u64) -> (u32, u32) {
    let (year, month, _) = civil_from_days((unix_secs / 86_400) as i64);
    (year.rem_euclid(100) as u32, month)
}

pub fn parse_release_tag(tag: &str) -> Option<(u32, u32, u32)> {
    let mut parts = tag.strip_prefix('v')?.split('.');
    let year: u32 = parts.next()?.parse().ok()?;
    let month: u32 = parts.next()?.parse().ok()?;
    let serial: u32 = parts.next()?.parse().ok()?;
    if parts.next().is_some() || !(0..100).contains(&year) || !(1..=12).contains(&month) || serial == 0 {
        return None;
    }
    // Reject leading zeros and channel suffixes: only the canonical tag counts.
    (tag == format!("v{year:02}.{month:02}.{serial}")).then_some((year, month, serial))
}

pub fn product_version(tags_at_head: &[&str], dirty: bool, year_yy: u32, month: u32) -> String {
    if !dirty && let Some(version) = newest_release(tags_at_head) {
        return version;
    }
    format!("{:02}.{:02}.0-dev", year_yy % 100, month)
}

fn newest_release(tags: &[&str]) -> Option<String> {
    tags.iter().filter_map(|tag| parse_release_tag(tag)).max().map(|(year, month, serial)| format!("{year:02}.{month:02}.{serial}"))
}

/// Howard Hinnant's civil_from_days, days since 1970-01-01.
fn civil_from_days(days_since_epoch: i64) -> (i32, u32, u32) {
    let z = days_since_epoch + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097) as u64;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { year + 1 } else { year };
    (year as i32, month as u32, day)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utc_months_cover_the_epoch_a_leap_day_and_this_month() {
        assert_eq!(utc_year_month(0), (70, 1));
        assert_eq!(utc_year_month(951_782_400), (0, 2));
        assert_eq!(utc_year_month(951_868_800), (0, 3));
        assert_eq!(utc_year_month(1_790_640_000), (26, 9));
        assert_eq!(utc_year_month(1_790_812_800), (26, 10));
    }

    #[test]
    fn only_canonical_release_tags_count() {
        assert_eq!(parse_release_tag("v26.09.1"), Some((26, 9, 1)));
        assert_eq!(parse_release_tag("v26.09.10"), Some((26, 9, 10)));
        for tag in ["v26.9.1", "v26.09.0", "v26.09.01", "v26.09.1-beta", "26.09.1", "v26.13.1", "v2026.09.1"] {
            assert_eq!(parse_release_tag(tag), None, "{tag}");
        }
    }

    #[test]
    fn clean_head_reuses_its_highest_tag_and_everything_else_is_dev() {
        let tags = ["v26.08.4", "v26.09.2", "v26.09.10", "v26.9.9", "nightly"];
        assert_eq!(product_version(&tags, false, 26, 9), "26.09.10");
        assert_eq!(product_version(&["v26.08.4"], false, 26, 9), "26.08.4");
        assert_eq!(product_version(&tags, true, 26, 9), "26.09.0-dev");
        assert_eq!(product_version(&[], false, 26, 10), "26.10.0-dev");
        assert_eq!(product_version(&["v26.09.1"], false, 2026, 9), "26.09.1");
    }
}
