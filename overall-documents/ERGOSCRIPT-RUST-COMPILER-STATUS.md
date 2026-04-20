# ErgoScript Rust Compiler — Status & Completion Roadmap

## What is it?

The `ergoscript-compiler` crate (part of [ergoplatform/sigma-rust](https://github.com/ergoplatform/sigma-rust)) is an attempt to compile ErgoScript source code to ErgoTree bytecode entirely in Rust — no Scala toolchain, no running node required. If completed, any Rust, WASM, or browser-based application could compile ErgoScript contracts in-process.

## Where it lives

- **Repository:** [github.com/ergoplatform/sigma-rust](https://github.com/ergoplatform/sigma-rust)
- **Crate path:** `ergoscript-compiler/` within the sigma-rust monorepo
- **Published version:** 0.24.0 on crates.io
- **Also:** Standalone repo at [github.com/ergoplatform/ergoscript-compiler](https://github.com/ergoplatform/ergoscript-compiler) (appears to be the earlier standalone version before integration into sigma-rust)

## Current state: a solid skeleton that compiles `1 + 2`

The compiler has a well-designed 8-stage pipeline. The architecture is sound — Rowan CST, Pratt parsing, HIR/MIR lowering, clean error propagation. But the implementation stopped at basic arithmetic expressions. It was built following the [eldiro language tutorial](https://arzg.github.io/lang/), and development appears to have paused once the tutorial's scope was exceeded.

### What works today (end-to-end through all 8 stages)

- Integer and Long literals: `42`, `100L`
- Binary arithmetic: `1 + 2 * 3`
- Unary negation: `-5`
- Parenthesized expressions: `(1 + 2) * 3`
- Val bindings: `val x = 1 + 2`
- The `HEIGHT` global variable
- Full pipeline: source → lex → parse → AST → HIR → bind → type-infer → MIR → ErgoTree

### What fails (everything else ErgoScript needs)

This is not a "95% done, just needs polish" situation. The compiler cannot handle any real contract because it's missing the fundamentals of the language:

```scala
// NONE of this compiles:
SELF.value                          // no dot access
SELF.tokens(0)._1                   // no method calls, no tuple access
INPUTS.filter { (b: Box) => ... }   // no lambdas, no blocks
out.propositionBytes == SELF.propositionBytes  // no == operator
deltaVaultYolo + deltaReserveVYolo == 0L       // no ==
sigmaProp(true && false)            // no sigmaProp, no boolean literals
if (x > 0) a else b                // no if/else, no >
fromBase16("aa")                    // no string literals, no built-in functions
blake2b256(bytes)                   // no built-in functions
val x: Long = 5L                    // no type annotations
```

## Pipeline stage-by-stage

| Stage | Files | Lines | Status | What works | What's missing |
|-------|-------|-------|--------|------------|----------------|
| **Lexer** | 2 | ~130 | Complete for current scope | 18 token types | ~30 missing tokens (`.` `=>` `:` `==` `!=` `>=` `<=` `>` `<` `\|\|` `!` `,` `[` `]` `_` `true` `false` `if` `else` string literals, etc.) |
| **Parser** | 9 | ~650 | Arithmetic only | Pratt parsing, precedence, parens, val defs | Block expressions, method calls, lambdas, if/else, type annotations, function calls, collection syntax |
| **AST** | 1 | 204 | Skeletal | 3 node types: Ident, BinaryExpr, Literal | Method call, field access, block, lambda, if/else, function def, type annotation, collection — roughly 15 missing node types |
| **HIR** | 2 | 228 | Partial | Binary ops, literals, HEIGHT global | SELF, INPUTS, OUTPUTS, method calls, collections, tuples, lambdas, let bindings, blocks |
| **Binder** | 1 | 62 | Skeleton | `HEIGHT` → GlobalVars::Height | Custom variable binding is `todo!()`. No scope analysis, no function resolution |
| **Type inference** | 1 | 108 | Minimal | `+` operator only | `-`, `*`, `/` are `todo!()`. No method return types, no generics, no boolean ops, no constraint solving |
| **MIR lowering** | 2 | 211 | Transforms what HIR produces | Binary ops → ergotree_ir, literals → Constant, Height passthrough | Everything the HIR doesn't produce yet. No optimization passes. |
| **Compiler** | 1 | 127 | Complete orchestrator | Wires all stages, error propagation, ErgoTree output | Nothing — this stage is done, it just needs the earlier stages to produce more |

**Total:** ~1,945 lines across 24 files. Roughly 50% of the pipeline scaffolding exists but only ~10% of the actual ErgoScript language is covered.

## What needs to be built

### Phase A: Lexer completion (~1 session)

Add ~30 token types. This is mechanical — extend the `logos` regex patterns:

```rust
// Missing tokens to add:
#[token(".")] Dot,
#[token("=>")] Arrow,
#[token(":")] Colon,
#[token(",")] Comma,
#[token("==")] EqEq,
#[token("!=")] NotEq,
#[token(">=")] GtEq,
#[token("<=")] LtEq,
#[token(">")] Gt,
#[token("<")] Lt,
#[token("||")] Or,
#[token("!")] Not,
#[token("[")] LBracket,
#[token("]")] RBracket,
#[token("_")] Underscore,
#[token("true")] TrueKw,
#[token("false")] FalseKw,
#[token("if")] IfKw,
#[token("else")] ElseKw,
#[regex(r#""[^"]*""#)] StringLiteral,
// ... plus ErgoScript keywords: sigmaProp, SELF, INPUTS, OUTPUTS, etc.
```

**Effort:** Small. The lexer is clean and well-structured. Adding tokens is additive — nothing breaks.

### Phase B: Parser grammar (~2-3 sessions)

This is the main work. Extend the Pratt parser with ~15 new grammar rules:

1. **Boolean/comparison operators** in `expr_binding_power` — add `==`, `!=`, `>=`, `<=`, `>`, `<`, `&&`, `||` with correct binding powers. The `&&` token already exists but isn't wired in.

2. **Block expressions** — `{ stmt; stmt; expr }`. Parse `LBrace`, loop on `stmt()`, parse final `expr`, expect `RBrace`.

3. **Method calls** — `expr.ident(args)`. After parsing an ident or expression, check for `.` and parse as postfix. This is the critical one — every contract uses `SELF.value`, `INPUTS.filter(...)`, `box.tokens(0)`.

4. **Lambda expressions** — `{ (param: Type, ...) => expr }`. When a `{` is followed by `(`, parse parameter list with type annotations, expect `=>`, parse body.

5. **If/else** — `if (expr) expr else expr`. Standard conditional.

6. **Index/call syntax** — `expr(args)`. ErgoScript uses parens for indexing (`tokens(0)`) and function calls (`sigmaProp(x)`).

7. **Type annotations** — `: Type` on val bindings and lambda params.

8. **Tuple access** — `expr._1`, `expr._2`. Postfix after dot.

9. **Unary not** — `!expr`. Prefix operator.

10. **Collection literals** — `Coll(...)`, `fromBase16("...")`.

**Effort:** Moderate. The Pratt parser framework handles precedence cleanly. Each new rule is ~20-50 lines. The pattern is well-established in the existing code.

### Phase C: AST + HIR expansion (~2-3 sessions)

For each new parser rule, add corresponding AST node types and HIR lowering:

- `MethodCallExpr`, `FieldAccessExpr`, `BlockExpr`, `LambdaExpr`, `IfElseExpr`, `IndexExpr`, `BoolLiteral`, `StringLiteral`, `CollectionExpr`, `TupleAccessExpr`, `UnaryNotExpr`, `TypeAnnotation`

Each AST node → HIR kind mapping is straightforward. The `hir::rewrite` mechanism already supports recursive tree walking.

### Phase D: Binder + type inference (~2-3 sessions)

- **Binder:** Resolve all ErgoScript globals: `SELF`, `INPUTS`, `OUTPUTS`, `CONTEXT`, `HEIGHT`, `dataInputs`. Map built-in functions: `sigmaProp`, `blake2b256`, `fromBase16`, `atLeast`, `proveDlog`, `min`, `max`. The `ScriptEnv` already supports custom variable injection.

- **Type inference:** Extend beyond `+` to cover all operators and method return types. ErgoScript has a small, fixed type system (`SInt`, `SLong`, `SByte`, `SBoolean`, `SBox`, `SColl[T]`, `STuple`, `SSigmaProp`, `SGroupElement`). No generics inference needed — types are explicit in lambdas and inferable from context.

### Phase E: MIR lowering completion (~1-2 sessions)

The MIR layer is already well-connected to `ergotree_ir`. For each new HIR node, add a lowering case that produces the corresponding `ergotree_ir::mir::Expr` variant. The `ergotree_ir` crate already defines all the IR nodes — `Filter`, `ForAll`, `Exists`, `Fold`, `Map`, `FuncValue`, `SelectField`, `MethodCall`, etc. The MIR lowering is mostly a mapping exercise.

## Estimated effort

| Phase | Sessions | What it unlocks |
|-------|----------|-----------------|
| A: Lexer | 1 | Tokenizes real contracts |
| B: Parser | 2-3 | Parses real contracts into syntax trees |
| C: AST + HIR | 2-3 | Lowers parsed trees into typed IR |
| D: Binder + types | 2-3 | Resolves names, assigns types |
| E: MIR lowering | 1-2 | Produces ErgoTree bytecode |
| **Total** | **8-12 sessions** | **Compiles real contracts in pure Rust** |

A "session" here means a focused Claude Code conversation with file access to the sigma-rust repo. The work is incremental — each phase produces testable output and doesn't break previous phases.

## Why this is tractable

1. **The architecture is already right.** The pipeline design (lex → parse → AST → HIR → bind → type → MIR → ErgoTree) is exactly what a compiler needs. Nobody has to redesign anything.

2. **The downstream is complete.** `ergotree_ir` has every IR node ErgoScript needs. The MIR lowering just needs to produce the right nodes — they're all defined and documented.

3. **ErgoScript is small.** It's not Scala. There are ~15 expression types, ~10 built-in types, ~20 built-in functions, no classes, no imports, no generics (beyond `Coll[T]`). The entire language spec fits in [one document](https://github.com/ergoplatform/sigmastate-interpreter/blob/develop/docs/LangSpec.md).

4. **Tests already exist.** The Scala `sigmastate-interpreter` has comprehensive test suites. Every ErgoScript feature has reference behavior to test against.

5. **Rust is the right language for this.** The lexer uses `logos` (fast, well-maintained). The parser uses `rowan` (the same CST library that powers rust-analyzer). Both are production-grade foundations.

## Why it hasn't been finished

No technical blocker — it appears to be a prioritization decision. The node compilation workaround (`/script/p2sAddress` + `/script/addressToTree`) works, so the Rust compiler was never critical path. The sigma-rust team focused on the interpreter (which IS production-critical for wallets, WASM bindings, and dApp backends) and left the compiler at the proof-of-concept stage.

There's no open issue tracking this. Nobody has filed a bug or feature request for completing the compiler. It's just quietly sitting at the arithmetic-expression stage, waiting for someone to pick it up.

## What completing it would mean

- **`cargo test` works offline** — no Ergo node needed for contract development
- **WASM contract compilation** — browser wallets and frontends compile ErgoScript directly
- **CI/CD without a node** — GitHub Actions, GitLab CI, any standard pipeline
- **Dynamic contract generation** — apps bake parameters into templates at runtime
- **IDE support** — LSP-based tooling with real-time compilation feedback
- **Lower barrier to entry** — Rust/TypeScript developers don't need to install Scala or run a node

The foundation is solid. The language is small. The downstream IR is complete. This is a finishable project.
