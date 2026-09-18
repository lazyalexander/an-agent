// Control flow for the Lean math probe. One continuation per call.
// Host injects: step (0-based int), last_ok (bool), last_diag (string).
// Returns #{ op: "lean", id, source, expect } or #{ op: "halt" }.
// The script never talks to Lean or the model; the host admits lean_check.

let problems = [
    #{
        id: "add_rfl",
        source: "example : 1 + 1 = 2 := rfl",
        expect: true
    },
    #{
        id: "mul_rfl",
        source: "example : 2 * 3 = 6 := rfl",
        expect: true
    },
    #{
        id: "add_comm",
        source: "example : ∀ n m : Nat, n + m = m + n := Nat.add_comm",
        expect: true
    },
    #{
        id: "add_assoc",
        source: "example : ∀ a b c : Nat, (a + b) + c = a + (b + c) := Nat.add_assoc",
        expect: true
    },
    #{
        id: "add_zero",
        source: "example : ∀ n : Nat, n + 0 = n := Nat.add_zero",
        expect: true
    },
    #{
        id: "zero_lt_one",
        source: "example : 0 < 1 := Nat.zero_lt_one",
        expect: true
    },
    #{
        id: "two_ne_five",
        source: "example : 2 + 2 ≠ 5 := by decide",
        expect: true
    },
    #{
        id: "false_rfl",
        source: "example : 1 + 1 = 3 := rfl",
        expect: false
    }
];

if params.step >= problems.len() {
    #{ op: "halt", solved: params.step }
} else {
    let p = problems[params.step];
    #{ op: "lean", id: p.id, source: p.source, expect: p.expect }
}
