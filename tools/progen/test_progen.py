"""Tests for the program generator itself (not for RustViz):

    python3 -m unittest discover -s tools/progen

The rustc test needs the toolchain from rust-toolchain.toml and is skipped
without `rustc`; the rest run anywhere.
"""
import os
import re
import shutil
import tempfile
import time
import unittest

import gen
import progen


class Determinism(unittest.TestCase):
    def test_same_seed_same_program(self):
        for i in range(40):
            self.assertEqual(gen.generate("d", i).render(), gen.generate("d", i).render())

    def test_seeds_and_indices_differ(self):
        a = [gen.generate("a", i).render() for i in range(20)]
        b = [gen.generate("b", i).render() for i in range(20)]
        self.assertGreater(len(set(a)), 15)
        self.assertNotEqual(a, b)

    def test_shape(self):
        for i in range(60):
            p = gen.generate("shape", i)
            text = p.render()
            self.assertEqual(text.count("{"), text.count("}"), text)
            self.assertTrue(re.search(r"^fn (main|run)\(", text, re.M), text)
            # 3-8 statements at the top of the body (plus the first lets)
            self.assertTrue(1 <= len(p.body.stmts) <= 11, text)

    def test_grammar_coverage(self):
        """Over a few hundred programs every family of templates shows up."""
        tags = set()
        for i in range(1500):
            tags.update(gen.generate("cov", i).tags())
        for t in ["let_string_move", "read", "call_move", "call_generic_move", "call_mut", "call_ref",
                  "call_generic_mut_ref_move", "let_ref_call_ret_ref", "let_ref_call_mut_ret_shared",
                  "let_mut_ref_call_ret_mut", "let_mut_ref_method_ret_mut", "method_mut_self", "shadow_borrow", "call_ret_struct", "call_ret_tuple",
                  "call_ret_option", "method_mut", "method_iter_mut", "method_last_mut", "assign",
                  "field_write", "index_write", "deref_write", "destructure_assign_swap",
                  "destructure_assign_tuple", "destructure_assign_struct", "try", "break", "continue",
                  "early_return", "if", "else_if", "match", "match_guard", "or_pattern", "if_let",
                  "let_else", "let_chain", "labeled_block", "loop", "while", "for", "while_let",
                  "struct_literal", "tuple_literal", "array_literal", "pattern_ref", "pattern_ref_mut",
                  "slice_pattern", "closure_borrow", "closure_borrow_mut", "closure_move_once",
                  "let_struct_ref_field", "struct_update_move", "labeled_loop"]:
            self.assertIn(t, tags)


@unittest.skipUnless(shutil.which("rustc"), "needs rustc")
class Compiles(unittest.TestCase):
    def test_most_programs_compile(self):
        work = tempfile.mkdtemp()
        n, ok, errs = 40, 0, []
        for i in range(n):
            good, why = progen.compiles(gen.generate("ci", i).render(), work)
            ok += good
            if not good:
                errs.append(why)
        self.assertGreaterEqual(ok, n * 0.8, errs)

    def test_rejects_borrow_errors(self):
        work = tempfile.mkdtemp()
        bad = 'fn main() {\n    let s = String::new();\n    let t = s;\n    println!("{}", s);\n}\n'
        self.assertEqual(progen.compiles(bad, work), (False, "E0382"))


def fake_checker(fails):
    """A Checker that doesn't run the plugin: a candidate "compiles" when
    its braces balance and `fails(text)` says which (check, sig) it has."""
    class C:
        def evaluate(self, texts, types):
            return [fails(t) if t.count("{") == t.count("}") else None for t in texts]
    return C()


class Shrinking(unittest.TestCase):
    def find(self, pred):
        for i in range(500):
            p = gen.generate("shrink", i)
            if pred(p.render()):
                return p
        self.fail("no program matches")

    def test_shrink_keeps_the_failure_and_gets_smaller(self):
        target = ("drop-without-value", "S:dot:owner_go_out_of_scope")
        # "fails" while some line moves a String into a call
        fails = lambda t: [target] if re.search(r"\b(take|take_gen|drop)\(\w+\);", t) else []
        p = self.find(lambda t: len(t.splitlines()) > 25 and fails(t))
        before = p.render()
        best, rounds = progen.shrink(p, target, fake_checker(fails), time.time() + 60)
        self.assertTrue(fails(best), best)
        self.assertLess(len(best.splitlines()), len(before.splitlines()))
        self.assertGreater(rounds, 0)
        # Minimal: no single statement line can go.
        body = [l for l in best.splitlines() if l.startswith("    ") and l.strip().endswith(";")]
        self.assertLessEqual(len(body), 3, best)

    def test_shrink_never_returns_a_passing_program(self):
        target = ("crash", "x.rs:1")
        fails = lambda t: [target] if "match" in t and "println!" in t else []
        p = self.find(lambda t: fails(t))
        best, _ = progen.shrink(p, target, fake_checker(fails), time.time() + 60)
        self.assertTrue(fails(best))

    def test_shrink_is_deterministic(self):
        target = ("x", "y")
        fails = lambda t: [target] if "if " in t else []
        p = self.find(fails)
        a, _ = progen.shrink(p, target, fake_checker(fails), time.time() + 60)
        b, _ = progen.shrink(p, target, fake_checker(fails), time.time() + 60)
        self.assertEqual(a, b)


class Reading(unittest.TestCase):
    OUT = """running 1 test
invariants: 3 snippets, 22 invariants, 0 known violations

thread 'corpus_invariants_hold' panicked at rustviz-lib/tests/corpus.rs:1:5:
invariant violations differ from KNOWN_INVARIANT_FAILURES (...).

NEW  /w/p1: event-on-dead-column (a column is lent ...)
       r@L5:dot:static_die: r's immutable borrow ends

NEW  /w/p1: outside-figure (an element is drawn outside ...)
       s@L36:dot:owner_go_out_of_scope: y = 1205 outside 0..1175
       s2@L36:dot:owner_go_out_of_scope: y = 1205 outside 0..1175

Snippets the plugin failed on: [
    "/w/p2 failed: RustViz can't visualize this program yet (internal error at timeline_panel.rs:5080: no layout column for the timeline of event 116). Please report it at https://github.com/rustviz/rustviz/issues with your snippet.",
]
"""

    def test_parse(self):
        f = progen.parse_check_output(self.OUT)
        self.assertEqual(f["/w/p1"][0], ("event-on-dead-column", "r@L5:dot:static_die: r's immutable borrow ends"))
        self.assertEqual(len(f["/w/p1"]), 3)
        self.assertEqual(f["/w/p2"], [("crash", "timeline_panel.rs:5080: no layout column for the timeline of event 116")])

    def test_signatures(self):
        types = {"r": "RS", "s": "S", "s2": "S", "w": "P"}
        sig = progen.signature
        self.assertEqual(sig("event-on-dead-column", "r@L5:dot:static_die: r's borrow ends", types), "RS:dot:static_die")
        self.assertEqual(sig("crash", "timeline_panel.rs:5080: no layout column for event 116", types), "timeline_panel.rs:5080")
        self.assertEqual(sig("outside-figure", "s@L36:dot:owner_go_out_of_scope: y = 1205 outside 0..1175", types), "y")
        self.assertEqual(sig("line-through-fork", "s@L4-7:line:full_privilege runs from (1, 2) to (3, 4), through "
                             "s@L5:dot:branch_join at (1, 3)", types), "line / dot")
        self.assertEqual(sig("invalid-state", "s@L3 r@L4-5 s2@L9", types), "RS S")
        self.assertEqual(sig("field-outlives-moved-struct", "w.a@L30-41:line:partial_privilege after w@L31:dot:move: x",
                             types), "P.a:line:partial_privilege / P:dot:move")

    def test_one_key_per_check(self):
        keys = progen.program_keys([("outside-figure", "y"), ("outside-figure", "x"), ("crash", "a.rs:1")])
        self.assertEqual(keys, [("crash", "a.rs:1"), ("outside-figure", "x")])


if __name__ == "__main__":
    unittest.main()
