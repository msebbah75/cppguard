mod solid;
use solid::{check_solid, SolidConfig, SolidViolation, SolidPrinciple, Severity};
use clang::{Clang, EntityKind, Index, TranslationUnit};
use clap::Parser;
use colored::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use walkdir::WalkDir;

// ─── CLI ─────────────────────────────────────────────────────────────────────

#[derive(Parser, Debug)]
#[command(
    name = "cpp_guard",
    about = "Analyse C++ code complexity via the Clang AST",
    version
)]
struct Cli {
    /// C++ source files or directories to analyse
    #[arg(required = true)]
    paths: Vec<PathBuf>,

    /// Extra compiler flags forwarded to Clang (e.g. -std=c++17 -I./include)
    #[arg(short = 'f', long = "flag", value_name = "FLAG", allow_hyphen_values = true)]
    compiler_flags: Vec<String>,

    /// Output results as JSON
    #[arg(long)]
    json: bool,

    /// Cyclomatic complexity threshold that triggers a warning
    #[arg(long, default_value = "10")]
    warn_complexity: u32,

    /// Maximum nesting depth threshold that triggers a warning
    #[arg(long, default_value = "5")]
    warn_depth: u32,

    /// Recursively scan directories
    #[arg(short, long)]
    recursive: bool,

    /// Disable SOLID principle checks
    #[arg(long)]
    no_solid: bool,

    /// Max public methods before SRP warning [default: 10]
    #[arg(long, default_value = "10")]
    srp_max_methods: usize,

    /// Max pure-virtual methods in an interface before ISP warning [default: 7]
    #[arg(long, default_value = "7")]
    isp_max_methods: usize,

    /// Max switch/if arms before OCP warning [default: 5]
    #[arg(long, default_value = "5")]
    ocp_max_arms: u32,

    /// Max `new` expressions in a function before DIP warning [default: 2]
    #[arg(long, default_value = "2")]
    dip_max_new: u32,
}

// ─── Data model ──────────────────────────────────────────────────────────────

/// Complexity metrics for a single function / method.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionMetrics {
    pub name: String,
    pub qualified_name: String,
    pub file: String,
    pub line: u32,
    /// McCabe cyclomatic complexity  (1 + number of decision points)
    pub cyclomatic_complexity: u32,
    /// Maximum nesting depth of control-flow structures
    pub max_nesting_depth: u32,
    /// Total number of AST nodes visited inside the function
    pub ast_node_count: u32,
    /// Number of parameters
    pub parameter_count: u32,
    /// Number of local variables declared
    pub local_variable_count: u32,
    /// Number of return statements
    pub return_count: u32,
    /// Halstead-inspired raw counts
    pub halstead: HalsteadRaw,
    /// Composite maintainability score  (0–100, higher = easier to maintain)
    pub maintainability_index: f64,
}

/// Raw Halstead operator / operand counts collected from the AST.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HalsteadRaw {
    pub distinct_operators: u32,
    pub distinct_operands: u32,
    pub total_operators: u32,
    pub total_operands: u32,
}

impl HalsteadRaw {
    pub fn vocabulary(&self) -> u32 { self.distinct_operators + self.distinct_operands }
    pub fn length(&self)    -> u32 { self.total_operators + self.total_operands }

    pub fn calculated_length(&self) -> f64 {
        let safe_log = |x: f64| if x > 0.0 { x * x.log2() } else { 0.0 };
        safe_log(self.distinct_operators as f64) + safe_log(self.distinct_operands as f64)
    }

    pub fn volume(&self) -> f64 {
        let vocab = self.vocabulary() as f64;
        if vocab > 0.0 { self.length() as f64 * vocab.log2() } else { 0.0 }
    }

    pub fn difficulty(&self) -> f64 {
        if self.distinct_operands == 0 { return 0.0; }
        (self.distinct_operators as f64 / 2.0)
            * (self.total_operands as f64 / self.distinct_operands as f64)
    }

    pub fn effort(&self) -> f64 { self.difficulty() * self.volume() }
}

/// Per-file summary.
#[derive(Debug, Serialize, Deserialize)]
pub struct FileReport {
    pub path: String,
    pub functions: Vec<FunctionMetrics>,
    pub parse_errors: Vec<String>,
    pub solid_violations: Vec<SolidViolation>,
}

impl FileReport {
    pub fn avg_cyclomatic(&self) -> f64 {
        if self.functions.is_empty() { return 0.0; }
        self.functions.iter().map(|f| f.cyclomatic_complexity as f64).sum::<f64>()
            / self.functions.len() as f64
    }
    pub fn max_cyclomatic(&self) -> u32 {
        self.functions.iter().map(|f| f.cyclomatic_complexity).max().unwrap_or(0)
    }
}

// ─── AST visitor ─────────────────────────────────────────────────────────────

struct VisitorState {
    cyclomatic: u32,
    max_depth: u32,
    ast_nodes: u32,
    locals: u32,
    returns: u32,
    operators: HashMap<String, u32>,
    operands: HashMap<String, u32>,
}

impl VisitorState {
    fn new() -> Self {
        Self {
            cyclomatic: 1,
            max_depth: 0,
            ast_nodes: 0,
            locals: 0,
            returns: 0,
            operators: HashMap::new(),
            operands: HashMap::new(),
        }
    }
    fn add_op(&mut self, k: &str)  { *self.operators.entry(k.to_string()).or_insert(0) += 1; }
    fn add_and(&mut self, k: &str) { *self.operands.entry(k.to_string()).or_insert(0) += 1; }

    fn halstead(&self) -> HalsteadRaw {
        HalsteadRaw {
            distinct_operators: self.operators.len() as u32,
            distinct_operands:  self.operands.len()  as u32,
            total_operators:    self.operators.values().sum(),
            total_operands:     self.operands.values().sum(),
        }
    }
}

fn visit_entity(entity: &clang::Entity, state: &mut VisitorState, depth: u32) {
    state.ast_nodes += 1;
    if depth > state.max_depth { state.max_depth = depth; }

    match entity.get_kind() {
        EntityKind::IfStmt | EntityKind::ConditionalOperator => {
            state.cyclomatic += 1;
            state.add_op("if");
        }
        EntityKind::ForStmt | EntityKind::ForRangeStmt => {
            state.cyclomatic += 1;
            state.add_op("for");
        }
        EntityKind::WhileStmt => { state.cyclomatic += 1; state.add_op("while"); }
        EntityKind::DoStmt    => { state.cyclomatic += 1; state.add_op("do");    }
        EntityKind::CaseStmt  => { state.cyclomatic += 1; state.add_op("case");  }
        EntityKind::CatchStmt => { state.cyclomatic += 1; state.add_op("catch"); }
        EntityKind::GotoStmt  => { state.cyclomatic += 1; state.add_op("goto");  }

        EntityKind::BinaryOperator | EntityKind::CompoundAssignOperator => {
            state.add_op("binop");
        }
        EntityKind::UnaryOperator       => { state.add_op("unaryop");      }
        EntityKind::CallExpr            => { state.add_op("call");          }

        EntityKind::DeclRefExpr => {
            if let Some(name) = entity.get_display_name() { state.add_and(&name); }
        }
        EntityKind::IntegerLiteral | EntityKind::FloatingLiteral
        | EntityKind::StringLiteral | EntityKind::CharacterLiteral
        | EntityKind::BoolLiteralExpr | EntityKind::NullPtrLiteralExpr => {
            state.add_and("<literal>");
        }

        EntityKind::VarDecl => {
            if entity.get_semantic_parent().map_or(false, |p| matches!(
                p.get_kind(),
                EntityKind::FunctionDecl | EntityKind::Method
                | EntityKind::Constructor | EntityKind::Destructor
                | EntityKind::CompoundStmt
            )) { state.locals += 1; }
        }

        EntityKind::ReturnStmt => { state.returns += 1; state.add_op("return"); }
        _ => {}
    }

    let next_depth = match entity.get_kind() {
        EntityKind::IfStmt | EntityKind::ForStmt | EntityKind::ForRangeStmt
        | EntityKind::WhileStmt | EntityKind::DoStmt
        | EntityKind::SwitchStmt | EntityKind::TryStmt => depth + 1,
        _ => depth,
    };

    for child in entity.get_children() {
        visit_entity(&child, state, next_depth);
    }
}

// ─── Metrics helpers ─────────────────────────────────────────────────────────

/// Microsoft variant of Maintainability Index, clamped to [0, 100].
fn maintainability_index(volume: f64, cyclomatic: u32, loc: u32) -> f64 {
    let v   = if volume > 0.0 { volume } else { 1.0 };
    let loc = if loc > 0 { loc } else { 1 };
    let raw = 171.0 - 5.2 * v.ln() - 0.23 * cyclomatic as f64 - 16.2 * (loc as f64).ln();
    (raw * 100.0 / 171.0).clamp(0.0, 100.0)
}

fn build_metrics(entity: &clang::Entity) -> Option<FunctionMetrics> {
    let name      = entity.get_name().unwrap_or_else(|| "<anonymous>".into());
    let qualified = entity.get_display_name().unwrap_or_else(|| name.clone());

    let loc_info = entity.get_location()?.get_file_location();
    let file = loc_info.file
        .map(|f| f.get_path().to_string_lossy().into_owned())
        .unwrap_or_default();
    let line = loc_info.line;

    let param_count = entity.get_arguments().map(|a| a.len() as u32).unwrap_or(0);
    let loc_count   = entity.get_range().map(|r| {
        let s = r.get_start().get_file_location().line;
        let e = r.get_end().get_file_location().line;
        e.saturating_sub(s) + 1
    }).unwrap_or(1);

    let mut state = VisitorState::new();
    for child in entity.get_children() {
        visit_entity(&child, &mut state, 1);
    }

    let halstead = state.halstead();
    let mi = maintainability_index(halstead.volume(), state.cyclomatic, loc_count);

    Some(FunctionMetrics {
        name,
        qualified_name: qualified,
        file,
        line,
        cyclomatic_complexity:  state.cyclomatic,
        max_nesting_depth:      state.max_depth,
        ast_node_count:         state.ast_nodes,
        parameter_count:        param_count,
        local_variable_count:   state.locals,
        return_count:           state.returns,
        halstead,
        maintainability_index:  mi,
    })
}

// ─── Analysis ────────────────────────────────────────────────────────────────

fn collect_functions(entity: &clang::Entity, source_path: &str, out: &mut Vec<FunctionMetrics>) {
    match entity.get_kind() {
        EntityKind::FunctionDecl | EntityKind::Method
        | EntityKind::Constructor | EntityKind::Destructor
        | EntityKind::FunctionTemplate => {
            if entity.is_definition() {
                let in_file = entity.get_location()
                    .and_then(|l| l.get_file_location().file)
                    .map_or(false, |f| f.get_path().to_string_lossy() == source_path);
                if in_file {
                    if let Some(m) = build_metrics(entity) { out.push(m); }
                }
            }
        }
        _ => {}
    }
    for child in entity.get_children() {
        collect_functions(&child, source_path, out);
    }
}

fn analyse_tu(
    tu: &TranslationUnit,
    source_path: &str,
    solid_cfg: Option<&SolidConfig>,
) -> (Vec<FunctionMetrics>, Vec<String>, Vec<SolidViolation>) {
    let mut metrics = Vec::new();
    let mut errors  = Vec::new();

    for diag in tu.get_diagnostics() {
        use clang::diagnostic::Severity;
        if matches!(diag.get_severity(), Severity::Error | Severity::Fatal) {
            errors.push(diag.get_text());
        }
    }

    collect_functions(&tu.get_entity(), source_path, &mut metrics);

    let solid = match solid_cfg {
        Some(cfg) => check_solid(&tu.get_entity(), source_path, cfg),
        None      => Vec::new(),
    };

    (metrics, errors, solid)
}

// ─── File discovery ───────────────────────────────────────────────────────────

fn collect_cpp_files(paths: &[PathBuf], recursive: bool) -> Vec<PathBuf> {
    let exts = ["cpp", "cxx", "cc", "c++", "C", "h", "hpp", "hxx", "h++"];
    let mut files = Vec::new();
    for path in paths {
        if path.is_file() {
            files.push(path.clone());
        } else if path.is_dir() {
            let walker = if recursive { WalkDir::new(path) } else { WalkDir::new(path).max_depth(1) };
            for entry in walker.into_iter().filter_map(|e| e.ok()) {
                let p = entry.path().to_path_buf();
                if p.is_file() {
                    if let Some(ext) = p.extension().and_then(|e| e.to_str()) {
                        if exts.contains(&ext) { files.push(p); }
                    }
                }
            }
        }
    }
    files
}

// ─── Console output ───────────────────────────────────────────────────────────

fn cc_label(cc: u32) -> ColoredString {
    match cc {
        1..=5  => format!("{:>3}", cc).green(),
        6..=10 => format!("{:>3}", cc).yellow(),
        11..=20 => format!("{:>3}", cc).red(),
        _      => format!("{:>3}", cc).bright_red().bold(),
    }
}

fn depth_label(d: u32) -> ColoredString {
    match d {
        0..=3 => format!("{}", d).green(),
        4..=5 => format!("{}", d).yellow(),
        _     => format!("{}", d).red(),
    }
}

fn mi_label(mi: f64) -> ColoredString {
    let s = format!("{:5.1}", mi);
    if mi >= 65.0 { s.green() } else if mi >= 40.0 { s.yellow() } else { s.red() }
}

fn print_report(reports: &[FileReport], warn_cc: u32, warn_depth: u32) {
    const W: usize = 52;
    println!();
    println!("{}", "═══ C++ Complexity Analysis ═══".bold().cyan());

    for report in reports {
        println!("\n{} {}", "▶".bold(), report.path.bright_white().bold());

        for e in &report.parse_errors {
            println!("  {} {}", "⚠ Parse error:".yellow(), e);
        }

        if report.functions.is_empty() {
            println!("  (no function definitions found)");
            continue;
        }

        println!(
            "  {:<W$} {:>4}  {:>5}  {:>5}  {:>5}  {:>5}  {:>5}",
            "Function".bold(), "CC".bold(), "Depth".bold(),
            "Nodes".bold(), "Param".bold(), "MI".bold(), "Line".bold()
        );
        println!("  {}", "─".repeat(W + 38));

        let mut warned: Vec<&FunctionMetrics> = Vec::new();

        for f in &report.functions {
            let name = if f.qualified_name.len() > W {
                format!("{}…", &f.qualified_name[..W - 1])
            } else {
                f.qualified_name.clone()
            };
            println!(
                "  {:<W$} {}  {}  {:>5}  {:>5}  {}  {:>5}",
                name,
                cc_label(f.cyclomatic_complexity),
                depth_label(f.max_nesting_depth),
                f.ast_node_count,
                f.parameter_count,
                mi_label(f.maintainability_index),
                f.line,
            );
            if f.cyclomatic_complexity >= warn_cc || f.max_nesting_depth >= warn_depth {
                warned.push(f);
            }
        }

        if !warned.is_empty() {
            println!("\n  {} Threshold violations:", "⚠".yellow().bold());
            for f in &warned {
                let mut reasons = Vec::new();
                if f.cyclomatic_complexity >= warn_cc {
                    reasons.push(format!("CC={} ≥ {}", f.cyclomatic_complexity, warn_cc));
                }
                if f.max_nesting_depth >= warn_depth {
                    reasons.push(format!("depth={} ≥ {}", f.max_nesting_depth, warn_depth));
                }
                println!("    {} {} ({})", "→".yellow(), f.qualified_name.bold(), reasons.join(", ").yellow());
            }
        }

        println!(
            "\n  Summary: {} functions | avg CC {:.1} | max CC {}",
            report.functions.len(), report.avg_cyclomatic(), report.max_cyclomatic()
        );

        // ── SOLID violations for this file ──────────────────────────────────
        if !report.solid_violations.is_empty() {
            println!("\n  {} SOLID principle findings:", "◈".cyan().bold());
            println!(
                "  {:<4} {:<22} {:<35} {}",
                "Sev".bold(), "Principle".bold(), "Entity".bold(), "Line".bold()
            );
            println!("  {}", "─".repeat(75));

            for v in &report.solid_violations {
                let sev_str = match v.severity {
                    Severity::Error   => format!("{:<4}", "ERR").red().bold(),
                    Severity::Warning => format!("{:<4}", "WARN").yellow(),
                    Severity::Info    => format!("{:<4}", "info").dimmed(),
                };
                let principle_str = format!(
                    "[{}] {:<18}",
                    v.principle.letter(),
                    v.principle.name()
                );
                let principle_colored = match v.principle {
                    SolidPrinciple::SingleResponsibility => principle_str.cyan(),
                    SolidPrinciple::OpenClosed           => principle_str.blue(),
                    SolidPrinciple::LiskovSubstitution   => principle_str.magenta(),
                    SolidPrinciple::InterfaceSegregation => principle_str.bright_cyan(),
                    SolidPrinciple::DependencyInversion  => principle_str.bright_blue(),
                };
                let entity = if v.entity.len() > 33 {
                    format!("{}…", &v.entity[..32])
                } else {
                    format!("{:<35}", v.entity)
                };
                println!("  {} {} {} {:>4}", sev_str, principle_colored, entity, v.line);
                println!("       {}", v.detail.dimmed());
            }

            let warn_count = report.solid_violations.iter()
                .filter(|v| v.severity >= Severity::Warning).count();
            println!(
                "\n  SOLID: {} finding(s), {} warning(s)/error(s)",
                report.solid_violations.len(), warn_count
            );
        }
    }

    println!();
    println!("{}", "Legend:".bold());
    println!("  CC = Cyclomatic Complexity   {} ≤5   {} 6-10   {} 11-20   {} >20",
        "■".green(), "■".yellow(), "■".red(), "■".bright_red());
    println!("  MI = Maintainability Index   {} ≥65 (good)   {} ≥40 (moderate)   {} <40 (poor)",
        "■".green(), "■".yellow(), "■".red());
    println!("  SOLID: [S]ingle-Responsibility  [O]pen-Closed  [L]iskov  [I]nterface-Seg  [D]ependency-Inv");
    println!();
}

// ─── main ─────────────────────────────────────────────────────────────────────

fn main() {
    let cli: Cli = Cli::parse();

    let clang = Clang::new().expect("Failed to initialise libclang");
    let index: Index<'_> = Index::new(&clang, false, true);

    let files = collect_cpp_files(&cli.paths, cli.recursive);
    if files.is_empty() {
        eprintln!("{}", "No C++ source files found.".red());
        std::process::exit(1);
    }

    let flags: Vec<&str> = cli.compiler_flags.iter()
        .flat_map(|f| f.split_whitespace())
        .collect();
    let mut reports: Vec<FileReport> = Vec::new();

    let solid_cfg = if cli.no_solid {
        None
    } else {
        Some(SolidConfig {
            srp_max_public_methods:    cli.srp_max_methods,
            srp_max_field_groups:       4,
            ocp_max_type_switch_arms:   cli.ocp_max_arms,
            isp_max_interface_methods:  cli.isp_max_methods,
            dip_max_new_expressions:    cli.dip_max_new,
        })
    };

    for file in &files {
        let path_str = file.to_string_lossy().into_owned();
        let parse_result = index.parser(&path_str).arguments(&flags).skip_function_bodies(false).parse();

        // If C++ parsing crashes libclang (AstDeserialization), retry as a plain C translation
        // unit — stripping any -std=c++XX flags that conflict with -x c-header.
        let parse_result = if matches!(parse_result, Err(clang::SourceError::AstDeserialization)) {
            let mut c_flags: Vec<&str> = flags.iter().copied()
                .filter(|f| !f.starts_with("-std=c++"))
                .collect();
            c_flags.extend_from_slice(&["-x", "c-header"]);
            index.parser(&path_str).arguments(&c_flags).skip_function_bodies(false).parse()
        } else {
            parse_result
        };

        match parse_result {
            Ok(tu) => {
                let (functions, parse_errors, solid_violations) =
                    analyse_tu(&tu, &path_str, solid_cfg.as_ref());
                reports.push(FileReport { path: path_str, functions, parse_errors, solid_violations });
            }
            Err(e) => {
                eprintln!("{} {} – {:?}", "Failed to parse".red(), path_str, e);
                reports.push(FileReport {
                    path: path_str, functions: vec![],
                    parse_errors: vec![format!("{:?}", e)],
                    solid_violations: vec![],
                });
            }
        }
    }

    if cli.json {
        println!("{}", serde_json::to_string_pretty(&reports).unwrap());
    } else {
        print_report(&reports, cli.warn_complexity, cli.warn_depth);
    }

    let has_violations = reports.iter().any(|r| r.functions.iter().any(|f| {
        f.cyclomatic_complexity >= cli.warn_complexity || f.max_nesting_depth >= cli.warn_depth
    }));
    if has_violations { std::process::exit(1); }
}