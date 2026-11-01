//! 评测报告：以（JSON）落盘、以（Markdown）渲染（表格形态沿用基准
//! 汇总脚本的模式：一指标一行，比率带百分比）。

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalReport {
    pub mode: String,
    pub generated_at: i64,
    pub latency: Option<LatencySummary>,
    #[serde(default)]
    pub context: Option<EvalContext>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalContext {
    pub commit: Option<String>,
    pub source: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct LatencySummary {
    pub count: usize,
    pub p50: f64,
    pub p95: f64,
}

impl EvalReport {
    pub fn render_markdown(&self) -> String {
        let mut out = String::new();
        self.render_summary(&mut out);
        self.render_latency_rows(&mut out);
        out
    }

    fn render_summary(&self, out: &mut String) {
        line(
            out,
            format!(
                "Eval report: {} mode ({} UTC)",
                self.mode,
                format_utc(self.generated_at)
            ),
        );
        out.push('\n');
    }

    fn render_latency_rows(&self, out: &mut String) {
        let Some(latency) = &self.latency else {
            return;
        };
        out.push_str("| Metric | Value |\n| --- | --- |\n");
        line(
            out,
            format!("| end-to-end latency p50 | {:.2} ms |", latency.p50),
        );
        line(
            out,
            format!("| end-to-end latency p95 | {:.2} ms |", latency.p95),
        );
    }
}

fn line(out: &mut String, content: String) {
    out.push_str(&content);
    out.push('\n');
}

pub fn format_utc(secs: i64) -> String {
    let (date, hour, minute) = utc_parts(secs);
    format!("{date} {hour:02}:{minute:02}")
}

pub fn format_utc_short(secs: i64) -> String {
    let (date, hour, minute) = utc_parts(secs);
    format!("{} {hour:02}:{minute:02}", &date[5..])
}

fn utc_parts(secs: i64) -> (String, u32, u32) {
    let (year, month, day) = civil_from_days(secs.div_euclid(86_400));
    let day_secs = secs.rem_euclid(86_400);
    (
        format!("{year:04}-{month:02}-{day:02}"),
        (day_secs / 3_600) as u32,
        ((day_secs % 3_600) / 60) as u32,
    )
}

fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let shifted = days + 719_468;
    let era = if shifted >= 0 {
        shifted
    } else {
        shifted - 146_096
    } / 146_097;
    let day_of_era = (shifted - era * 146_097) as u64;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era as i64 + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let mp = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if month <= 2 { year + 1 } else { year }, month, day)
}

pub fn write_report(report: &EvalReport, report_dir: &std::path::Path) -> std::io::Result<String> {
    std::fs::create_dir_all(report_dir)?;
    let markdown = report.render_markdown();
    serde_json::to_writer_pretty(
        std::io::BufWriter::new(std::fs::File::create(
            report_dir.join(format!("{}.json", report.mode)),
        )?),
        report,
    )
    .map_err(std::io::Error::other)?;
    std::fs::write(report_dir.join(format!("{}.md", report.mode)), &markdown)?;
    Ok(markdown)
}

#[cfg(test)]
mod tests {
    use super::{EvalReport, LatencySummary};

    fn report(mode: &str, latency: Option<LatencySummary>) -> EvalReport {
        EvalReport {
            mode: mode.to_owned(),
            generated_at: 0,
            latency,
            context: None,
        }
    }

    #[test]
    fn generated_at_is_a_numeric_unix_timestamp() {
        let mut owned = report("iterate", None);
        owned.generated_at = 1_791_763_200;
        let json = serde_json::to_string(&owned).expect("serialize");
        assert!(
            json.contains(r#""generated_at":1791763200"#),
            "the timestamp serializes as a bare number: {json}"
        );
        let parsed: EvalReport = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(parsed.generated_at, 1_791_763_200);
    }

    #[test]
    fn utc_formatting_is_fixed_and_timezone_free() {
        assert_eq!(super::format_utc(0), "1970-01-01 00:00");
        assert_eq!(super::format_utc(1_791_763_200), "2026-10-12 00:00");
        assert_eq!(
            super::format_utc(1_791_763_200 + 86_399),
            "2026-10-12 23:59"
        );
        assert_eq!(super::format_utc(-1), "1969-12-31 23:59");
        assert_eq!(super::format_utc_short(-86_400), "12-31 00:00");
        assert_eq!(super::format_utc_short(1_000), "01-01 00:16");
    }

    #[test]
    fn latency_only_shows_when_sampled() {
        let unsampled = report("iterate", None);
        let markdown = unsampled.render_markdown();
        assert!(
            !markdown.contains("latency"),
            "no latency samples, no latency rows: {markdown}"
        );

        let sampled = report(
            "live",
            Some(LatencySummary {
                count: 2,
                p50: 12.5,
                p95: 40.0,
            }),
        );
        let markdown = sampled.render_markdown();
        assert!(
            markdown.contains("end-to-end latency p50 | 12.50 ms |")
                && markdown.contains("end-to-end latency p95 | 40.00 ms |"),
            "sampled percentiles render as table rows: {markdown}"
        );
    }

    #[test]
    fn legacy_rows_with_retired_fields_still_parse() {
        let legacy = r#"{"mode":"replay","generated_at":1000,"classify":null,"tasks":[],"latency":null,"judge":null,"skipped":0}"#;
        let parsed: EvalReport = serde_json::from_str(legacy).expect("legacy row parses");
        assert_eq!(parsed.mode, "replay");
        assert_eq!(parsed.generated_at, 1000);
        assert!(parsed.latency.is_none() && parsed.context.is_none());
    }
}
