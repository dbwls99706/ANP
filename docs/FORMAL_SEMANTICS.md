# Formal Semantics of ANP Graph Execution

**Abstract.** We present a formal specification of the execution semantics for ANP (AI Native Programming), a system in which programs are represented as executable directed acyclic graphs rather than textual source code. We define the abstract syntax of graphs, a structural type system with subtyping, well-formedness conditions, and small-step operational semantics. We state and sketch proofs for key properties including type safety, termination, determinism, and parallel correctness.

---

## 1. Abstract Syntax

**Definition 1.1 (Graph).** A graph is a five-tuple G = (N, E, κ, τ, C) where:

- N is a finite set of **nodes**,
- E ⊆ N × P × N × P is a set of **directed edges** with port annotations,
- κ : N → Kind is the **kind function**,
- τ : N → T is the **type assignment**,
- C : N → Config is the **configuration function**.

**Definition 1.2 (Auxiliary Domains).**

```
Port         P  = String
Config          = JSON Value
Kind            = { Source, Processor, Sink, Service }
```

**Definition 1.3 (Port Signatures).** Each node type declares input and output port signatures:

```
inputs  : N → 𝒫(P × T)
outputs : N → 𝒫(P × T)
```

For convenience, we write `type(n, p)` to denote the type associated with port p on node n, resolved from either `inputs(n)` or `outputs(n)` as appropriate.

**Definition 1.4 (Predecessors and Successors).**

```
pred(n)  = { n' ∈ N | ∃ p₁, p₂ : (n', p₁, n, p₂) ∈ E }
succ(n)  = { n' ∈ N | ∃ p₁, p₂ : (n, p₁, n', p₂) ∈ E }
```

---

## 2. Type System

**Definition 2.1 (Types).** The set of types T is defined inductively:

```
T  ::=  Any
     |  String  |  Number  |  Boolean  |  Null
     |  Object(F?)
     |  Array(T?)
     |  Html
     |  Binary
     |  Union(T₁, …, Tₖ)

where F = { f₁ : T₁, …, fₘ : Tₘ }   (field mapping, m ≥ 0)
```

The option annotation `F?` (resp. `T?`) distinguishes typed containers (`Some(F)`) from untyped ones (`None`), enabling gradual typing.

**Definition 2.2 (Subtyping).** The subtyping relation `<:` is the smallest reflexive and transitive relation satisfying:

| Rule | Premise | Conclusion |
|------|---------|------------|
| S-TOP | — | T <: Any |
| S-REFL | — | T <: T |
| S-HTML | — | Html <: String |
| S-OBJ-WIDTH | — | Object(Some(F)) <: Object(None) |
| S-OBJ-DEPTH | ∀(k, T₂) ∈ F₂ : k ∈ dom(F₁) ∧ F₁(k) <: T₂ | Object(Some(F₁)) <: Object(Some(F₂)) |
| S-ARR-WIDTH | — | Array(Some(T₁)) <: Array(None) |
| S-ARR-DEPTH | T₁ <: T₂ | Array(Some(T₁)) <: Array(Some(T₂)) |
| S-UNION-L | Tᵢ <: T₂ for some i | Union(T₁, …, Tₖ) <: T₂ |
| S-UNION-R | T₁ <: Tᵢ for some i | T₁ <: Union(T₁, …, Tₖ) |

**Lemma 2.3 (Subtyping is a Partial Order).** The relation `<:` is reflexive (S-REFL), transitive (by induction on derivations), and antisymmetric up to structural equivalence. `Any` is the unique top element.

---

## 3. Well-Formedness Rules

**Definition 3.1 (Well-Formed Graph).** A graph G = (N, E, κ, τ, C) is **well-formed**, written ⊢ G **wf**, if and only if all of the following hold:

**WF-1 (Acyclicity).** The directed graph (N, E) is acyclic. Formally, there exists a topological ordering ≺ on N such that for all (n₁, p₁, n₂, p₂) ∈ E, we have n₁ ≺ n₂.

**WF-2 (Kind Constraints).** Edges respect the behavioral contracts of node kinds:

```
∀ (n₁, p₁, n₂, p₂) ∈ E :
    κ(n₁) ≠ Sink          — sinks produce no output
  ∧ κ(n₂) ≠ Source        — sources accept no input
  ∧ κ(n₂) ≠ Service       — services are event-loop roots, not edge targets
```

**WF-3 (Port Existence).** Every edge references declared ports:

```
∀ (n₁, p₁, n₂, p₂) ∈ E :
    p₁ ∈ { p | (p, _) ∈ outputs(n₁) }
  ∧ p₂ ∈ { p | (p, _) ∈ inputs(n₂) }
```

**WF-4 (Type Safety).** Data flowing along each edge is type-compatible:

```
∀ (n₁, p₁, n₂, p₂) ∈ E :  type(n₁, p₁) <: type(n₂, p₂)
```

**WF-5 (Unique Node Identifiers).** All node identifiers are distinct:

```
∀ nᵢ, nⱼ ∈ N :  id(nᵢ) = id(nⱼ)  ⟹  nᵢ = nⱼ
```

---

## 4. Operational Semantics

### 4.1 Execution State

**Definition 4.1 (State).** An execution state is a triple σ = (S, D, R) where:

```
S : N → Status        Status  = { Pending, Running, Complete, Failed }
D : (N × P) → Value   (partial function: data available on ports)
R : N → Readiness     Readiness = { Ready, Blocked, Done }
```

**Definition 4.2 (Initial State).** For a well-formed graph G, the initial state σ₀ is:

```
S₀(n) = Pending           ∀ n ∈ N
D₀     = ∅                (no data on any port)
R₀(n) = Ready             if pred(n) = ∅
R₀(n) = Blocked           otherwise
```

**Definition 4.3 (Readiness).** A node n is Ready in state σ when:

```
Ready(σ, n)  ⟺  S(n) = Pending  ∧  ∀ (n', p', n, p) ∈ E : S(n') = Complete
```

**Definition 4.4 (Terminal State).** A state σ is terminal when:

```
∀ n ∈ N :  S(n) ∈ { Complete, Failed }
```

### 4.2 Transition Rules

We define a small-step transition relation σ → σ'.

**E-STEP (Single Node Execution):**

```
    S(n) = Pending     Ready(σ, n)
    v = eval(τ(n), C(n), { D(n', p') | (n', p', n, p) ∈ E })
    v ≠ ⊥
  ──────────────────────────────────────────────────────────────
    σ → σ[S(n) ↦ Complete, D(n, p) ↦ v(p) for p ∈ outputs(n)]
```

**E-FAIL (Node Failure):**

```
    S(n) = Pending     Ready(σ, n)
    eval(τ(n), C(n), …) = ⊥
  ──────────────────────────────────────────────────────
    σ → σ[S(n) ↦ Failed]
```

**E-PAR (Parallel Execution):**

```
    W = { n ∈ N | Ready(σ, n) }     |W| ≥ 2
    ∀ nᵢ, nⱼ ∈ W, i ≠ j :  nᵢ ∉ pred*(nⱼ) ∧ nⱼ ∉ pred*(nᵢ)
  ──────────────────────────────────────────────────────────────
    σ → σ' where σ' applies E-STEP (or E-FAIL) to all n ∈ W
```

### 4.3 Stage-Based Scheduling

**Definition 4.5 (Execution Stage).** A stage is a maximal set of mutually independent ready nodes. The runtime partitions execution into stages via topological sort:

```
stage : N → ℕ
stage(n) = 0                           if pred(n) = ∅
stage(n) = 1 + max{ stage(n') | n' ∈ pred(n) }   otherwise
```

All nodes at the same stage are independent and may execute concurrently.

---

## 5. Verification Properties

**Theorem 5.1 (Progress).** If G is well-formed and σ is a non-terminal reachable state, then there exists at least one node n such that Ready(σ, n).

*Proof sketch.* Since G is a DAG (WF-1), every non-terminal state has at least one Pending node. Among all Pending nodes, select those with minimal topological rank. All predecessors of such nodes are Complete (by minimality), so Ready(σ, n) holds. □

**Theorem 5.2 (Type Preservation).** If G satisfies WF-4 and node n executes successfully, producing value v on output port p, then typeof(v) <: type(n, p).

*Proof sketch.* By induction on the structure of eval. Each node implementation is required to satisfy a contract: given inputs conforming to declared input types, it produces outputs conforming to declared output types. WF-4 ensures input conformance across edges; the node contract ensures output conformance. □

**Theorem 5.3 (Termination).** If G is well-formed (WF-1) and contains no Service nodes, then execution terminates in at most |N| steps (where one step processes one stage).

*Proof sketch.* Each transition moves at least one node from Pending to Complete or Failed. Since |N| is finite and no node transitions back to Pending, the execution completes in at most |N| transitions. The number of stages is bounded by the length of the longest path in the DAG, which is at most |N| − 1. □

**Theorem 5.4 (Determinism).** If G is well-formed and every node's eval function is deterministic, then for any initial input, the execution produces a unique terminal state.

*Proof sketch.* By confluence. Consider two execution traces that differ only in the order of independent node evaluations. Since independent nodes share no data dependencies (no edge connects them), reordering produces identical port data. By induction on stages, the terminal state is unique. □

**Theorem 5.5 (Parallel Safety).** Nodes within the same execution stage share no mutable state. Therefore, concurrent execution of a stage preserves sequential semantics.

*Proof sketch.* Two nodes nᵢ, nⱼ in the same stage satisfy nᵢ ∉ pred*(nⱼ) and nⱼ ∉ pred*(nᵢ). Communication occurs exclusively through port data D, which is written only by a node's own execution and read only after the producing node is Complete. Thus no write–write or read–write conflicts arise. □

---

## 6. Optimization Correctness

**Definition 6.1 (Semantic Equivalence).** Two graphs G₁ and G₂ are semantically equivalent, written G₁ ≡ G₂, if for all inputs I and terminal states σ₁, σ₂ obtained by executing G₁(I) and G₂(I) respectively, the observable outputs are identical.

**Definition 6.2 (Correct Optimization).** An optimization pass O : Graph → Graph is correct if ∀ G : G ≡ O(G).

**Proposition 6.3 (Dead Node Elimination).** Let G' = G \ { n ∈ N | n is unreachable from any Sink }. Then G ≡ G'.

*Justification.* Removed nodes contribute to no observable output (no path to any Sink). By definition of semantic equivalence, eliminating them preserves behavior. □

**Proposition 6.4 (Transform Fusion).** Let n₁, n₂ be adjacent Processor nodes of type `transform` connected by the default port, with mappings M₁ and M₂ respectively. Let n' be a single transform node with mapping M₂ ∘ M₁. Then replacing (n₁, n₂) with n' preserves semantics, provided M₂ depends only on outputs of M₁.

*Justification.* Sequential application of M₁ then M₂ produces the same result as the composed mapping, by the associativity of function composition over the JSON value domain. □

---

## 7. Compilation Correctness

**Definition 7.1 (Execution Plan).** The compiler `compile(G)` produces a plan P = (stages, routes) where:

```
stages : list of list of N      (topologically ordered stage groups)
routes : N × P → N × P          (routing table derived from E)
```

**Theorem 7.1 (Compilation Correctness).** For all well-formed graphs G:

```
exec(compile(G)) ≡ exec(G)
```

*Proof sketch.* The compilation proceeds in three phases:

1. **Topological sort** produces a linear order consistent with E. By WF-1, such an order exists. This preserves the invariant that every node is scheduled after all its predecessors.

2. **Stage assignment** groups nodes by their topological depth (Definition 4.5). This preserves the property that all inputs to a node are available before it executes, since all predecessors reside in earlier stages.

3. **Route table construction** maps each edge (n₁, p₁, n₂, p₂) ∈ E to an entry in the routing table. The mapping is bijective with respect to E, so the data flow graph is isomorphic to the original edge set.

Since each phase preserves the dependency structure and data flow of G, the compiled plan is behaviorally equivalent. □

---

## 8. Notation Summary

| Symbol | Meaning |
|--------|---------|
| G = (N, E, κ, τ, C) | Graph with nodes, edges, kinds, types, configs |
| κ(n) | Kind of node n |
| τ(n) | Type of node n |
| T₁ <: T₂ | T₁ is a subtype of T₂ |
| σ = (S, D, R) | Execution state |
| σ → σ' | Single-step transition |
| pred(n), succ(n) | Predecessor / successor nodes |
| pred*(n) | Transitive closure of pred |
| eval(τ, C, inputs) | Node evaluation function |
| ⊥ | Error / failure value |
| ≡ | Semantic equivalence |

---

*This specification accompanies ANP v0.7. The formalization covers the core DAG execution model; Service nodes (long-running event loops) and the GraphOp mutation API operate outside the pure functional semantics described here and are treated as extensions.*
