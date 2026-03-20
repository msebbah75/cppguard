use std::collections::HashSet;
use std::path::{Path, PathBuf};
use crate::ast::AstNode;

// ─── Public data types ────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct FileMetrics {
    pub path: PathBuf,
    pub functions: Vec<FunctionMetrics>,
}

#[derive(Debug, Clone)]
pub struct FunctionMetrics {
    pub name: String,
    pub qualified_name: String,
    pub file: String,
    pub line: u32,

    // --- Classic metrics ---
    /// McCabe Cyclomatic Complexity (M = E − N + 2P)
    /// Here we use the decision-point counting approximation: CC = #branches + 1
    pub cyclomatic: u32,

    /// Cognitive Complexity (Sonar-source definition)
    pub cognitive: u32,

    /// Maximum nesting depth
    pub max_nesting: u32,

    /// Number of parameters
    pub param_count: u32,

    // --- Halstead metrics ---
    pub halstead: Halstead,

    // --- Size metrics ---
    pub loc: u32,           // lines of code (begin..end from range)
    pub statement_count: u32,
}

#[derive(Debug, Clone, Default)]
pub struct Halstead {
    pub n1: u32, // total operators
    pub n2: u32, // total operands
    pub nu1: u32, // unique operators
    pub nu2: u32, // unique operands

    // Derived
    pub vocabulary: u32,  // η  = η1 + η2
    pub length: u32,       // N  = N1 + N2
    pub volume: f64,       // V  = N * log2(η)
    pub difficulty: f64,   // D  = (η1/2) * (N2/η2)
    pub effort: f64,       // E  = D * V
    pub time_to_program: f64, // T  = E / 18  (seconds)
    pub delivered_bugs: f64,  // B  = V / 3000
}

// ─── Entry point ─────────────────────────────────────────────────────────────

pub fn analyze(root: &AstNode, source_file: &Path) -> FileMetrics {
    let source_str = source_file.to_string_lossy().to_string();

    // Collect top-level function / method declarations that originate from
    // the file being analyzed (skip system headers, etc.)
    let mut functions = Vec::new();
    collect_functions(root, &source_str, &mut functions);

    FileMetrics {
        path: source_file.to_path_buf(),
        functions,
    }
}

// ─── Function collection ──────────────────────────────────────────────────────

fn collect_functions(node: &AstNode, source_file: &str, out: &mut Vec<FunctionMetrics>) {
    match node.kind_str() {
        "FunctionDecl" | "CXXMethodDecl" | "CXXConstructorDecl"
        | "CXXDestructorDecl" | "CXXConversionDecl" => {
            // Skip declarations without a body (pure prototypes).
            let has_body = node.inner.iter().any(|c| {
                matches!(
                    c.kind_str(),
                    "CompoundStmt" | "TryStmt"
                )
            });
            if !has_body {
                // Still recurse in case of nested lambdas declared at namespace scope.
                for child in &node.inner {
                    collect_functions(child, source_file, out);
                }
                return;
            }

            // Filter to the file under analysis.
            let loc_file = node
                .loc
                .as_ref()
                .and_then(|l| l.resolved_file())
                .unwrap_or("");

            // Accept the function if its location matches (substring match handles
            // absolute vs relative path discrepancies).
            if !loc_file.is_empty()
                && !source_file.is_empty()
                && !loc_file.contains(source_file)
                && !source_file.contains(loc_file)
            {
                for child in &node.inner {
                    collect_functions(child, source_file, out);
                }
                return;
            }

            let line = node.loc.as_ref().and_then(|l| l.resolved_line()).unwrap_or(0);

            let name = node.name.clone().unwrap_or_else(|| "<anonymous>".into());

            // Count parameters (ParmVarDecl direct children)
            let param_count = node
                .inner
                .iter()
                .filter(|c| c.kind_str() == "ParmVarDecl")
                .count() as u32;

            // LOC from range
            let loc = compute_loc(node);

            // Body node
            let body = node.inner.iter().find(|c| {
                matches!(c.kind_str(), "CompoundStmt" | "TryStmt")
            });

            let cyclomatic = body.map_or(1, |b| cyclomatic_complexity(b));
            let (cognitive, _) = body.map_or((0, 0), |b| cognitive_complexity(b, 0));
            let max_nesting = body.map_or(0, max_nesting_depth);
            let statement_count = body.map_or(0, count_statements);
            let halstead = body.map_or_default(compute_halstead);

            out.push(FunctionMetrics {
                name: name.clone(),
                qualified_name: node
                    .mangled_name
                    .clone()
                    .unwrap_or_else(|| name.clone()),
                file: loc_file.to_string(),
                line,
                cyclomatic,
                cognitive,
                max_nesting,
                param_count,
                halstead,
                loc,
                statement_count,
            });

            // Recurse to pick up lambdas / nested functions.
            for child in &node.inner {
                collect_functions(child, source_file, out);
            }
        }
        _ => {
            for child in &node.inner {
                collect_functions(child, source_file, out);
            }
        }
    }
}

// ─── Cyclomatic Complexity ────────────────────────────────────────────────────
//
// CC = number of linearly independent paths = (number of binary decision points) + 1
// We count each: if / else-if, for, while, do-while, case, catch, ternary (?:),
// logical &&  / ||  (short-circuit operators add branches).

fn cyclomatic_complexity(node: &AstNode) -> u32 {
    let mut count: u32 = 1; // base path
    for n in node.walk() {
        match n.kind_str() {
            "IfStmt" => count += 1,
            "ForStmt" | "CXXForRangeStmt" | "WhileStmt" | "DoStmt" => count += 1,
            "SwitchCase" | "CaseStmt" => count += 1,
            "CXXCatchStmt" => count += 1,
            "ConditionalOperator" => count += 1, // ternary
            "BinaryOperator" => {
                if matches!(n.opcode.as_deref(), Some("&&") | Some("||")) {
                    count += 1;
                }
            }
            _ => {}
        }
    }
    count
}

// ─── Cognitive Complexity ─────────────────────────────────────────────────────
//
// Based on the Sonar-source white-paper (simplified):
//  +1 for each structural increment (if, for, while, do, switch, catch, …)
//  +1 for each nesting level beyond the first structural increment
//  +1 for each sequence of logical operators (&&, ||, !)
//  +1 for recursion (not implemented here – would need fn name context)
//
// Returns (score, nesting_level_consumed).

fn cognitive_complexity(node: &AstNode, nesting: u32) -> (u32, u32) {
    let mut total: u32 = 0;

    match node.kind_str() {
        "IfStmt" => {
            total += 1 + nesting;
            // then branch
            if let Some(then) = node.inner.get(1) {
                let (s, _) = cognitive_complexity(then, nesting + 1);
                total += s;
            }
            // else branch
            if let Some(else_node) = node.inner.get(2) {
                // else-if does NOT add extra nesting level in Sonar model
                let extra = if else_node.kind_str() == "IfStmt" { 1 } else { 1 + nesting };
                total += extra;
                let (s, _) = cognitive_complexity(else_node, nesting + 1);
                total += s;
            }
        }
        "ForStmt" | "CXXForRangeStmt" | "WhileStmt" | "DoStmt" => {
            total += 1 + nesting;
            for child in &node.inner {
                let (s, _) = cognitive_complexity(child, nesting + 1);
                total += s;
            }
        }
        "SwitchStmt" => {
            total += 1 + nesting;
            for child in &node.inner {
                let (s, _) = cognitive_complexity(child, nesting + 1);
                total += s;
            }
        }
        "CXXTryStmt" | "CXXCatchStmt" => {
            total += 1 + nesting;
            for child in &node.inner {
                let (s, _) = cognitive_complexity(child, nesting + 1);
                total += s;
            }
        }
        "ConditionalOperator" => {
            total += 1 + nesting;
            for child in &node.inner {
                let (s, _) = cognitive_complexity(child, nesting + 1);
                total += s;
            }
        }
        "LambdaExpr" => {
            // Lambda body is a new structural scope – increment nesting.
            for child in &node.inner {
                let (s, _) = cognitive_complexity(child, nesting + 1);
                total += s;
            }
        }
        "BinaryOperator" => {
            // Consecutive same logical operators count as one increment.
            let op = node.opcode.as_deref().unwrap_or("");
            if op == "&&" || op == "||" {
                total += count_logical_sequences(node);
            } else {
                for child in &node.inner {
                    let (s, _) = cognitive_complexity(child, nesting);
                    total += s;
                }
            }
        }
        _ => {
            for child in &node.inner {
                let (s, _) = cognitive_complexity(child, nesting);
                total += s;
            }
        }
    }

    (total, nesting)
}

/// Count the number of distinct logical operator sequences (&&/|| chains).
/// Each run of the same operator within a BinaryOperator subtree counts as +1.
fn count_logical_sequences(node: &AstNode) -> u32 {
    fn walk_logical(n: &AstNode, last_op: &mut Option<String>, count: &mut u32) {
        if n.kind_str() == "BinaryOperator" {
            let op = n.opcode.as_deref().unwrap_or("");
            if op == "&&" || op == "||" {
                if last_op.as_deref() != Some(op) {
                    *count += 1;
                    *last_op = Some(op.to_string());
                }
                for child in &n.inner {
                    walk_logical(child, last_op, count);
                }
                return;
            }
        }
        *last_op = None;
        for child in &n.inner {
            walk_logical(child, last_op, count);
        }
    }

    let mut last_op: Option<String> = None;
    let mut count = 0u32;
    walk_logical(node, &mut last_op, &mut count);
    count
}

// ─── Max Nesting Depth ────────────────────────────────────────────────────────

fn max_nesting_depth(node: &AstNode) -> u32 {
    fn recurse(node: &AstNode, depth: u32) -> u32 {
        let is_scope = matches!(
            node.kind_str(),
            "IfStmt"
                | "ForStmt"
                | "CXXForRangeStmt"
                | "WhileStmt"
                | "DoStmt"
                | "SwitchStmt"
                | "CXXTryStmt"
                | "CXXCatchStmt"
                | "LambdaExpr"
                | "CompoundStmt"
        );
        let next_depth = if is_scope { depth + 1 } else { depth };
        let mut max = next_depth;
        for child in &node.inner {
            max = max.max(recurse(child, next_depth));
        }
        max
    }
    recurse(node, 0)
}

// ─── Statement Count ──────────────────────────────────────────────────────────

fn count_statements(node: &AstNode) -> u32 {
    node.walk()
        .filter(|n| {
            n.kind_str().ends_with("Stmt") && n.kind_str() != "CompoundStmt"
        })
        .count() as u32
}

// ─── Halstead Metrics ─────────────────────────────────────────────────────────

/// Operators: BinaryOperator opcodes, UnaryOperator opcodes, keywords-as-operators
/// Operands: DeclRefExpr names, IntegerLiteral, FloatingLiteral, StringLiteral, …

fn compute_halstead(node: &AstNode) -> Halstead {
    let mut operators: Vec<String> = Vec::new();
    let mut operands: Vec<String> = Vec::new();

    for n in node.walk() {
        match n.kind_str() {
            "BinaryOperator" | "CompoundAssignOperator" => {
                if let Some(op) = &n.opcode {
                    operators.push(op.clone());
                }
            }
            "UnaryOperator" => {
                if let Some(op) = &n.opcode {
                    operators.push(format!("unary_{}", op));
                }
            }
            "CallExpr" => {
                operators.push("()".into());
            }
            "MemberExpr" => {
                operators.push("->/.".into());
            }
            "ArraySubscriptExpr" => {
                operators.push("[]".into());
            }
            "ConditionalOperator" => {
                operators.push("?:".into());
            }
            "CXXNewExpr" => {
                operators.push("new".into());
            }
            "CXXDeleteExpr" => {
                operators.push("delete".into());
            }
            "CXXThrowExpr" => {
                operators.push("throw".into());
            }
            "ReturnStmt" => {
                operators.push("return".into());
            }
            "DeclRefExpr" => {
                if let Some(name) = &n.name {
                    operands.push(name.clone());
                }
            }
            "IntegerLiteral" | "FloatingLiteral" | "CharacterLiteral" => {
                // We use the type as a stand-in for the literal value since
                // the AST JSON does not always include the literal value itself.
                let val = n
                    .ty
                    .as_ref()
                    .and_then(|t| t.qual_type.as_deref())
                    .unwrap_or("literal")
                    .to_string();
                operands.push(val);
            }
            "StringLiteral" | "CXXBoolLiteralExpr" | "CXXNullPtrLiteralExpr" => {
                operands.push(n.kind_str().to_string());
            }
            _ => {}
        }
    }

    let n1 = operators.len() as u32;
    let n2 = operands.len() as u32;
    let nu1 = operators.iter().collect::<HashSet<_>>().len() as u32;
    let nu2 = operands.iter().collect::<HashSet<_>>().len() as u32;

    let vocabulary = nu1 + nu2;
    let length = n1 + n2;

    let volume = if vocabulary > 1 {
        (length as f64) * (vocabulary as f64).log2()
    } else {
        0.0
    };

    let difficulty = if nu2 > 0 {
        (nu1 as f64 / 2.0) * (n2 as f64 / nu2 as f64)
    } else {
        0.0
    };

    let effort = difficulty * volume;
    let time_to_program = effort / 18.0;
    let delivered_bugs = volume / 3000.0;

    Halstead {
        n1,
        n2,
        nu1,
        nu2,
        vocabulary,
        length,
        volume,
        difficulty,
        effort,
        time_to_program,
        delivered_bugs,
    }
}

// ─── LOC ──────────────────────────────────────────────────────────────────────

fn compute_loc(node: &AstNode) -> u32 {
    if let Some(range) = &node.range {
        let begin = range.begin.as_ref().and_then(|l| l.resolved_line());
        let end = range.end.as_ref().and_then(|l| l.resolved_line());
        if let (Some(b), Some(e)) = (begin, end) {
            return e.saturating_sub(b) + 1;
        }
    }
    0
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

trait OptionNodeExt {
    fn map_or_default<F>(&self, f: F) -> Halstead
    where
        F: Fn(&AstNode) -> Halstead;
}

impl OptionNodeExt for Option<&AstNode> {
    fn map_or_default<F>(&self, f: F) -> Halstead
    where
        F: Fn(&AstNode) -> Halstead,
    {
        match self {
            Some(n) => f(n),
            None => Halstead::default(),
        }
    }
}
