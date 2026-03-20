/// solid.rs — SOLID principle heuristics via the Clang AST
///
/// Each check returns a list of `SolidViolation` findings.
/// All checks are heuristic — static analysis can approximate but not prove
/// principle adherence.
///
/// Principles checked
/// ──────────────────
/// S — Single Responsibility : class has too many public methods / field groups
/// O — Open/Closed           : large switch/if-else chains dispatching on type tags
/// L — Liskov Substitution   : overriding method widens preconditions (coarse check)
/// I — Interface Segregation : base class (interface) has too many pure-virtual methods
/// D — Dependency Inversion  : method creates concrete objects with `new` instead of
///                             depending on abstractions

use clang::{Entity, EntityKind};
use serde::{Deserialize, Serialize};

// ─── Public types ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum SolidPrinciple {
    SingleResponsibility,
    OpenClosed,
    LiskovSubstitution,
    InterfaceSegregation,
    DependencyInversion,
}

impl SolidPrinciple {
    pub fn letter(&self) -> char {
        match self {
            Self::SingleResponsibility => 'S',
            Self::OpenClosed           => 'O',
            Self::LiskovSubstitution   => 'L',
            Self::InterfaceSegregation => 'I',
            Self::DependencyInversion  => 'D',
        }
    }
    pub fn name(&self) -> &'static str {
        match self {
            Self::SingleResponsibility => "Single Responsibility",
            Self::OpenClosed           => "Open/Closed",
            Self::LiskovSubstitution   => "Liskov Substitution",
            Self::InterfaceSegregation => "Interface Segregation",
            Self::DependencyInversion  => "Dependency Inversion",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SolidViolation {
    pub principle: SolidPrinciple,
    pub entity:    String, // class or function name
    pub file:      String,
    pub line:      u32,
    pub detail:    String,
    pub severity:  Severity,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Info,
    Warning,
    Error,
}

impl std::fmt::Display for Severity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Info    => write!(f, "info"),
            Self::Warning => write!(f, "warning"),
            Self::Error   => write!(f, "error"),
        }
    }
}

// ─── Thresholds (all tuneable) ────────────────────────────────────────────────

pub struct SolidConfig {
    /// S: warn when a class has more than this many public methods
    pub srp_max_public_methods: usize,
    /// S: warn when a class has more than this many distinct field-name prefixes
    pub srp_max_field_groups: usize,
    /// O: warn when a switch/if-else inside a method has more than this many arms
    pub ocp_max_type_switch_arms: u32,
    /// I: warn when an interface (all-pure-virtual class) has more than N methods
    pub isp_max_interface_methods: usize,
    /// D: warn when a function/method contains more than this many `new` expressions
    pub dip_max_new_expressions: u32,
}

impl Default for SolidConfig {
    fn default() -> Self {
        Self {
            srp_max_public_methods:    10,
            srp_max_field_groups:       4,
            ocp_max_type_switch_arms:   5,
            isp_max_interface_methods: 7,
            dip_max_new_expressions:   2,
        }
    }
}

// ─── Top-level entry point ────────────────────────────────────────────────────

/// Walk the translation-unit entity and collect all SOLID violations found
/// in entities that belong to `source_path`.
pub fn check_solid(
    tu_entity: &Entity,
    source_path: &str,
    cfg: &SolidConfig,
) -> Vec<SolidViolation> {
    let mut violations = Vec::new();
    walk(tu_entity, source_path, cfg, &mut violations);
    violations.sort_by(|a, b| a.line.cmp(&b.line));
    violations
}

fn walk(
    entity: &Entity,
    source_path: &str,
    cfg: &SolidConfig,
    out: &mut Vec<SolidViolation>,
) {
    match entity.get_kind() {
        EntityKind::ClassDecl
        | EntityKind::StructDecl
        | EntityKind::ClassTemplate
        | EntityKind::ClassTemplatePartialSpecialization => {
            if in_source(entity, source_path) {
                check_srp(entity, cfg, out);
                check_isp(entity, cfg, out);
            }
        }
        EntityKind::FunctionDecl
        | EntityKind::Method
        | EntityKind::Constructor
        | EntityKind::Destructor
        | EntityKind::FunctionTemplate => {
            if entity.is_definition() && in_source(entity, source_path) {
                check_ocp(entity, cfg, out);
                check_lsp(entity, out);
                check_dip(entity, cfg, out);
            }
        }
        _ => {}
    }

    for child in entity.get_children() {
        walk(&child, source_path, cfg, out);
    }
}

// ─── S — Single Responsibility Principle ─────────────────────────────────────
//
// Heuristics:
//   1. Class with > N public methods likely has more than one responsibility.
//   2. Field names that share many distinct prefixes suggest multiple concerns
//      being bundled together (e.g. `audio_*`, `video_*`, `network_*`).

fn check_srp(entity: &Entity, cfg: &SolidConfig, out: &mut Vec<SolidViolation>) {
    let class_name = entity.get_name().unwrap_or_else(|| "<anonymous>".into());

    let methods: Vec<Entity> = entity
        .get_children()
        .into_iter()
        .filter(|c| {
            matches!(c.get_kind(), EntityKind::Method | EntityKind::Constructor | EntityKind::Destructor)
                && c.is_definition()
        })
        .collect();

    let public_methods = methods
        .iter()
        .filter(|m| m.get_accessibility()
            .map_or(false, |a| a == clang::Accessibility::Public))
        .count();

    if public_methods > cfg.srp_max_public_methods {
        out.push(violation(
            SolidPrinciple::SingleResponsibility,
            &class_name,
            entity,
            Severity::Warning,
            format!(
                "class '{}' has {} public methods (threshold: {}). \
                 Consider splitting it into smaller, focused classes.",
                class_name, public_methods, cfg.srp_max_public_methods
            ),
        ));
    }

    // Field-prefix heuristic
    let field_prefixes: std::collections::HashSet<String> = entity
        .get_children()
        .into_iter()
        .filter(|c| c.get_kind() == EntityKind::FieldDecl)
        .filter_map(|c| c.get_name())
        .filter_map(|name| {
            // Take the first segment before '_' if present
            let seg = name.split('_').next()?.to_string();
            if seg.len() >= 3 && seg != name { Some(seg) } else { None }
        })
        .collect();

    if field_prefixes.len() > cfg.srp_max_field_groups {
        out.push(violation(
            SolidPrinciple::SingleResponsibility,
            &class_name,
            entity,
            Severity::Info,
            format!(
                "class '{}' fields span {} distinct name-prefixes ({:?}), \
                 suggesting multiple concerns.",
                class_name,
                field_prefixes.len(),
                field_prefixes.into_iter().collect::<Vec<_>>()
            ),
        ));
    }
}

// ─── O — Open/Closed Principle ────────────────────────────────────────────────
//
// Heuristic: a function/method that contains a large switch or if/else-if chain
// that dispatches on an enum/int tag is a classic OCP violation — adding a new
// "type" requires modifying this function rather than extending via polymorphism.

fn check_ocp(entity: &Entity, cfg: &SolidConfig, out: &mut Vec<SolidViolation>) {
    let fn_name = entity.get_display_name().unwrap_or_else(|| "<anonymous>".into());

    // Count switch arms and top-level if/else-if chains
    let (switch_arms, if_chain_len) = count_dispatch_arms(entity);

    if switch_arms > cfg.ocp_max_type_switch_arms {
        out.push(violation(
            SolidPrinciple::OpenClosed,
            &fn_name,
            entity,
            Severity::Warning,
            format!(
                "'{}' contains a switch with {} arms. \
                 Large type-dispatch switches often violate OCP — \
                 consider virtual dispatch or std::variant + std::visit.",
                fn_name, switch_arms
            ),
        ));
    }

    if if_chain_len > cfg.ocp_max_type_switch_arms {
        out.push(violation(
            SolidPrinciple::OpenClosed,
            &fn_name,
            entity,
            Severity::Info,
            format!(
                "'{}' contains an if/else-if chain with {} branches. \
                 This may be a type-dispatch pattern that resists extension.",
                fn_name, if_chain_len
            ),
        ));
    }
}

/// Returns (max_switch_case_count, max_if_else_chain_length) inside `entity`.
fn count_dispatch_arms(entity: &Entity) -> (u32, u32) {
    let mut max_switch = 0u32;
    let mut max_if     = 0u32;

    fn recurse(node: &Entity, max_switch: &mut u32, max_if: &mut u32) {
        match node.get_kind() {
            EntityKind::SwitchStmt => {
                let arms = count_case_stmts(node);
                if arms > *max_switch { *max_switch = arms; }
            }
            EntityKind::IfStmt => {
                let chain = if_else_chain_length(node);
                if chain > *max_if { *max_if = chain; }
            }
            _ => {}
        }
        for child in node.get_children() {
            recurse(&child, max_switch, max_if);
        }
    }

    for child in entity.get_children() {
        recurse(&child, &mut max_switch, &mut max_if);
    }
    (max_switch, max_if)
}

fn count_case_stmts(switch: &Entity) -> u32 {
    switch
        .get_children()
        .iter()
        .filter(|c| matches!(c.get_kind(), EntityKind::CaseStmt | EntityKind::DefaultStmt))
        .count() as u32
}

/// Counts the length of an if / else-if / … chain.
fn if_else_chain_length(if_node: &Entity) -> u32 {
    let mut len = 1u32;
    // The else branch is the last child of IfStmt
    let children = if_node.get_children();
    if let Some(last) = children.last() {
        if last.get_kind() == EntityKind::IfStmt {
            len += if_else_chain_length(last);
        }
    }
    len
}

// ─── L — Liskov Substitution Principle ───────────────────────────────────────
//
// Heuristic: an overriding method that throws unconditionally or returns a
// fixed sentinel value on every path is suspicious — it may be strengthening
// preconditions or weakening postconditions.
// We flag overrides that contain only a single statement (often `throw` or
// `return constant`), plus overrides that call `std::terminate` / `abort`.

fn check_lsp(entity: &Entity, out: &mut Vec<SolidViolation>) {
    // Only look at overriding methods
    if entity.get_kind() != EntityKind::Method { return; }

    let is_override = entity
        .get_children()
        .iter()
        .any(|c| c.get_kind() == EntityKind::OverrideAttr);

    // Alternative: check if the method overrides something via CXXMethodDecl flags.
    // The `clang` crate exposes `is_virtual` and we can detect if there is a
    // parent with a matching pure-virtual declaration.
    let is_virtual_override = entity.is_virtual_base()
        || is_override
        || has_virtual_parent_method(entity);

    if !is_virtual_override { return; }

    let fn_name = entity.get_display_name().unwrap_or_else(|| "<anonymous>".into());
    let stmts   = direct_statement_count(entity);

    // Flag body that is a single throw
    if contains_unconditional_throw(entity) {
        out.push(violation(
            SolidPrinciple::LiskovSubstitution,
            &fn_name,
            entity,
            Severity::Warning,
            format!(
                "overriding method '{}' throws unconditionally. \
                 This may violate LSP by strengthening preconditions \
                 (callers expecting the base contract will break).",
                fn_name
            ),
        ));
        return;
    }

    // Flag suspiciously tiny override bodies (1 statement ≠ return / assertion)
    if stmts == 1 {
        out.push(violation(
            SolidPrinciple::LiskovSubstitution,
            &fn_name,
            entity,
            Severity::Info,
            format!(
                "overriding method '{}' has a single-statement body. \
                 Verify it honours the base-class contract (LSP).",
                fn_name
            ),
        ));
    }
}

fn has_virtual_parent_method(method: &Entity) -> bool {
    // Walk the semantic parent (the class) and find a base with a matching
    // virtual method of the same name.
    let name = match method.get_name() {
        Some(n) => n,
        None    => return false,
    };
    let parent = match method.get_semantic_parent() {
        Some(p) => p,
        None    => return false,
    };
    parent.get_children().iter().any(|c| {
        c.get_kind() == EntityKind::BaseSpecifier
            && c.get_children().iter().any(|base_class| {
                base_class.get_children().iter().any(|m| {
                    m.get_kind() == EntityKind::Method
                        && m.is_pure_virtual_method()
                        && m.get_name().as_deref() == Some(&name)
                })
            })
    })
}

fn contains_unconditional_throw(entity: &Entity) -> bool {
    // Simple: the only reachable statement at the top level of the body is a throw.
    let children = entity.get_children();
    // Body is typically the last child (CompoundStmt)
    if let Some(body) = children.iter().find(|c| c.get_kind() == EntityKind::CompoundStmt) {
        let stmts = body.get_children();
        if stmts.len() == 1 && stmts[0].get_kind() == EntityKind::ThrowExpr {
            return true;
        }
        // Also catch: { throw ...; } with a single stmt
        if stmts.iter().all(|s| s.get_kind() == EntityKind::ThrowExpr) && !stmts.is_empty() {
            return true;
        }
    }
    false
}

fn direct_statement_count(entity: &Entity) -> usize {
    entity
        .get_children()
        .iter()
        .find(|c| c.get_kind() == EntityKind::CompoundStmt)
        .map(|body| body.get_children().len())
        .unwrap_or(0)
}

// ─── I — Interface Segregation Principle ─────────────────────────────────────
//
// Heuristic: a class that is "interface-like" (all public, all pure virtual,
// no data members) with too many methods forces implementors to depend on
// methods they don't use.

fn check_isp(entity: &Entity, cfg: &SolidConfig, out: &mut Vec<SolidViolation>) {
    let class_name = entity.get_name().unwrap_or_else(|| "<anonymous>".into());

    let children = entity.get_children();

    // Only consider "interface" classes: no data fields, all methods pure virtual
    let has_fields = children
        .iter()
        .any(|c| c.get_kind() == EntityKind::FieldDecl);
    if has_fields { return; }

    let methods: Vec<&Entity> = children
        .iter()
        .filter(|c| matches!(c.get_kind(), EntityKind::Method | EntityKind::Destructor))
        .collect();

    if methods.is_empty() { return; }

    let all_pure = methods
        .iter()
        .filter(|m| m.get_kind() == EntityKind::Method)
        .all(|m| m.is_pure_virtual_method());

    if !all_pure { return; }

    let method_count = methods
        .iter()
        .filter(|m| m.get_kind() == EntityKind::Method)
        .count();

    if method_count > cfg.isp_max_interface_methods {
        out.push(violation(
            SolidPrinciple::InterfaceSegregation,
            &class_name,
            entity,
            Severity::Warning,
            format!(
                "interface '{}' declares {} pure-virtual methods (threshold: {}). \
                 Clients are forced to depend on methods they may not need. \
                 Consider splitting into smaller, role-specific interfaces.",
                class_name, method_count, cfg.isp_max_interface_methods
            ),
        ));
    }
}

// ─── D — Dependency Inversion Principle ──────────────────────────────────────
//
// Heuristic: a function that directly constructs concrete objects via `new`
// (CXXNewExpr) is coupling itself to a concrete implementation.  High-level
// modules should depend on abstractions injected from outside.

fn check_dip(entity: &Entity, cfg: &SolidConfig, out: &mut Vec<SolidViolation>) {
    let fn_name = entity.get_display_name().unwrap_or_else(|| "<anonymous>".into());

    let new_count = count_new_expressions(entity);
    if new_count > cfg.dip_max_new_expressions {
        out.push(violation(
            SolidPrinciple::DependencyInversion,
            &fn_name,
            entity,
            Severity::Warning,
            format!(
                "'{}' contains {} `new` expressions (threshold: {}). \
                 Prefer constructor injection or a factory/DI container \
                 to avoid hard-coding concrete dependencies.",
                fn_name, new_count, cfg.dip_max_new_expressions
            ),
        ));
    }
}

fn count_new_expressions(entity: &Entity) -> u32 {
    let mut count = 0u32;
    fn recurse(node: &Entity, count: &mut u32) {
        if node.get_kind() == EntityKind::NewExpr { *count += 1; }
        for child in node.get_children() { recurse(&child, count); }
    }
    for child in entity.get_children() { recurse(&child, &mut count); }
    count
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

fn in_source(entity: &Entity, source_path: &str) -> bool {
    entity
        .get_location()
        .and_then(|l| l.get_file_location().file)
        .map_or(false, |f| f.get_path().to_string_lossy() == source_path)
}

fn violation(
    principle: SolidPrinciple,
    entity_name: &str,
    entity: &Entity,
    severity: Severity,
    detail: String,
) -> SolidViolation {
    let loc = entity
        .get_location()
        .map(|l| l.get_file_location())
        .map(|fl| (
            fl.file.map(|f| f.get_path().to_string_lossy().into_owned()).unwrap_or_default(),
            fl.line,
        ))
        .unwrap_or_default();

    SolidViolation {
        principle,
        entity: entity_name.to_string(),
        file:   loc.0,
        line:   loc.1,
        detail,
        severity,
    }
}
