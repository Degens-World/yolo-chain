# ErgoScript Rust Compiler — Handoff

**Goal:** Complete the `ergoscript-compiler` crate so it can compile real ErgoScript contracts to ErgoTree bytecode in pure Rust. No Ergo node needed.

**Starting point:** The crate compiles `val x = 1 + 2 * HEIGHT`. Everything else fails.

**End state:** The crate compiles contracts like the 7 governance contracts in `06-governance/contracts/`, which use every major ErgoScript feature: lambdas, method calls, collections, tuples, if/else, boolean logic, built-in functions.

---

## Setup

```bash
git clone https://github.com/ergoplatform/sigma-rust
cd sigma-rust
# The compiler is at:
ls ergoscript-compiler/src/
# Run existing tests:
cargo test -p ergoscript-compiler
```

The crate is part of the sigma-rust monorepo. It depends on `ergotree-ir` (same repo) for the target IR types. Everything builds with `cargo`.

## The pipeline (already wired)

```
source text
  → Lexer (logos)         → tokens
  → Parser (rowan/pratt)  → CST (concrete syntax tree)
  → AST                   → typed AST nodes
  → HIR                   → high-level IR
  → Binder                → resolved names
  → Type Inference        → typed HIR
  → MIR Lowering          → ergotree_ir::mir::Expr
  → ErgoTree              → serializable bytecode
```

The orchestrator (`compiler.rs`) already chains all stages. Each stage has its own error type. The public API is two functions:

```rust
pub fn compile(source: &str, env: ScriptEnv) -> Result<ErgoTree, CompileError>
pub fn compile_expr(source: &str, env: ScriptEnv) -> Result<Expr, CompileError>
```

## How to work

**Strategy: one language feature at a time, tested end-to-end.** Don't try to implement everything at once. Pick a feature, thread it through all pipeline stages, write a test that compiles a minimal contract using that feature, verify the output matches what the Ergo node produces.

**Test pattern:**
```rust
#[test]
fn compile_feature_x() {
    let tree = compile("{ sigmaProp(HEIGHT > 0) }", ScriptEnv::new()).unwrap();
    let bytes = tree.sigma_serialize_bytes().unwrap();
    // Compare against known-good hex from Ergo node compilation
    assert_eq!(hex::encode(bytes), "10010400d191a37300");
}
```

Get the expected hex by compiling the same source via an Ergo node:
```bash
curl -s -X POST "http://localhost:9053/script/p2sAddress" \
  -H "Content-Type: application/json" -H "api_key: $KEY" \
  -d '{"source": "{ sigmaProp(HEIGHT > 0) }", "treeVersion": 0}'
# → address
curl -s "http://localhost:9053/script/addressToTree/$ADDR"
# → {"tree": "10010400d191a37300"}
```

**Suggested order of features** (each builds on the previous):

### Session 1: Lexer + boolean/comparison operators

Add tokens: `.` `=>` `:` `,` `==` `!=` `>=` `<=` `>` `<` `||` `!` `[` `]` `true` `false` `if` `else` plus string literals.

Wire `==`, `!=`, `>`, `<`, `>=`, `<=`, `&&`, `||` into the Pratt parser with correct precedence (comparison binds tighter than boolean). `&&` token already exists in the lexer but isn't in the parser.

Add `BoolLiteral` to AST/HIR. Thread through to MIR.

**Test target:** `{ sigmaProp(HEIGHT > 0 && HEIGHT < 100) }`

### Session 2: Block expressions + val bindings in blocks

Parse `{ val x = expr; val y = expr; expr }` — multiple statements in a block, last expression is the return value.

This is critical because every ErgoScript contract is a block.

**Test target:** `{ val x: Long = 5L; sigmaProp(x > 0L) }`

### Session 3: Method calls + field access + SELF/INPUTS/OUTPUTS

Parse `expr.ident` (field access) and `expr.ident(args)` (method call). Add postfix parsing after the current `lhs()` function.

Add globals: `SELF`, `INPUTS`, `OUTPUTS`, `CONTEXT` to the binder. These map to `ergotree_ir::mir::global_vars::GlobalVars`.

Add method resolution for `Box` type: `.value`, `.tokens`, `.propositionBytes`, `.creationInfo`, `.id`, `.R4[T]` through `.R9[T]`.

**Test target:** `{ sigmaProp(SELF.value > 0L) }`

### Session 4: Index access + collection operations + tuple access

Parse `expr(index)` — ErgoScript uses parens for indexing. Parse `expr._1`, `expr._2` for tuple field access.

Add `.size`, `.filter`, `.exists`, `.forall`, `.fold`, `.map`, `.slice` method resolution on `Coll[T]`.

**Test target:** `{ sigmaProp(SELF.tokens.size > 0 && SELF.tokens(0)._2 == 1L) }`

### Session 5: Lambda expressions

Parse `{ (param: Type) => body }` inside method call arguments. This is the big one — it unlocks `.filter`, `.forall`, `.fold`, etc.

Map to `ergotree_ir::mir::func_value::FuncValue` in MIR lowering.

**Test target:**
```scala
{
  val found = INPUTS.filter { (b: Box) =>
    b.tokens.size > 0 && b.tokens(0)._1 == SELF.tokens(0)._1
  }
  sigmaProp(found.size == 1)
}
```

### Session 6: Built-in functions + fromBase16 + string literals

Add `sigmaProp()`, `blake2b256()`, `fromBase16()`, `atLeast()`, `proveDlog()`, `min()`, `max()`, `longToByteArray()`, `allOf()`, `anyOf()`.

Add string literal lexing/parsing for `fromBase16("...")`.

**Test target:** One of the actual governance contracts (e.g., `vault.es`).

### Sessions 7-8: If/else + type annotations + remaining gaps

Parse `if (cond) expr else expr`. Add type annotation syntax `: Type` on val bindings.

Run all 7 governance contracts through the compiler. Fix whatever fails. The contracts exercise every major feature — they're the acceptance test suite.

**Done when:** `compile(vault_es_source, env)` produces the same ErgoTree bytes as the Ergo node.

## Key files to modify (in order of likely changes)

| File | What to change |
|------|----------------|
| `src/lexer/token_kind.rs` | Add ~30 token variants |
| `src/syntax.rs` | Add matching SyntaxKind variants |
| `src/parser/grammar/expr.rs` | Add postfix parsing (dot, call, index), lambda, if/else, boolean ops |
| `src/parser/grammar/stmt.rs` | Add type annotations on val bindings |
| `src/ast.rs` | Add ~15 AST node types |
| `src/hir.rs` | Add HIR node types + lowering from AST |
| `src/binder.rs` | Add global variable resolution (SELF, INPUTS, etc.), built-in function lookup |
| `src/type_infer.rs` | Add type rules for all operators and method return types |
| `src/mir/lower.rs` | Map new HIR nodes to `ergotree_ir::mir::*` types |

## Reference materials

- **ErgoScript language spec:** `sigmastate-interpreter/docs/LangSpec.md` on GitHub
- **ergotree_ir types:** `~/.cargo/registry/src/*/ergotree-ir-0.28.0/src/mir/` — every IR node the compiler needs to produce
- **Scala compiler reference:** `sigmastate-interpreter/sc/src/main/scala/sigmastate/lang/` — the Scala implementation that handles everything
- **Test contracts:** `yolo-chain/06-governance/contracts/*.es` — 7 real contracts exercising all features
- **Known-good ErgoTree hex:** Compile any `.es` file via Ergo node and compare output

## What NOT to change

- **Don't touch `ergotree-ir` or `ergotree-interpreter`.** They're complete and production. The compiler's job is to produce `ergotree_ir::mir::Expr` values — the downstream handles everything from there.
- **Don't redesign the pipeline.** The 8-stage architecture is correct. Just fill in the stages.
- **Don't add optimization passes yet.** Get correctness first. The Scala compiler doesn't optimize either — it relies on the interpreter's JIT.

## Validation approach

For each new feature, compile a minimal test contract with both:
1. The Rust compiler (`compile()`)
2. The Ergo node (`/script/p2sAddress` + `/script/addressToTree`)

The ErgoTree hex must match byte-for-byte. If it doesn't, the Scala compiler's output is authoritative — the Rust compiler has a bug.

The 7 contracts in `06-governance/contracts/` plus the emission contract in `01-emission-tests/emission.es` are the final acceptance test. When all 8 compile correctly, the project is done.
