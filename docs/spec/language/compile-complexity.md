# Compile-time Complexity Constraints

Scope: Normative for v0.1 unless this file or a section states otherwise.

To preserve fast compilation, the v0.1.0 core:

1. MUST NOT require global type inference.
2. MUST NOT require HKT/typeclass-style higher-kinded abstractions.
3. SHOULD restrict compile-time metaprogramming to bounded forms.
4. Recursive-layout validation MUST use a finite declaration/impl summary or substitution-aware occurs-check, including symbolic associated-projection edges; it MUST NOT enumerate monomorphized type instances. Any summary-graph cycle pass MUST be linear in that finite graph (`O(V + E)` after type/projection normalization and summary construction).
5. Ownership checking MUST remain function-local path-sensitive dataflow over the closed v0.1 root shapes; it MUST NOT require a general linear-type constraint solver or ownership-aware trait-bound solver.
   Obligation-region identities are finite lexical block/arm identifiers, and
   each CFG edge's exited-region set is fixed during CFG construction. Checking
   those sets MUST NOT introduce region inference or lifetime solving.
6. Trait-bound checking MUST remain first-order nominal obligation resolution over declared impl heads and inline bounds. It MUST terminate through a finite normalized-obligation worklist, memoization, or an equivalent cycle-safe strategy.
   Building that environment and validating dependent bound arguments are finite two-pass declaration checks. Exact environment lookup is local assumption matching, not global proof search.
   Trait default-body checking MUST collect the finite member-signature set once,
   add only the exact symbolic current-trait obligation and its fixed
   compiler-known prerequisites, and check each body once against abstract
   `Self`. It MUST NOT expand called bodies, infer hidden requirements, accept an
   obligation cycle as proof, or re-check a default body for each impl.
   Recursive derive eligibility is the sole derive-local exception: it operates
   on the finite generated-candidate graph defined in `algebraic-data.md` and
   does not change ordinary trait proof. For `V` candidate summaries, `E`
   candidate dependency edges, and `S` normalized recursive target-type nodes,
   fixed-point pruning, SCC discovery, and the no-growth checks MUST be bounded
   by `O(V + E + S)` after requirement-summary construction. They MUST NOT
   enumerate concrete type-argument or monomorphized-instance orbits.
7. Trait solving MUST NOT perform global type inference, higher-kinded unification, negative reasoning, specialization search, shape/value solving, or ownership-aware proof search.
8. Associated-type projection MUST reuse unique trait-impl resolution. It MUST NOT introduce an independent general equality solver.
9. Associated-type normalization and cycle detection MUST memoize concrete and canonical symbolic projection obligations, with an active-stack or equivalent finite cycle check over declaration and impl-binding summaries. It MUST NOT enumerate possible concrete implementors or monomorphized type combinations to prove equality or detect cycles.
   Canonical symbolic keys include base identity, complete declaring trait reference, and associated name. Projection-origin provenance required by impl-head and overlap checks is a finite marker and MUST NOT trigger alternate normalization search.
10. Reference-free abstraction-slot validation MUST be a structural walk over the finite alias-expanded type expression, including nested generic and callable components. Implementations SHOULD memoize alias and projection summaries so the check remains linear in the visited type structure; it MUST NOT introduce lifetime inference, region solving, or post-substitution body rechecking.
    Public-API type closure MUST likewise use one finite structural walk over each resolved, alias-expanded declaration contract. Foreign package declarations require one canonical-identity membership lookup in the contract-owning module's public re-export index. That index records resolved nominal and trait targets, including identity-preserving re-export chains, but does not expand a transparent-alias target into bridge entries. The check MUST NOT retain source-alias visibility provenance, traverse nominal representation declarations, inspect downstream uses or manifests, search sibling modules for bridges, or search alternate expansions. Implementations SHOULD reuse memoized alias and projection summaries so this check remains linear in the visited contract structure.
    Local reference validation is limited to stable-place classification,
    lexical storage-containment checks, and finite referent/ownership-provenance
    propagation within one function. It MUST NOT perform exclusivity,
    overlap, last-use, region, or interprocedural lifetime analysis.
11. Coherence checking MAY compare impl headers pairwise within an indexed trait/target-head bucket, with worst-case `O(I_b^2)` comparisons for a bucket of `I_b` impls; bound combinations MUST NOT cause exponential search. Orphan locality MUST reuse the same alias-expanded nominal target head and MUST NOT inspect substitutions or search nested type arguments for impl authority. After alias expansion and normalization, structural error unions in valid impl headers are rigid with respect to impl parameters. Header validation, coherence, and obligation lookup MUST NOT perform error-union member subtraction, partitioning, permutation enumeration, or ACI/set unification.
12. Equality operator checking MUST reuse the ordinary first-order `Eq` trait obligation machinery from Rules 6-7. Tuple obligations are finite structural walks; conditional `Opt`, `Res`, `Seq`, and `Vec` implementations are ordinary nominal impl obligations. v0.1 MUST NOT add recursive or coinductive equality proof search. The derive-local candidate fixed point in Rule 6 decides whether a generated impl exists before operator checking; an operator lookup MUST NOT invoke or extend that fixed point.
13. Relational operator checking MUST reuse ordinary first-order `Ord` obligation lookup and one `Ord.cmp` dispatch. The fixed prelude prerequisite `Ord -> Eq` is one finite implication edge; it MUST NOT enable user-defined supertraits, transitive implication search, or general capability inference.
14. Hash-bound checking MUST reuse the ordinary first-order `Hash` trait obligation machinery from Rules 6-7. Tuple obligations are finite structural walks, and conditional `Opt`, `Res`, `Seq`, and `Vec` implementations are ordinary nominal impl obligations. `Hash` and `Eq` remain independent obligations; v0.1 MUST NOT infer either from the other.
15. `ReduceSchema.reduce_by` expression-local callback inference MUST inspect
    one complete invocation expression, bind the carrier element type, and make
    at most one written-order pass over the finite phase list. It MUST NOT build
    a deferred local-use graph, scan later statements, or retain provisional
    binding state.
16. Tensor dimension inference MUST use the closed procedure in
    `../stdlib/tensor.md`. Let `E` be total axis-expression nodes, `Q` the
    equation count, `V` distinct dimension variables, and `L <= 64` the maximum
    working bit length including the `Nat::MAX + 1` comparison sentinel. One
    partition candidate requires at most `V`
    assignment rounds and `O(L)` saturating evaluations per solved univariate
    equation; a conservative bound is `O(V * Q * L * E * M(L))`, where `M(L)`
    is the implementation's bounded-integer multiplication cost.
17. For operand list `j` with concrete rank `R_j`, `F_j` fixed entries, and
    `P_j > 0` pack occurrences, its candidate count is at most
    `choose(R_j - F_j + P_j - 1, P_j - 1)` when `R_j >= F_j`. The total
    cross-operand candidate bound is the product of those finite counts.
    Implementations MUST count solutions only in the saturated lattice
    `zero | one | many | binding-required`, memoize equivalent partition/solver
    states where needed, and MUST NOT enumerate tensor elements or dimension
    values.
18. Concrete dimension magnitude may affect only bounded `Nat` arithmetic and
    `O(L)` univariate search. It MUST NOT cause trial division, divisor
    enumeration, a loop proportional to the dimension value, backend execution,
    or general multivariate integer solving. Unsupported multivariate
    components terminate with `DIM_BINDING_REQUIRED`.
19. Tensor distributive/factoring proof MUST use the closed written-tree-directed
    structural relation in `tensor-model.md`. Implementations MUST memoize
    equivalent written-subtree obligations when needed and MUST NOT enumerate
    arbitrary rewrite paths, algebraically equivalent intermediate trees, or a
    fully distributed normal form whose branch structure is absent from both
    written expressions. Dimension magnitudes and tensor element counts MUST NOT
    affect proof search. Compilers MUST use the direct, left-rule, then
    right-rule priority fixed there rather than comparing all successful proofs.
    Explicit sum-branch permutation matching MAY use a polynomial deterministic
    bipartite-matching algorithm over the finite written branches; it MUST NOT
    enumerate branch permutations.
20. v0.1 `einop` validation MUST reject any `+` node with one lexical scan of
    the finite schema tree before dimension solving or lowering. A compiler MUST
    NOT expand tagged-sum branches, construct Cartesian branch products, or run
    branch-routing search for `einop`.
21. Local name resolution MUST process declarations in source order over a
    finite lexical-scope stack with one finite binding table per scope, or an
    equivalent bounded representation. Lookup work MUST be bounded by the active
    lexical ancestor chain; shadowing and same-scope collision checks MUST NOT
    require whole-program binding inference or a deferred use graph.
22. Structural pattern validation MUST use one source-order recursive walk over
    the finite pattern tree plus finite lookups in the resolved tuple, struct,
    or variant declaration. Field and binding duplicate checks SHOULD use
    per-container and per-alternative-path sets. For `P` pattern nodes and `F`
    total declared fields inspected for exact-field checks, validation MUST be
    bounded by `O(P + F)` with indexed field lookup, excluding ordinary nominal
    type normalization already required by type checking. It MUST NOT expand
    nested or-patterns into a Cartesian set of complete patterns.
23. Raw-pointer relay validation MUST remain callable-local and lexical over the
    finite source expression/control-flow graph. Implementations MAY propagate
    the exact inferred pointer type and enclosing outermost-`unsafe` region
    identity through permitted expressions and immutable locals, but MUST NOT
    perform pointer lifetime, provenance, alias, or liveness analysis, or any
    interprocedural escape analysis.
24. `Never` inhabitance analysis MUST use the finite tri-state graph defined in
    `type-system.md`. For `V` bounded normalized type/declaration nodes and `E`
    dependency edges, monotone worklist evaluation MUST be `O(V + E)` after
    graph construction: each node can advance at most from `Uninhabited` to
    `Unknown` to `Inhabited`. Re-entry into one nominal head with a changed
    argument vector MUST terminate at `Unknown`; implementations MUST NOT
    enumerate transformed generic instance orbits, introduce a general subtype
    closure, search for constructors, or perform interprocedural divergence
    proof.
