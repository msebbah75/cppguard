use std::path::Path;
use std::process::Command;
use anyhow::{bail, Result};
use serde::Deserialize;

// ─── Clang AST JSON structures ────────────────────────────────────────────────

/// A single node in the Clang AST (`-ast-dump=json`).
/// Only the fields we need are declared; serde ignores the rest.
#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AstNode {
    /// Node kind, e.g. "FunctionDecl", "IfStmt", "ForStmt", …
    pub kind: Option<String>,

    /// Human-readable name for declarations
    pub name: Option<String>,

    /// Mangled name (useful for disambiguation)
    pub mangled_name: Option<String>,

    /// Source location
    pub loc: Option<Location>,

    /// Source range
    pub range: Option<Range>,

    /// Whether this decl is "used" (present on some nodes)
    pub is_used: Option<bool>,

    /// Whether the node is implicit (compiler-generated)
    pub is_implicit: Option<bool>,

    /// Return / value type string
    #[serde(rename = "type")]
    pub ty: Option<TypeRef>,

    /// Nested / child nodes
    #[serde(default)]
    pub inner: Vec<AstNode>,

    /// Operator text for BinaryOperator / UnaryOperator
    pub opcode: Option<String>,

    /// Value category (lvalue / rvalue / xvalue)
    pub value_category: Option<String>,

    /// Referenced declaration info (DeclRefExpr)
    pub referenced_decl: Option<Box<AstNode>>,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Location {
    pub file: Option<String>,
    pub line: Option<u32>,
    pub col: Option<u32>,
    /// When a node is in an #included file the AST uses an `expansionLoc`
    pub expansion_loc: Option<Box<Location>>,
    /// Sometimes the location is spelled out in a spelling loc
    pub spelling_loc: Option<Box<Location>>,
}

impl Location {
    /// Return the most concrete file path available.
    pub fn resolved_file(&self) -> Option<&str> {
        self.file
            .as_deref()
            .or_else(|| self.expansion_loc.as_ref()?.file.as_deref())
            .or_else(|| self.spelling_loc.as_ref()?.file.as_deref())
    }

    pub fn resolved_line(&self) -> Option<u32> {
        self.line
            .or_else(|| self.expansion_loc.as_ref()?.line)
            .or_else(|| self.spelling_loc.as_ref()?.line)
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct Range {
    pub begin: Option<Location>,
    pub end: Option<Location>,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TypeRef {
    pub qual_type: Option<String>,
    pub desugared_qual_type: Option<String>,
}

// ─── AST dump helper ──────────────────────────────────────────────────────────

/// Run `clang -Xclang -ast-dump=json -fsyntax-only` on `file` and return the
/// raw JSON string.
pub fn dump_ast(file: &Path, extra_args: &[String]) -> Result<String> {
    let mut cmd = Command::new("clang++");
    cmd.args([
        "-Xclang",
        "-ast-dump=json",
        "-fsyntax-only",
        // Suppress the "note: …" lines that clang emits to stderr for some
        // diagnostics so they don't contaminate the JSON on stdout.
        "-w",
    ]);
    cmd.arg(file);
    cmd.args(extra_args);

    let output = cmd.output()?;

    if !output.status.success() && output.stdout.is_empty() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!(
            "clang++ exited with status {} and produced no AST.\nStderr:\n{}",
            output.status,
            stderr
        );
    }

    // clang writes the JSON to stdout; warnings / errors go to stderr.
    Ok(String::from_utf8(output.stdout)?)
}

// ─── AST traversal helpers ────────────────────────────────────────────────────

impl AstNode {
    /// Depth-first iterator over all descendants (including `self`).
    pub fn walk(&self) -> impl Iterator<Item = &AstNode> {
        WalkIter {
            stack: vec![self],
        }
    }

    pub fn kind_str(&self) -> &str {
        self.kind.as_deref().unwrap_or("")
    }
}

struct WalkIter<'a> {
    stack: Vec<&'a AstNode>,
}

impl<'a> Iterator for WalkIter<'a> {
    type Item = &'a AstNode;

    fn next(&mut self) -> Option<Self::Item> {
        let node = self.stack.pop()?;
        // Push children in reverse order so we visit them left-to-right.
        for child in node.inner.iter().rev() {
            self.stack.push(child);
        }
        Some(node)
    }
}
