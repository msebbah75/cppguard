# cppguard

A Rust CLI tool that analyses **C++ code complexity** by walking the Clang AST
via the [`clang`](https://crates.io/crates/clang) crate (libclang bindings).

## Metrics computed per function

| Metric | Description |
|---|---|
| **Cyclomatic Complexity (CC)** | McCabe's metric: 1 + decision-point count (`if`, `for`, `while`, `case`, `catch`, `?:`, …) |
| **Max Nesting Depth** | Deepest level of control-flow nesting |
| **AST Node Count** | Total nodes in the function sub-tree |
| **Parameter Count** | Number of formal parameters |
| **Local Variable Count** | `VarDecl` nodes inside the function body |
| **Return Count** | Number of `return` statements |
| **Halstead Volume / Difficulty / Effort** | Operator/operand counts from the AST |
| **Maintainability Index (MI)** | Microsoft variant: `max(0, (171 − 5.2·ln(V) − 0.23·CC − 16.2·ln(LoC))·100/171)` |

## Prerequisites

You need **libclang** installed on your system:

```bash
# Ubuntu / Debian
sudo apt install libclang-dev

# Arch Linux
sudo pacman -S clang

# macOS (Homebrew)
brew install llvm
export LIBCLANG_PATH=$(brew --prefix llvm)/lib
```

## Build

```bash
cargo build --release
```

## Usage

```
cpp_guard [OPTIONS] <PATHS>...

Arguments:
  <PATHS>...  C++ source files or directories to analyse

Options:
  -f, --flag <FLAG>              Extra compiler flags forwarded to Clang
      --json                     Output results as JSON
      --warn-complexity <N>      CC threshold for warnings [default: 10]
      --warn-depth <N>           Nesting depth threshold [default: 5]
  -r, --recursive                Recursively scan directories
  -h, --help                     Print help
  -V, --version                  Print version
```

### Examples

```bash
# Analyse a single file with C++17
./target/release/cpp_guard src/main.cpp -f -std=c++17

# Analyse a whole project, warn at CC≥15
./target/release/cpp_guard -r ./src -f -std=c++17 -f -I./include --warn-complexity 15

# Emit JSON (pipe to jq, etc.)
./target/release/cpp_guard main.cpp --json | jq '.[] | .functions[] | {name, cc: .cyclomatic_complexity}'
```

## Complexity thresholds (CC)

| Range | Rating | Colour |
|---|---|---|
| 1–5 | Low – easy to test | 🟢 green |
| 6–10 | Moderate | 🟡 yellow |
| 11–20 | High – refactor candidate | 🔴 red |
| > 20 | Very high – must refactor | 🔴 bold red |

## Exit codes

| Code | Meaning |
|---|---|
| 0 | All functions below thresholds |
| 1 | At least one function exceeded `--warn-complexity` or `--warn-depth` |

## Architecture

```
main.rs
 ├─ Cli          – clap argument parsing
 ├─ FunctionMetrics / HalsteadRaw – data model (serde-serialisable)
 ├─ FileReport   – per-file aggregation
 ├─ visit_entity – recursive AST walker (cyclomatic, depth, Halstead)
 ├─ build_metrics – assembles FunctionMetrics for one function entity
 ├─ analyse_tu   – drives visit over a TranslationUnit
 ├─ collect_cpp_files – file discovery (walkdir)
 └─ print_report – coloured table output
```
