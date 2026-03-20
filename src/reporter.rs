use std::io::{self, Write};
use anyhow::Result;
use crate::metrics::{FileMetrics, FunctionMetrics};

// ANSI escape codes (only used for "text" format on a TTY)
const RED: &str = "\x1b[31m";
const YELLOW: &str = "\x1b[33m";
const GREEN: &str = "\x1b[32m";
const BOLD: &str = "\x1b[1m";
const RESET: &str = "\x1b[0m";
const DIM: &str = "\x1b[2m";

pub fn report(
    results: &[FileMetrics],
    format: &str,
    cc_threshold: u32,
    cog_threshold: u32,
    only_violations: bool,
) -> Result<()> {
    match format {
        "json" => report_json(results, cc_threshold, cog_threshold, only_violations),
        "csv" => report_csv(results, cc_threshold, cog_threshold, only_violations),
        _ => report_text(results, cc_threshold, cog_threshold, only_violations),
    }
}

// ─── Text reporter ─────────────────────────────────────────────────────────────

fn report_text(
    results: &[FileMetrics],
    cc_threshold: u32,
    cog_threshold: u32,
    only_violations: bool,
) -> Result<()> {
    let use_color = atty::is(atty::Stream::Stdout);

    let c = |code: &str| if use_color { code } else { "" };

    let stdout = io::stdout();
    let mut out = stdout.lock();

    let mut total_functions = 0usize;
    let mut total_violations = 0usize;

    for file_metrics in results {
        writeln!(
            out,
            "\n{}{}{}{}",
            c(BOLD),
            "━━━ ",
            file_metrics.path.display(),
            c(RESET)
        )?;

        let fns: Vec<&FunctionMetrics> = file_metrics
            .functions
            .iter()
            .filter(|f| {
                !only_violations
                    || f.cyclomatic > cc_threshold
                    || f.cognitive > cog_threshold
            })
            .collect();

        if fns.is_empty() {
            writeln!(out, "  {}No functions found / no violations.{}", c(DIM), c(RESET))?;
            continue;
        }

        // Header
        writeln!(
            out,
            "  {}{:<40} {:>6} {:>6} {:>7} {:>6} {:>8} {:>8}{}",
            c(BOLD),
            "Function",
            "Line",
            "CC",
            "CogC",
            "Depth",
            "Params",
            "LOC",
            "Stmts",
            c(RESET)
        )?;
        writeln!(out, "  {}{}{}", c(DIM), "─".repeat(90), c(RESET))?;

        for f in &fns {
            total_functions += 1;

            let cc_violation = f.cyclomatic > cc_threshold;
            let cog_violation = f.cognitive > cog_threshold;
            let any_violation = cc_violation || cog_violation;

            if any_violation {
                total_violations += 1;
            }

            let row_color = if any_violation { c(RED) } else { c(GREEN) };

            writeln!(
                out,
                "  {}{:<40} {:>6} {:>6} {:>7} {:>6} {:>8} {:>8}{}",
                row_color,
                truncate(&f.name, 40),
                f.line,
                cc_badge(f.cyclomatic, cc_threshold, use_color),
                cog_badge(f.cognitive, cog_threshold, use_color),
                f.max_nesting,
                f.param_count,
                f.loc,
                f.statement_count,
                c(RESET),
            )?;

            // Halstead sub-row
            writeln!(
                out,
                "    {}Halstead → V={:.1}  D={:.2}  E={:.0}  bugs≈{:.2}  time≈{:.0}s{}",
                c(DIM),
                f.halstead.volume,
                f.halstead.difficulty,
                f.halstead.effort,
                f.halstead.delivered_bugs,
                f.halstead.time_to_program,
                c(RESET),
            )?;
        }
    }

    // Summary
    writeln!(out)?;
    writeln!(
        out,
        "{}{}Summary:{} {} function(s) analysed, {} violation(s) (CC>{} or CogC>{}).",
        c(BOLD),
        if total_violations > 0 { c(YELLOW) } else { c(GREEN) },
        c(RESET),
        total_functions,
        total_violations,
        cc_threshold,
        cog_threshold,
    )?;

    Ok(())
}

fn cc_badge(val: u32, threshold: u32, color: bool) -> String {
    if !color {
        return format!("{:>6}", val);
    }
    if val > threshold {
        format!("{}{:>6}{}", RED, val, RESET)
    } else if val > threshold / 2 {
        format!("{}{:>6}{}", YELLOW, val, RESET)
    } else {
        format!("{}{:>6}{}", GREEN, val, RESET)
    }
}

fn cog_badge(val: u32, threshold: u32, color: bool) -> String {
    if !color {
        return format!("{:>7}", val);
    }
    if val > threshold {
        format!("{}{:>7}{}", RED, val, RESET)
    } else if val > threshold / 2 {
        format!("{}{:>7}{}", YELLOW, val, RESET)
    } else {
        format!("{}{:>7}{}", GREEN, val, RESET)
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}…", &s[..max - 1])
    }
}

// ─── JSON reporter ─────────────────────────────────────────────────────────────

fn report_json(
    results: &[FileMetrics],
    cc_threshold: u32,
    cog_threshold: u32,
    only_violations: bool,
) -> Result<()> {
    use serde_json::{json, Value};

    let output: Vec<Value> = results
        .iter()
        .map(|fm| {
            let fns: Vec<Value> = fm
                .functions
                .iter()
                .filter(|f| {
                    !only_violations
                        || f.cyclomatic > cc_threshold
                        || f.cognitive > cog_threshold
                })
                .map(|f| {
                    json!({
                        "name": f.name,
                        "file": f.file,
                        "line": f.line,
                        "cyclomatic_complexity": f.cyclomatic,
                        "cognitive_complexity": f.cognitive,
                        "max_nesting_depth": f.max_nesting,
                        "param_count": f.param_count,
                        "loc": f.loc,
                        "statement_count": f.statement_count,
                        "halstead": {
                            "operators_total": f.halstead.n1,
                            "operands_total": f.halstead.n2,
                            "operators_unique": f.halstead.nu1,
                            "operands_unique": f.halstead.nu2,
                            "vocabulary": f.halstead.vocabulary,
                            "length": f.halstead.length,
                            "volume": round2(f.halstead.volume),
                            "difficulty": round2(f.halstead.difficulty),
                            "effort": round2(f.halstead.effort),
                            "time_to_program_s": round2(f.halstead.time_to_program),
                            "delivered_bugs": round2(f.halstead.delivered_bugs),
                        },
                        "violations": {
                            "cyclomatic": f.cyclomatic > cc_threshold,
                            "cognitive": f.cognitive > cog_threshold,
                        }
                    })
                })
                .collect();

            json!({
                "file": fm.path.to_string_lossy(),
                "functions": fns,
            })
        })
        .collect();

    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}

fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

// ─── CSV reporter ──────────────────────────────────────────────────────────────

fn report_csv(
    results: &[FileMetrics],
    cc_threshold: u32,
    cog_threshold: u32,
    only_violations: bool,
) -> Result<()> {
    let stdout = io::stdout();
    let mut out = stdout.lock();

    writeln!(
        out,
        "file,function,line,cyclomatic,cognitive,max_nesting,params,loc,statements,\
         halstead_volume,halstead_difficulty,halstead_effort,halstead_bugs,cc_violation,cog_violation"
    )?;

    for fm in results {
        for f in &fm.functions {
            if only_violations && f.cyclomatic <= cc_threshold && f.cognitive <= cog_threshold {
                continue;
            }
            writeln!(
                out,
                "{},{},{},{},{},{},{},{},{},{:.2},{:.2},{:.2},{:.4},{},{}",
                fm.path.display(),
                csv_escape(&f.name),
                f.line,
                f.cyclomatic,
                f.cognitive,
                f.max_nesting,
                f.param_count,
                f.loc,
                f.statement_count,
                f.halstead.volume,
                f.halstead.difficulty,
                f.halstead.effort,
                f.halstead.delivered_bugs,
                f.cyclomatic > cc_threshold,
                f.cognitive > cog_threshold,
            )?;
        }
    }

    Ok(())
}

fn csv_escape(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}
