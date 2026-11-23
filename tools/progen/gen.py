"""Feature-combination Rust programs for RustViz's corpus checks (#1218).

A program is a few helper items plus one generated function body (`fn main`
or `fn run(..) -> Option<usize>`, where `?` and `return None` work). The
body is 3-8 statements drawn from the templates below, over a handful of
typed variables. The generator tracks, per variable, whether it holds a
value (moved, maybe moved, partly moved, not yet assigned) and which loans
it depends on, so most programs it writes pass the borrow checker; rustc
filters out the rest (`progen.py` keeps only programs rustc accepts).

Everything is seeded: `generate(seed, index)` always returns the same
program. The tree (`Program` / `Block` / `Stmt`) is what the shrinker
edits (delete a statement, replace a compound statement with one of its
blocks, take a simpler alternative).
"""
import random
import re

# ─── Helper items (included when the body names them) ─────────────────

HELPERS = [
    ("Holder", "struct Holder<'a> {\n    r: &'a String,\n    n: i32,\n}"),
    ("Pair", "struct Pair {\n    a: String,\n    b: String,\n}"),
    ("get_a", "impl Pair {\n    fn get_a(&self) -> &String {\n        &self.a\n    }\n}"),
    ("set_a", "impl Pair {\n    fn set_a(&mut self, s: String) {\n        self.a = s;\n    }\n}"),
    ("into_a", "impl Pair {\n    fn into_a(self) -> String {\n        self.a\n    }\n}"),
    ("make", 'fn make() -> String {\n    String::from("made")\n}'),
    ("make_pair", 'fn make_pair() -> Pair {\n    Pair { a: make(), b: make() }\n}'),
    ("take", "fn take(_s: String) {}"),
    ("take_gen", "fn take_gen<T>(_t: T) {}"),
    ("id", "fn id<T>(t: T) -> T {\n    t\n}"),
    ("len_of", "fn len_of(s: &String) -> usize {\n    s.len()\n}"),
    ("push_x", "fn push_x(s: &mut String) {\n    s.push('x');\n}"),
    ("mut_gen", "fn mut_gen<T>(_t: &mut T) {}"),
    ("first", "fn first<'a>(s: &'a String) -> &'a str {\n    s.as_str()\n}"),
    ("longest", "fn longest<'a>(a: &'a String, b: &'a String) -> &'a String {\n"
                "    if a.len() > b.len() { a } else { b }\n}"),
    ("downgrade", "fn downgrade(s: &mut String) -> &String {\n    s.push('!');\n    s\n}"),
    ("wrap_mut", "fn wrap_mut<'a>(s: &'a mut String) -> &'a mut String {\n    s\n}"),
    ("holder", "fn holder<'a>(s: &'a String) -> Holder<'a> {\n    Holder { r: s, n: 0 }\n}"),
    ("split", "fn split(s: String) -> (String, i32) {\n    (s, 1)\n}"),
    ("maybe", "fn maybe(s: String) -> Option<String> {\n    Some(s)\n}"),
    ("len_opt", "fn len_opt(s: &String) -> Option<usize> {\n    s.chars().next().map(|_| s.len())\n}"),
    ("apply", "fn apply<F: FnOnce() -> R, R>(f: F) -> R {\n    f()\n}"),
]
# Items an item needs (make_pair calls make and builds a Pair, ...).
HELPER_NEEDS = {"make_pair": ["make", "Pair"], "holder": ["Holder"], "get_a": ["Pair"],
                "set_a": ["Pair"], "into_a": ["Pair"]}
METHODS = {"get_a", "set_a", "into_a"}


def items_for(body_text):
    """Helper items the body uses, in a fixed order."""
    want = set()
    for name, _ in HELPERS:
        pat = r"\.%s\(" % name if name in METHODS else r"\b%s\b" % name
        if re.search(pat, body_text):
            want.add(name)
    changed = True
    while changed:
        changed = False
        for n in list(want):
            for d in HELPER_NEEDS.get(n, []):
                if d not in want:
                    want.add(d)
                    changed = True
    return [src for name, src in HELPERS if name in want]


# ─── Program tree ─────────────────────────────────────────────────────

class Block:
    def __init__(self, stmts=None, tail=None):
        self.stmts = stmts or []
        self.tail = tail  # a tail expression (a block's value), or None


class Arm:
    """`head => { block }` in a match; `inline` renders a one-line arm."""
    def __init__(self, head, block, inline=False):
        self.head, self.block, self.inline = head, block, inline


class Stmt:
    """A statement: `parts` are (indent, str | Block | Arm). `tags` name
    the templates that made it (for coverage and bucket descriptions).
    `alts` are simpler statements the shrinker may substitute."""
    def __init__(self, tags, parts, inline=False, alts=None):
        self.tags = tags if isinstance(tags, list) else [tags]
        self.parts = parts
        self.inline = inline
        self.alts = alts or []

    def blocks(self):
        out = []
        for _, p in self.parts:
            if isinstance(p, Block):
                out.append(p)
            elif isinstance(p, Arm):
                out.append(p.block)
        return out


class Program:
    def __init__(self, fn_head, body, fn_tail=None, params=None, types=None, seed=None):
        self.fn_head = fn_head      # "fn main() {" or a format with {params}
        self.params = params or []  # ["s: String", ...] (run only)
        self.body = body
        self.fn_tail = fn_tail
        self.types = types or {}    # variable name -> type class
        self.seed = seed

    def body_lines(self):
        out = [self.fn_head.replace("PARAMS", ", ".join(self.params))]
        render_block(self.body, "    ", out)
        if self.fn_tail:
            out.append("    " + self.fn_tail)
        out.append("}")
        return out

    def render(self):
        body = "\n".join(self.body_lines())
        return "\n\n".join(items_for(body) + [body]) + "\n"

    def tags(self):
        out = []
        def walk(b):
            for s in b.stmts:
                out.extend(s.tags)
                for c in s.blocks():
                    walk(c)
        walk(self.body)
        return out


def one_line(block):
    """The block on one line, or None if it doesn't fit on one."""
    lines = []
    render_block(block, "", lines)
    if len(lines) > 1 or any("\n" in l for l in lines):
        return None
    return lines[0] if lines else ""


def render_block(block, ind, out):
    for s in block.stmts:
        render_stmt(s, ind, out)
    if block.tail is not None:
        out.append(ind + block.tail)


def render_stmt(s, ind, out):
    if s.inline:
        text = inline_text(s)
        if text is not None and len(text) + len(ind) <= 100:
            out.append(ind + text)
            return
    for depth, p in s.parts:
        pre = ind + "    " * depth
        if isinstance(p, str):
            out.append(pre + p)
        elif isinstance(p, Block):
            render_block(p, pre, out)
        else:
            render_arm(p, pre, out)


def render_arm(arm, pre, out):
    if arm.inline and not arm.block.stmts and arm.block.tail is not None:
        out.append(pre + "%s => %s," % (arm.head, arm.block.tail))
        return
    if arm.inline:
        inner = one_line(arm.block)
        if inner is not None and arm.block.tail is None and len(arm.block.stmts) == 1 \
                and inner.endswith(";") and not inner.startswith("let "):
            out.append(pre + "%s => %s," % (arm.head, inner[:-1]))
            return
        if inner == "":
            out.append(pre + "%s => {}" % arm.head)
            return
    out.append(pre + arm.head + " => {")
    render_block(arm.block, pre + "    ", out)
    out.append(pre + "}")


def inline_text(s):
    bits = []
    for _, p in s.parts:
        if isinstance(p, str):
            bits.append(p)
        elif isinstance(p, Block):
            t = one_line(p)
            if t is None:
                return None
            if t:
                bits.append(t)
        else:
            return None
    return " ".join(bits)


# ─── Variables and the borrow model ───────────────────────────────────

# Type classes: S String, RS &String, MS &mut String, STR &str, VS
# Vec<String>, VR Vec<&str>, H Holder (a reference field), P Pair (owned
# fields), B Box<String>, O Option<String>, T (String, i32), A [String; 2],
# I i32, CF/CM/CO/CMV closures (Fn borrowing, FnMut, FnOnce returning a
# String, `move` Fn).
COPY = {"RS", "STR", "I", "CF"}
DISPLAY = {"S", "RS", "MS", "STR", "B", "I"}
DEBUG = {"VS", "VR", "O", "T", "A"}
RUST_TY = {"S": "String", "RS": "&String", "MS": "&mut String", "STR": "&str", "VS": "Vec<String>",
           "VR": "Vec<&str>", "H": "Holder", "P": "Pair", "B": "Box<String>", "O": "Option<String>",
           "T": "(String, i32)", "A": "[String; 2]", "I": "i32"}
BASE = {"S": "s", "RS": "r", "MS": "m", "STR": "x", "VS": "v", "VR": "vr", "H": "h", "P": "p",
        "B": "b", "O": "o", "T": "tp", "A": "arr", "I": "n", "CF": "c", "CM": "c", "CO": "c", "CMV": "c"}
WORDS = ["hi", "ab", "rust", "x", "abc", "hey"]


class Var:
    def __init__(self, name, ty, mut, deps, tick, depth, loop):
        self.name, self.ty, self.mut = name, ty, mut
        self.state = "ok"          # ok | moved | maybe | uninit
        self.moved_fields = set()  # fields moved out (P: a/b, T: 0)
        self.deps = deps           # [(lender Var, "shr" | "mut", tick)]
        self.r = self.w = tick     # last read / last write-like access
        self.depth, self.loop = depth, loop
        self.hidden = False        # shadowed


class Gen:
    def __init__(self, seed, index=0, fn_kind=None):
        self.rng = random.Random("progen:%s:%s" % (seed, index))
        self.tick = 0
        self.scopes = [[]]
        self.loop = 0          # loop nesting level
        self.loop_labels = []  # label (or None) of each enclosing loop
        self.allow_outer_move = False
        self.names = set()
        self.types = {}
        self.depth = 0
        self.dead = False      # the current block can't continue (diverged)
        self.fn_kind = fn_kind or self.rng.choice(["main", "main", "run"])
        self.ret = "opt" if self.fn_kind == "run" else "unit"
        self.label_n = 0
        self.breaks = []

    # ── variables
    def visible(self):
        seen, out = set(), []
        for scope in reversed(self.scopes):
            for v in reversed(scope):
                if v.name not in seen and not v.hidden:
                    seen.add(v.name)
                    out.append(v)
        out.reverse()
        return out

    def fresh(self, base):
        name = base
        k = 2
        while name in self.names:
            name = "%s%d" % (base, k)
            k += 1
        self.names.add(name)
        return name

    def new_var(self, ty, mut=False, deps=None, name=None, state="ok"):
        name = name or self.fresh(BASE[ty])
        self.tick += 1
        v = Var(name, ty, mut, list(deps or []), self.tick, self.depth, self.loop)
        v.state = state
        self.scopes[-1].append(v)
        self.types.setdefault(name, ty)
        return v

    def deps_ok(self, v, seen=None):
        for (l, kind, t0) in v.deps:
            if l.w > t0 or (kind == "mut" and l.r > t0):
                return False
            if l.state != "ok" and kind != "own":
                return False
        return True

    def ok(self, v):
        return v.state == "ok" and not v.moved_fields and self.deps_ok(v)

    def field_ok(self, v, f):
        return v.state == "ok" and f not in v.moved_fields and self.deps_ok(v)

    def touch(self, v, write=False):
        self.tick += 1
        v.r = self.tick
        if write:
            v.w = self.tick

    def movable(self, v):
        return self.ok(v) and (v.loop == self.loop or self.allow_outer_move)

    def move(self, v):
        self.touch(v, write=True)
        if v.ty not in COPY:
            v.state = "moved"

    def borrow(self, v, kind):
        """deps of a new reference to v."""
        self.touch(v, write=(kind == "mut"))
        return [d for d in v.deps] + [(v, kind, self.tick)]

    def pick(self, pred):
        c = [v for v in self.visible() if pred(v)]
        return self.rng.choice(c) if c else None

    def pick_ty(self, tys, extra=lambda v: True, okf=None):
        okf = okf or self.ok
        return self.pick(lambda v: v.ty in tys and okf(v) and extra(v))

    # ── snapshots for branches
    def snap(self):
        return {id(v): (v, v.state, set(v.moved_fields), list(v.deps), v.r, v.w)
                for scope in self.scopes for v in scope}

    def restore(self, sn):
        for (v, st, mf, deps, r, w) in sn.values():
            v.state, v.moved_fields, v.deps, v.r, v.w = st, set(mf), list(deps), r, w

    def merge(self, base, outs):
        """State after a conditional whose live paths end in `outs`."""
        if not outs:
            self.restore(base)
            self.dead = True
            return
        for key, (v, st0, *_rest) in base.items():
            sts = [o[key][1] for o in outs]
            if all(s == sts[0] for s in sts):
                st = sts[0]
            elif all(s in ("ok", "uninit") for s in sts) and st0 == "uninit":
                st = "maybe"
            else:
                st = "maybe"
            v.state = st
            v.moved_fields = set().union(*[o[key][2] for o in outs])
            deps = []
            for o in outs:
                for d in o[key][3]:
                    if d not in deps:
                        deps.append(d)
            v.deps = deps
            v.r = max(o[key][4] for o in outs)
            v.w = max(o[key][5] for o in outs)

    # ── blocks
    def block(self, n, loop=False, label=None, inner=None, scope_vars=()):
        """A child block of n statements; returns (Block, end snapshot or
        None if it diverged, break snapshots)."""
        self.scopes.append(list(scope_vars))
        self.depth += 1
        saved_dead = self.dead
        self.dead = False
        if loop:
            self.loop += 1
            self.loop_labels.append(label)
            saved_breaks, self.breaks = self.breaks, []
        stmts = []
        for k in range(n):
            if self.dead:
                break
            s = inner(k) if inner else self.stmt()
            if s:
                stmts.append(s)
        breaks = []
        if loop:
            breaks, self.breaks = self.breaks, saved_breaks
            self.loop -= 1
            self.loop_labels.pop()
        end = None if self.dead else self.snap()
        self.dead = saved_dead
        self.depth -= 1
        self.scopes.pop()
        return Block(stmts), end, breaks

    # ── values
    def word(self):
        return '"%s"' % self.rng.choice(WORDS)

    def cond(self):
        """A bool expression reading some variable."""
        if self.fn_kind == "run" and self.rng.random() < 0.3:
            return "flag"
        v = self.pick_ty({"S", "RS", "MS", "B", "I", "VS", "VR", "O", "STR"})
        if not v:
            return "true"
        self.touch(v)
        return {"S": "%s.len() > 1", "RS": "%s.is_empty()", "MS": "%s.len() > 2", "B": "%s.len() > 2",
                "I": "%s > 0", "VS": "%s.len() > 1", "VR": "%s.is_empty()", "O": "%s.is_some()",
                "STR": "%s.len() > 1"}[v.ty] % v.name

    def string_value(self, moving=True):
        """(expr, tag) for a String; moves happen now."""
        r = self.rng.random()
        choices = []
        if moving:
            choices += [("move", 6), ("box_out", 1), ("unwrap", 1), ("field", 2), ("tuple_field", 1),
                        ("pop", 1), ("mem_take", 1), ("id", 1), ("call_once", 1), ("into_a", 1)]
        choices += [("lit", 5), ("make", 1), ("clone", 2), ("to_string", 1), ("format", 1)]
        total = sum(w for _, w in choices)
        x = r * total
        for kind, w in choices:
            x -= w
            if x <= 0:
                break
        res = self.string_of(kind)
        if res is None:
            return ("String::from(%s)" % self.word(), "lit")
        return res

    def string_of(self, kind):
        rng = self.rng
        if kind == "lit":
            return ("String::from(%s)" % self.word(), "lit")
        if kind == "make":
            return ("make()", "call_ret_owned")
        if kind == "move":
            v = self.pick(lambda v: v.ty == "S" and self.movable(v))
            if v:
                self.move(v)
                return (v.name, "move")
        if kind == "box_out":
            v = self.pick(lambda v: v.ty == "B" and self.movable(v))
            if v:
                self.move(v)
                return ("*" + v.name, "box_move_out")
        if kind == "unwrap":
            v = self.pick(lambda v: v.ty == "O" and self.movable(v))
            if v:
                self.move(v)
                return ("%s.unwrap()" % v.name, "unwrap")
        if kind == "field":
            v = self.pick(lambda v: v.ty == "P" and v.state == "ok" and self.deps_ok(v)
                          and len(v.moved_fields) < 2 and (v.loop == self.loop or self.allow_outer_move))
            if v:
                f = rng.choice([f for f in "ab" if f not in v.moved_fields])
                self.touch(v, write=True)
                v.moved_fields.add(f)
                return ("%s.%s" % (v.name, f), "field_move")
        if kind == "tuple_field":
            v = self.pick(lambda v: v.ty == "T" and self.movable(v))
            if v:
                self.touch(v, write=True)
                v.moved_fields.add("0")
                return ("%s.0" % v.name, "field_move")
        if kind == "pop":
            v = self.pick(lambda v: v.ty == "VS" and v.mut and self.ok(v))
            if v:
                self.touch(v, write=True)
                return (rng.choice(["%s.pop().unwrap()", "%s.remove(0)"]) % v.name, "vec_take")
        if kind == "mem_take":
            v = self.pick(lambda v: (v.ty == "S" and v.mut or v.ty == "MS") and self.ok(v))
            if v:
                self.touch(v, write=True)
                arg = v.name if v.ty == "MS" else "&mut " + v.name
                return ("std::mem::%s" % rng.choice(["take(%s)" % arg, "replace(%s, String::new())" % arg]),
                        "mem_take")
        if kind == "id":
            v = self.pick(lambda v: v.ty == "S" and self.movable(v))
            if v:
                self.move(v)
                return ("id(%s)" % v.name, "call_generic_move")
        if kind == "call_once":
            v = self.pick(lambda v: v.ty == "CO" and self.movable(v))
            if v:
                self.move(v)
                return (rng.choice(["%s()", "apply(%s)"]) % v.name, "closure_call_once")
        if kind == "into_a":
            v = self.pick(lambda v: v.ty == "P" and self.movable(v))
            if v:
                self.move(v)
                return ("%s.into_a()" % v.name, "method_self")
        if kind == "clone":
            v = self.pick_ty({"S", "RS", "MS", "B"})
            if v:
                self.touch(v)
                return ("%s.clone()" % v.name if v.ty != "B" else "(*%s).clone()" % v.name, "clone")
        if kind == "to_string":
            v = self.pick_ty({"S", "RS", "MS", "STR"})
            if v:
                self.touch(v)
                return ("%s.%s()" % (v.name, rng.choice(["to_string", "to_uppercase"])), "read_call")
        if kind == "format":
            v = self.pick_ty(DISPLAY)
            if v:
                self.touch(v)
                return ('format!("{}!", %s)' % v.name, "format")
        return None

    def ref_value(self):
        """(expr, deps, tag) for a &String."""
        rng = self.rng
        opts = []
        s = self.pick_ty({"S"})
        if s:
            opts += [("amp", 4), ("longest", 1)]
        if self.pick_ty({"RS"}):
            opts.append(("copy", 2))
        if self.pick_ty({"MS"}):
            opts += [("reborrow", 1)]
        if self.pick(lambda v: v.ty == "S" and v.mut and self.ok(v)):
            opts.append(("downgrade", 1))
        if self.pick_ty({"VS"}):
            opts.append(("vfirst", 1))
        if self.pick_ty({"O"}):
            opts.append(("oref", 1))
        if self.pick_ty({"B"}):
            opts.append(("boxref", 1))
        if self.pick(lambda v: v.ty == "P" and self.ok(v)):
            opts += [("pfield", 1), ("get_a", 1)]
        if self.pick_ty({"H"}):
            opts.append(("hfield", 1))
        if not opts:
            return None
        kind = weighted(rng, opts)
        if kind == "amp":
            v = self.pick_ty({"S"})
            return ("&" + v.name, self.borrow(v, "shr"), "borrow")
        if kind == "longest":
            a = self.pick_ty({"S"})
            b = self.pick_ty({"S"})
            d = self.borrow(a, "shr") + self.borrow(b, "shr")
            return ("longest(&%s, &%s)" % (a.name, b.name), d, "call_ret_ref")
        if kind == "copy":
            v = self.pick_ty({"RS"})
            self.touch(v)
            return (v.name, list(v.deps), "ref_copy")
        if kind == "reborrow":
            v = self.pick_ty({"MS"})
            return ("&*" + v.name, self.borrow(v, "shr"), "reborrow")
        if kind == "downgrade":
            v = self.pick(lambda v: v.ty == "S" and v.mut and self.ok(v))
            return ("downgrade(&mut %s)" % v.name, self.borrow(v, "mut"), "call_mut_ret_shared")
        if kind == "vfirst":
            v = self.pick_ty({"VS"})
            return (rng.choice(["%s.first().unwrap()", "&%s[0]", "%s.last().unwrap()"]) % v.name,
                    self.borrow(v, "shr"), "method_ret_ref")
        if kind == "oref":
            v = self.pick_ty({"O"})
            return ("%s.as_ref().unwrap()" % v.name, self.borrow(v, "shr"), "method_ret_ref")
        if kind == "boxref":
            v = self.pick_ty({"B"})
            return (rng.choice(["&*%s", "&%s"]) % v.name, self.borrow(v, "shr"), "box_borrow")
        if kind == "pfield":
            v = self.pick(lambda v: v.ty == "P" and self.ok(v))
            return ("&%s.%s" % (v.name, rng.choice("ab")), self.borrow(v, "shr"), "field_borrow")
        if kind == "get_a":
            v = self.pick(lambda v: v.ty == "P" and self.ok(v))
            return ("%s.get_a()" % v.name, self.borrow(v, "shr"), "method_ref_self")
        if kind == "hfield":
            v = self.pick_ty({"H"})
            self.touch(v)
            return ("%s.r" % v.name, list(v.deps), "field_ref_copy")
        return None

    def mut_value(self):
        """(expr, deps, tag) for a &mut String."""
        rng = self.rng
        opts = []
        if self.pick(lambda v: v.ty == "S" and v.mut and self.ok(v)):
            opts += [("amp", 5), ("wrap", 1)]
        if self.pick(lambda v: v.ty == "MS" and self.movable(v)):
            opts += [("move", 1), ("reborrow", 1)]
        if self.pick(lambda v: v.ty == "VS" and v.mut and self.ok(v)):
            opts.append(("vlast", 1))
        if self.pick(lambda v: v.ty == "O" and v.mut and self.ok(v)):
            opts.append(("omut", 1))
        if self.pick(lambda v: v.ty == "B" and v.mut and self.ok(v)):
            opts.append(("bmut", 1))
        if self.pick(lambda v: v.ty == "P" and v.mut and self.ok(v)):
            opts.append(("pfield", 1))
        if not opts:
            return None
        kind = weighted(rng, opts)
        if kind == "amp":
            v = self.pick(lambda v: v.ty == "S" and v.mut and self.ok(v))
            return ("&mut " + v.name, self.borrow(v, "mut"), "borrow_mut")
        if kind == "wrap":
            v = self.pick(lambda v: v.ty == "S" and v.mut and self.ok(v))
            return ("wrap_mut(&mut %s)" % v.name, self.borrow(v, "mut"), "call_ret_mut")
        if kind == "move":
            v = self.pick(lambda v: v.ty == "MS" and self.movable(v))
            deps = list(v.deps)
            self.move(v)
            return (v.name, deps, "mut_ref_move")
        if kind == "reborrow":
            v = self.pick(lambda v: v.ty == "MS" and self.movable(v))
            return ("&mut *" + v.name, self.borrow(v, "mut"), "reborrow_mut")
        if kind == "vlast":
            v = self.pick(lambda v: v.ty == "VS" and v.mut and self.ok(v))
            return (rng.choice(["%s.last_mut().unwrap()", "&mut %s[0]", "%s.iter_mut().next().unwrap()"]) % v.name,
                    self.borrow(v, "mut"), "method_ret_mut")
        if kind == "omut":
            v = self.pick(lambda v: v.ty == "O" and v.mut and self.ok(v))
            return ("%s.as_mut().unwrap()" % v.name, self.borrow(v, "mut"), "method_ret_mut")
        if kind == "bmut":
            v = self.pick(lambda v: v.ty == "B" and v.mut and self.ok(v))
            return (rng.choice(["&mut *%s", "%s.as_mut()"]) % v.name, self.borrow(v, "mut"), "box_borrow_mut")
        if kind == "pfield":
            v = self.pick(lambda v: v.ty == "P" and v.mut and self.ok(v))
            return ("&mut %s.%s" % (v.name, rng.choice("ab")), self.borrow(v, "mut"), "field_borrow_mut")
        return None

    # ── statements
    def stmt(self):
        """One statement (or None if the chosen template had nothing to
        work on; the caller just moves on)."""
        for _ in range(12):
            t = weighted(self.rng, self.templates())
            s = t()
            if s is not None:
                return s
        return None

    def templates(self):
        nested = self.depth < 2
        ts = [
            (self.t_let_owned, 10), (self.t_let_ref, 6), (self.t_let_mut_ref, 5), (self.t_read, 10),
            (self.t_call, 7), (self.t_method, 6), (self.t_assign, 4), (self.t_place_write, 4),
            (self.t_destructure_assign, 2), (self.t_let_pattern, 4), (self.t_closure, 3),
            (self.t_drop, 1), (self.t_shadow, 1), (self.t_let_underscore, 1), (self.t_swap, 1),
        ]
        if nested:
            ts += [(self.t_if, 4), (self.t_match, 4), (self.t_if_let, 3), (self.t_let_else, 2),
                   (self.t_let_chain, 2), (self.t_labeled_block, 1), (self.t_loop, 3), (self.t_block, 1),
                   (self.t_let_branchy, 3), (self.t_let_deferred, 1)]
        if self.ret == "opt":
            ts.append((self.t_try, 3))
        if self.loop:
            ts.append((self.t_break_continue, 3))
            if nested:
                ts.append((self.t_move_then_break, 2))
        if self.depth > 0:
            ts.append((self.t_return, 1))
        return ts

    def t_let_owned(self, ty=None):
        rng = self.rng
        ty = ty or weighted(rng, [("S", 8), ("VS", 2), ("VR", 1), ("H", 2), ("P", 2), ("B", 2), ("O", 2),
                                  ("T", 1), ("A", 1), ("I", 1)])
        mut = rng.random() < 0.6
        deps = []
        tag = "let_" + ty.lower()
        if ty == "S":
            e, t2 = self.string_value()
            tag = "let_string_" + t2
        elif ty == "VS":
            if rng.random() < 0.5:
                e, mut = "Vec::new()", True
                annot = True
            else:
                a, _ = self.string_value()
                b, _ = self.string_value()
                e = "vec![%s, %s]" % (a, b)
        elif ty == "VR":
            s = self.pick_ty({"S"})
            if s:
                e = "vec![%s.as_str(), %s]" % (s.name, self.word())
                deps = self.borrow(s, "shr")
            else:
                e = "vec![%s]" % self.word()
        elif ty == "H":
            r = self.ref_value()
            if not r:
                return None
            e = rng.choice(["Holder { r: %s, n: 1 }", "holder(%s)"]) % r[0]
            if r[0].startswith("&") and "holder" in e:
                pass
            deps = r[1]
            tag = "let_struct_ref_field"
            h2 = self.pick_ty({"H"})
            if h2 and rng.random() < 0.3:
                self.touch(h2)
                e, deps, tag = "Holder { n: 2, ..%s }" % h2.name, list(h2.deps), "struct_update_copy"
        elif ty == "P":
            k = rng.random()
            p2 = self.pick(lambda v: v.ty == "P" and v.state == "ok" and "b" not in v.moved_fields
                           and self.deps_ok(v) and v.loop == self.loop)
            if k < 0.2:
                e, tag = "make_pair()", "call_ret_struct"
            elif k < 0.5 and p2:
                a, _ = self.string_value()
                self.touch(p2, write=True)
                p2.moved_fields.add("b")
                e, tag = "Pair { a: %s, ..%s }" % (a, p2.name), "struct_update_move"
            else:
                a, _ = self.string_value()
                b, _ = self.string_value()
                e, tag = "Pair { a: %s, b: %s }" % (a, b), "struct_literal"
        elif ty == "B":
            a, _ = self.string_value()
            e = "Box::new(%s)" % a
        elif ty == "O":
            k = rng.random()
            if k < 0.2:
                e, mut = "None", True
            elif k < 0.4:
                a, _ = self.string_value()
                e, tag = "maybe(%s)" % a, "call_ret_option"
            else:
                a, _ = self.string_value()
                e = "Some(%s)" % a
        elif ty == "T":
            if rng.random() < 0.4:
                a, _ = self.string_value()
                e, tag = "split(%s)" % a, "call_ret_tuple"
            else:
                a, _ = self.string_value()
                e, tag = "(%s, 1)" % a, "tuple_literal"
        elif ty == "A":
            a, _ = self.string_value()
            b, _ = self.string_value()
            e, tag = "[%s, %s]" % (a, b), "array_literal"
        elif ty == "I":
            v = self.pick_ty({"S", "RS", "MS", "VS"})
            if v and rng.random() < 0.6:
                self.touch(v)
                e = "%s.len() as i32" % v.name
            else:
                e = str(rng.randint(0, 3))
        v = self.new_var(ty, mut, deps)
        annot = ": " + RUST_TY[ty] if (e in ("Vec::new()", "None") and ty in RUST_TY) else ""
        return Stmt(tag, [(0, "let %s%s%s = %s;" % ("mut " if mut else "", v.name, annot, e))],
                    alts=self.alt_nomut(mut, v, annot, e))

    def alt_nomut(self, mut, v, annot, e):
        if not mut:
            return []
        return [Stmt("let", [(0, "let %s%s = %s;" % (v.name, annot, e))])]

    def t_let_ref(self):
        rng = self.rng
        if rng.random() < 0.25:
            s = self.pick_ty({"S", "B"})
            if not s:
                return None
            e = rng.choice(["%s.as_str()", "first(&%s)", "&%s[..]"]) % s.name
            if s.ty == "B":
                e = "%s.as_str()" % s.name
            deps = self.borrow(s, "shr")
            v = self.new_var("STR", deps=deps)
            return Stmt("let_str_ref", [(0, "let %s = %s;" % (v.name, e))])
        r = self.ref_value()
        if not r:
            return None
        mut = rng.random() < 0.2
        v = self.new_var("RS", mut, r[1])
        return Stmt("let_ref_" + r[2], [(0, "let %s%s = %s;" % ("mut " if mut else "", v.name, r[0]))])

    def t_let_mut_ref(self):
        r = self.mut_value()
        if not r:
            return None
        v = self.new_var("MS", False, r[1])
        return Stmt("let_mut_ref_" + r[2], [(0, "let %s = %s;" % (v.name, r[0]))])

    def t_read(self):
        rng = self.rng
        cands = [v for v in self.visible() if (v.ty in DISPLAY or v.ty in DEBUG or v.ty in ("H", "P"))
                 and (self.ok(v) or (v.ty == "P" and v.state == "ok" and len(v.moved_fields) < 2
                                     and self.deps_ok(v)))]
        if not cands:
            return None
        k = 1 if rng.random() < 0.7 or len(cands) < 2 else 2
        picks = rng.sample(cands, k)
        args, fmts = [], []
        for v in picks:
            self.touch(v)
            if v.ty in DISPLAY:
                args.append(v.name); fmts.append("{}")
            elif v.ty in DEBUG:
                args.append(v.name); fmts.append("{:?}")
            elif v.ty == "H":
                args.append("%s.%s" % (v.name, rng.choice(["r", "n"]))); fmts.append("{}")
            else:
                f = [f for f in "ab" if f not in v.moved_fields][0]
                args.append("%s.%s" % (v.name, f)); fmts.append("{}")
        return Stmt("read", [(0, 'println!("%s", %s);' % (" ".join(fmts), ", ".join(args)))])

    def t_call(self):
        rng = self.rng
        k = weighted(rng, [("take", 3), ("take_gen", 2), ("take_gen_ref", 1), ("take_gen_mut", 2),
                           ("len_of", 2), ("push_x", 3), ("push_x_reborrow", 2), ("mut_gen", 2),
                           ("set_a", 1), ("apply", 1)])
        if k in ("take", "take_gen"):
            if k == "take_gen" and rng.random() < 0.5:
                v = self.pick(lambda v: v.ty in ("VS", "P", "B", "O", "H", "VR", "T") and self.movable(v))
                if not v:
                    return None
                self.move(v)
                return Stmt("call_generic_move", [(0, "take_gen(%s);" % v.name)])
            e, t = self.string_value()
            if t == "lit":
                return None
            return Stmt("call_move" if k == "take" else "call_generic_move", [(0, "%s(%s);" % (k, e))])
        if k == "take_gen_ref":
            r = self.ref_value()
            if not r:
                return None
            return Stmt("call_generic_ref", [(0, "take_gen(%s);" % r[0])])
        if k == "take_gen_mut":
            v = self.pick(lambda v: v.ty == "MS" and self.movable(v))
            if v:
                self.move(v)
                return Stmt("call_generic_mut_ref_move", [(0, "take_gen(%s);" % v.name)])
            r = self.mut_value()
            if not r:
                return None
            return Stmt("call_generic_mut", [(0, "take_gen(%s);" % r[0])])
        if k == "len_of":
            r = self.ref_value()
            if not r:
                return None
            return Stmt("call_ref", [(0, "len_of(%s);" % r[0])])
        if k == "push_x":
            r = self.mut_value()
            if not r:
                return None
            return Stmt("call_mut", [(0, "push_x(%s);" % r[0])])
        if k == "push_x_reborrow":
            v = self.pick_ty({"MS"})
            if not v:
                return None
            self.touch(v, write=True)
            return Stmt("call_mut_implicit_reborrow", [(0, rng.choice(["push_x(%s);", "push_x(&mut *%s);"]) % v.name)])
        if k == "mut_gen":
            v = self.pick_ty({"MS"})
            if v and rng.random() < 0.5:
                self.touch(v, write=True)
                return Stmt("call_generic_mut_reborrow", [(0, "mut_gen(%s);" % v.name)])
            v = self.pick(lambda v: v.mut and v.ty in ("S", "VS", "P", "O", "B", "H") and self.ok(v))
            if not v:
                return None
            self.touch(v, write=True)
            return Stmt("call_generic_mut", [(0, "mut_gen(&mut %s);" % v.name)])
        if k == "set_a":
            v = self.pick(lambda v: v.ty == "P" and v.mut and v.state == "ok" and self.deps_ok(v))
            if not v:
                return None
            e, _ = self.string_value()
            if not (v.state == "ok" and self.deps_ok(v)) or v.moved_fields:
                return None
            self.touch(v, write=True)
            return Stmt("method_mut_self", [(0, "%s.set_a(%s);" % (v.name, e))])
        if k == "apply":
            v = self.pick(lambda v: v.ty in ("CF", "CO", "CMV", "CM") and self.movable(v))
            if not v:
                return None
            self.move(v)
            return Stmt("call_closure_generic", [(0, "apply(%s);" % v.name)])
        return None

    def t_method(self):
        rng = self.rng
        k = weighted(rng, [("push", 4), ("vpush", 3), ("vrpush", 2), ("iter_mut", 2), ("last_mut", 2),
                           ("otake", 1), ("oinsert", 1), ("len", 2), ("call_closure", 3), ("clear", 1)])
        if k == "push":
            v = self.pick(lambda v: (v.ty in ("S", "B") and v.mut or v.ty == "MS") and self.ok(v))
            if not v:
                return None
            self.touch(v, write=True)
            return Stmt("method_mut", [(0, rng.choice(["%s.push('!');", '%s.push_str("ab");']) % v.name)])
        if k == "clear":
            v = self.pick(lambda v: (v.ty in ("S", "VS", "VR") and v.mut or v.ty == "MS") and self.ok(v))
            if not v:
                return None
            self.touch(v, write=True)
            return Stmt("method_mut", [(0, "%s.clear();" % v.name)])
        if k == "vpush":
            v = self.pick(lambda v: v.ty == "VS" and v.mut and self.ok(v))
            if not v:
                return None
            e, t = self.string_value()
            if not self.ok(v):
                return None
            self.touch(v, write=True)
            return Stmt("vec_push_" + ("move" if t == "move" else "value"), [(0, "%s.push(%s);" % (v.name, e))])
        if k == "vrpush":
            v = self.pick(lambda v: v.ty == "VR" and v.mut and self.ok(v))
            s = self.pick_ty({"S", "RS", "B"})
            if not v or not s:
                return None
            d = self.borrow(s, "shr") if s.ty != "RS" else list(s.deps)
            self.touch(v, write=True)
            v.deps += d
            return Stmt("vec_push_borrow", [(0, "%s.push(%s.as_str());" % (v.name, s.name))])
        if k == "iter_mut":
            v = self.pick(lambda v: v.ty == "VS" and v.mut and self.ok(v))
            if not v:
                return None
            self.touch(v, write=True)
            x = self.fresh("e")
            return Stmt(["for_iter_mut", "method_iter_mut"],
                        [(0, "for %s in %s.iter_mut() {" % (x, v.name)), (1, "%s.push('!');" % x), (0, "}")],
                        inline=rng.random() < 0.3)
        if k == "last_mut":
            v = self.pick(lambda v: v.ty == "VS" and v.mut and self.ok(v))
            if not v:
                return None
            self.touch(v, write=True)
            x = self.fresh("l")
            e, _ = self.string_value(moving=False)
            return Stmt(["if_let", "method_last_mut", "deref_write"],
                        [(0, "if let Some(%s) = %s.last_mut() {" % (x, v.name)), (1, "*%s = %s;" % (x, e)), (0, "}")],
                        inline=rng.random() < 0.3)
        if k == "otake":
            v = self.pick(lambda v: v.ty == "O" and v.mut and self.ok(v))
            if not v:
                return None
            self.touch(v, write=True)
            if rng.random() < 0.5:
                n = self.new_var("O")
                return Stmt("method_take", [(0, "let %s = %s.take();" % (n.name, v.name))])
            return Stmt("method_take", [(0, "%s.take();" % v.name)])
        if k == "oinsert":
            v = self.pick(lambda v: v.ty == "O" and v.mut and (self.ok(v) or v.state == "moved"))
            if not v:
                return None
            e, _ = self.string_value()
            if v.state == "moved":
                self.touch(v, write=True)
                v.state = "ok"
                return Stmt("assign_reinit", [(0, "%s = Some(%s);" % (v.name, e))])
            if not self.ok(v):
                return None
            self.touch(v, write=True)
            return Stmt("method_replace", [(0, "%s.replace(%s);" % (v.name, e))])
        if k == "len":
            v = self.pick_ty({"S", "RS", "MS", "VS", "VR", "B", "STR"})
            if not v:
                return None
            self.touch(v)
            n = self.new_var("I")
            return Stmt("method_read", [(0, "let %s = %s.len();" % (n.name, v.name))])
        if k == "call_closure":
            v = self.pick(lambda v: v.ty in ("CF", "CMV") and self.ok(v) or v.ty == "CM" and v.mut and self.ok(v))
            if not v:
                return None
            self.touch(v, write=v.ty == "CM")
            return Stmt("closure_call", [(0, "%s();" % v.name)])
        return None

    def t_assign(self):
        rng = self.rng
        v = self.pick(lambda v: v.mut and v.ty in ("S", "RS", "B", "O", "VS", "I", "MS", "P")
                      and v.state in ("ok", "moved", "maybe") and (v.state != "ok" or self.deps_ok(v) or v.ty in ("RS", "MS")))
        if not v:
            return None
        before = v.state
        if v.ty == "S":
            e, t = self.string_value()
            if v.state == "moved" and t == "move" and e == v.name:
                return None
        elif v.ty == "RS":
            r = self.ref_value()
            if not r:
                return None
            e = r[0]
            if any(d[0].depth > v.depth for d in r[1]):
                return None
            v.deps = r[1]
        elif v.ty == "MS":
            r = self.mut_value()
            if not r or any(d[0].depth > v.depth for d in r[1]) or r[0] == v.name:
                return None
            e = r[0]
            v.deps = r[1]
        elif v.ty == "B":
            a, _ = self.string_value()
            e = "Box::new(%s)" % a
        elif v.ty == "O":
            a, _ = self.string_value()
            e = rng.choice(["Some(%s)" % a, "None"])
        elif v.ty == "VS":
            e = "Vec::new()"
        elif v.ty == "P":
            a, _ = self.string_value()
            b, _ = self.string_value()
            e = "Pair { a: %s, b: %s }" % (a, b)
        else:
            e = str(rng.randint(0, 5))
        self.touch(v, write=True)
        v.state = "ok"
        v.moved_fields = set()
        tag = "assign_reinit" if before != "ok" else ("assign_ref" if v.ty in ("RS", "MS") else "assign")
        return Stmt(tag, [(0, "%s = %s;" % (v.name, e))])

    def t_place_write(self):
        rng = self.rng
        k = weighted(rng, [("deref", 3), ("box", 1), ("field", 2), ("hfield", 1), ("index", 2),
                           ("tuple", 1), ("array", 1), ("addassign", 1)])
        if k in ("deref", "addassign"):
            v = self.pick_ty({"MS"})
            if not v:
                return None
            if k == "addassign":
                self.touch(v, write=True)
                return Stmt("deref_add_assign", [(0, '*%s += "!";' % v.name)])
            e, _ = self.string_value()
            if not self.ok(v):
                return None
            self.touch(v, write=True)
            return Stmt("deref_write", [(0, "*%s = %s;" % (v.name, e))])
        if k == "box":
            v = self.pick(lambda v: v.ty == "B" and v.mut and self.ok(v))
            if not v:
                return None
            e, _ = self.string_value()
            if not self.ok(v):
                return None
            self.touch(v, write=True)
            return Stmt("box_deref_write", [(0, "*%s = %s;" % (v.name, e))])
        if k == "field":
            v = self.pick(lambda v: v.ty == "P" and v.mut and v.state == "ok" and self.deps_ok(v))
            if not v:
                return None
            f = rng.choice(sorted(v.moved_fields) or ["a", "b"])
            e, _ = self.string_value()
            if v.state != "ok":
                return None
            self.touch(v, write=True)
            reinit = f in v.moved_fields
            v.moved_fields.discard(f)
            return Stmt("field_write_reinit" if reinit else "field_write", [(0, "%s.%s = %s;" % (v.name, f, e))])
        if k == "hfield":
            v = self.pick(lambda v: v.ty == "H" and v.mut and v.state == "ok")
            if not v:
                return None
            if rng.random() < 0.5:
                if not self.ok(v):
                    return None
                self.touch(v, write=True)
                return Stmt("field_write_copy", [(0, "%s.n += 1;" % v.name)])
            r = self.ref_value()
            if not r or any(d[0].depth > v.depth for d in r[1]):
                return None
            self.touch(v, write=True)
            v.deps = r[1]
            return Stmt("field_write_ref", [(0, "%s.r = %s;" % (v.name, r[0]))])
        if k == "index":
            v = self.pick(lambda v: v.ty == "VS" and v.mut and self.ok(v))
            if not v:
                return None
            e, _ = self.string_value()
            if not self.ok(v):
                return None
            self.touch(v, write=True)
            return Stmt("index_write", [(0, "%s[0] = %s;" % (v.name, e))])
        if k == "tuple":
            v = self.pick(lambda v: v.ty == "T" and v.mut and v.state == "ok")
            if not v:
                return None
            e, _ = self.string_value()
            if v.state != "ok":
                return None
            self.touch(v, write=True)
            v.moved_fields.discard("0")
            return Stmt("tuple_field_write", [(0, "%s.0 = %s;" % (v.name, e))])
        if k == "array":
            v = self.pick(lambda v: v.ty == "A" and v.mut and self.ok(v))
            if not v:
                return None
            e, _ = self.string_value()
            if not self.ok(v):
                return None
            self.touch(v, write=True)
            return Stmt("index_write", [(0, "%s[%d] = %s;" % (v.name, rng.randint(0, 1), e))])
        return None

    def t_destructure_assign(self):
        rng = self.rng
        ss = [v for v in self.visible() if v.ty == "S" and v.mut and v.state in ("ok", "moved", "maybe")
              and v.loop == self.loop]
        k = rng.random()
        if k < 0.4:
            oks = [v for v in ss if self.ok(v)]
            if len(oks) < 2:
                return None
            a, b = rng.sample(oks, 2)
            self.touch(a, write=True)
            self.touch(b, write=True)
            return Stmt("destructure_assign_swap", [(0, "(%s, %s) = (%s, %s);" % (a.name, b.name, b.name, a.name))])
        if k < 0.7:
            n = self.pick(lambda v: v.ty == "I" and v.mut)
            if not ss or not n:
                return None
            a = rng.choice(ss)
            e, t = self.string_value()
            if e == a.name:
                return None
            self.touch(a, write=True)
            a.state = "ok"
            n.state = "ok"
            return Stmt("destructure_assign_tuple", [(0, "(%s, %s) = split(%s);" % (a.name, n.name, e))])
        if len(ss) < 2:
            return None
        a, b = rng.sample(ss, 2)
        p = self.pick(lambda v: v.ty == "P" and self.movable(v))
        if p:
            self.move(p)
            src = p.name
        else:
            src = "make_pair()"
        for x in (a, b):
            self.touch(x, write=True)
            x.state = "ok"
        return Stmt("destructure_assign_struct", [(0, "Pair { a: %s, b: %s } = %s;" % (a.name, b.name, src))])

    def t_let_pattern(self):
        rng = self.rng
        k = weighted(rng, [("pair", 3), ("pair_ref", 2), ("pair_refmut", 1), ("tuple", 2), ("tuple_ref", 1),
                           ("array", 1), ("holder", 1)])
        if k == "pair":
            p = self.pick(lambda v: v.ty == "P" and v.state == "ok" and self.deps_ok(v) and v.loop == self.loop)
            if not p:
                return None
            if p.moved_fields:
                f = [f for f in "ab" if f not in p.moved_fields]
                if not f:
                    return None
                f = f[0]
                self.touch(p, write=True)
                p.moved_fields.add(f)
                x = self.new_var("S", rng.random() < 0.3)
                return Stmt("pattern_partial_move", [(0, "let Pair { %s: %s, .. } = %s;" % (f, x.name, p.name))])
            if rng.random() < 0.5:
                self.move(p)
                a = self.new_var("S")
                b = self.new_var("S")
                return Stmt("pattern_struct_move", [(0, "let Pair { a: %s, b: %s } = %s;" % (a.name, b.name, p.name))])
            f = rng.choice("ab")
            self.touch(p, write=True)
            p.moved_fields.add(f)
            x = self.new_var("S")
            return Stmt("pattern_partial_move", [(0, "let Pair { %s: %s, .. } = %s;" % (f, x.name, p.name))])
        if k in ("pair_ref", "pair_refmut"):
            mut = k == "pair_refmut"
            p = self.pick(lambda v: v.ty == "P" and self.ok(v) and (v.mut or not mut))
            if not p:
                return None
            f = rng.choice("ab")
            d = self.borrow(p, "mut" if mut else "shr")
            x = self.new_var("MS" if mut else "RS", deps=d)
            if rng.random() < 0.5:
                pat = "let Pair { %s: %s %s, .. } = %s;" % (f, "ref mut" if mut else "ref", x.name, p.name)
            else:
                pat = "let Pair { %s: %s, .. } = &%s%s;" % (f, x.name, "mut " if mut else "", p.name)
            return Stmt("pattern_ref_mut" if mut else "pattern_ref", [(0, pat)])
        if k in ("tuple", "tuple_ref"):
            t = self.pick(lambda v: v.ty == "T" and self.movable(v))
            if not t:
                return None
            if k == "tuple":
                self.move(t)
                x = self.new_var("S", rng.random() < 0.3)
                n = self.new_var("I")
                return Stmt("pattern_tuple_move", [(0, "let (%s%s, %s) = %s;" % ("mut " if x.mut else "", x.name, n.name, t.name))])
            d = self.borrow(t, "shr")
            x = self.new_var("RS", deps=d)
            return Stmt("pattern_ref", [(0, "let (ref %s, _) = %s;" % (x.name, t.name))])
        if k == "array":
            a = self.pick(lambda v: v.ty == "A" and self.movable(v))
            if not a:
                return None
            if rng.random() < 0.5:
                self.move(a)
                x = self.new_var("S")
                y = self.new_var("S")
                return Stmt("pattern_array_move", [(0, "let [%s, %s] = %s;" % (x.name, y.name, a.name))])
            d = self.borrow(a, "shr")
            x = self.new_var("RS", deps=d)
            return Stmt("pattern_slice_ref", [(0, "let [%s, ..] = &%s;" % (x.name, a.name))])
        if k == "holder":
            h = self.pick_ty({"H"})
            if not h:
                return None
            self.touch(h)
            x = self.new_var("RS", deps=list(h.deps))
            return Stmt("pattern_struct_copy_ref", [(0, "let Holder { r: %s, .. } = %s;" % (x.name, h.name))])
        return None

    def t_closure(self):
        rng = self.rng
        k = weighted(rng, [("fn", 3), ("fnmut", 2), ("once", 2), ("movefn", 1)])
        if k == "fn":
            s = self.pick_ty({"S", "RS", "VS", "B"})
            if not s:
                return None
            d = self.borrow(s, "shr") if s.ty != "RS" else list(s.deps)
            c = self.new_var("CF", deps=d)
            return Stmt("closure_borrow", [(0, "let %s = || %s.len();" % (c.name, s.name))])
        if k == "fnmut":
            s = self.pick(lambda v: v.ty in ("S", "VS") and v.mut and self.ok(v))
            if not s:
                return None
            d = self.borrow(s, "mut")
            c = self.new_var("CM", True, d)
            body = "%s.push('!')" % s.name if s.ty == "S" else "%s.push(String::new())" % s.name
            return Stmt("closure_borrow_mut", [(0, "let mut %s = || %s;" % (c.name, body))])
        if k == "once":
            s = self.pick(lambda v: v.ty == "S" and self.movable(v))
            if not s:
                return None
            self.move(s)
            c = self.new_var("CO")
            return Stmt("closure_move_once", [(0, "let %s = move || %s;" % (c.name, s.name))])
        s = self.pick(lambda v: v.ty in ("S", "VS") and self.movable(v))
        if not s:
            return None
        self.move(s)
        c = self.new_var("CMV")
        return Stmt("closure_move_fn", [(0, "let %s = move || %s.len();" % (c.name, s.name))])

    def t_drop(self):
        v = self.pick(lambda v: v.ty in ("S", "VS", "VR", "P", "B", "O", "H", "MS", "T", "A") and self.movable(v))
        if not v:
            return None
        self.move(v)
        return Stmt("drop_call", [(0, "drop(%s);" % v.name)])

    def t_shadow(self):
        rng = self.rng
        v = self.pick(lambda v: v.ty == "S" and self.ok(v) and v.depth == self.depth)
        if not v:
            return None
        if rng.random() < 0.5:
            d = self.borrow(v, "shr")
            v.hidden = True
            n = self.new_var("RS", deps=d, name=v.name)
            return Stmt("shadow_borrow", [(0, "let %s = &%s;" % (n.name, n.name))])
        if v.loop != self.loop:
            return None
        self.move(v)
        v.hidden = True
        n = self.new_var("S", True, name=v.name)
        return Stmt("shadow_move", [(0, "let mut %s = %s;" % (n.name, n.name))])

    def t_let_underscore(self):
        v = self.pick(lambda v: v.ty in ("S", "VS", "P", "B", "O") and self.ok(v))
        if not v:
            return None
        self.touch(v)
        return Stmt("let_underscore_no_move", [(0, "let _ = %s;" % v.name)])

    def t_swap(self):
        rng = self.rng
        ss = [v for v in self.visible() if v.ty == "S" and v.mut and self.ok(v)]
        if len(ss) < 2:
            return None
        a, b = rng.sample(ss, 2)
        self.touch(a, write=True)
        self.touch(b, write=True)
        return Stmt("mem_swap", [(0, "std::mem::swap(&mut %s, &mut %s);" % (a.name, b.name))])

    # ── control flow
    def n_inner(self):
        return self.rng.randint(1, 3)

    def t_if(self):
        rng = self.rng
        c = self.cond()
        base = self.snap()
        b1, e1, _ = self.block(self.n_inner())
        outs = [e1] if e1 else []
        parts = [(0, "if %s {" % c), (1, b1)]
        tags = ["if"]
        k = rng.random()
        if k < 0.25:
            self.restore(base)
            c2 = self.cond()
            b2, e2, _ = self.block(self.n_inner())
            self.restore(base)
            b3, e3, _ = self.block(self.n_inner())
            outs += [e for e in (e2, e3) if e]
            parts += [(0, "} else if %s {" % c2), (1, b2), (0, "} else {"), (1, b3), (0, "}")]
            tags.append("else_if")
        elif k < 0.75:
            self.restore(base)
            b2, e2, _ = self.block(self.n_inner())
            outs += [e2] if e2 else []
            parts += [(0, "} else {"), (1, b2), (0, "}")]
            tags.append("if_else")
        else:
            outs.append(base)
            parts.append((0, "}"))
        self.merge(base, outs)
        alts = [Stmt(tags[:1], parts[:2] + [(0, "}")])] if len(parts) > 3 else []
        return Stmt(tags, parts, inline=rng.random() < 0.3, alts=alts)

    def t_block(self):
        base = self.snap()
        b, e, _ = self.block(self.n_inner() + 1)
        self.merge(base, [e] if e else [])
        return Stmt("inner_block", [(0, "{"), (1, b), (0, "}")])

    def arms(self, specs, scrut_tags):
        """specs: [(head, [(ty, name, mut, deps)])] -> (Arm list, outs)."""
        base = self.snap()
        arms, outs = [], []
        inline = self.rng.random() < 0.35
        for head, binds in specs:
            self.restore(base)
            scope = []
            self.scopes.append(scope)
            vs = [self.new_var(ty, mut, deps, name=name) for (ty, name, mut, deps) in binds]
            self.scopes.pop()
            b, e, _ = self.block(self.rng.randint(0, 2), scope_vars=vs)
            if e:
                outs.append(e)
            arms.append(Arm(head, b, inline))
        self.merge(base, outs)
        return arms

    def t_match(self):
        rng = self.rng
        k = weighted(rng, [("opt", 4), ("opt_ref", 2), ("int", 2), ("str", 1), ("tuple", 1), ("pair", 1),
                           ("slice", 2), ("array", 1)])
        tags = ["match"]
        if k in ("opt", "opt_ref"):
            o = self.pick(lambda v: v.ty == "O" and (self.movable(v) if k == "opt" else self.ok(v)))
            if not o:
                return None
            x = self.fresh("x")
            if k == "opt":
                scrut = o.name
                guard = rng.random() < 0.4
                ref = rng.random() < 0.25
                if ref:
                    d = self.borrow(o, "shr")
                    specs = [("Some(ref %s)" % x + (" if %s.len() > 1" % x if guard else ""), [("RS", x, False, d)])]
                    if guard:
                        specs.append(("Some(_)", []))
                    tags.append("pattern_ref")
                else:
                    self.touch(o, write=True)
                    pre = self.snap()
                    o.state = "moved"
                    specs = [("Some(%s)" % x + (" if %s.len() > 1" % x if guard else ""), [("S", x, False, [])])]
                    if guard:
                        specs.append(("Some(_)", []))
                    tags.append("match_move")
                    arms = self.arms(specs + [("None", [])], tags)
                    # None and Some(_) arms don't move o; the arms merge says maybe.
                    return self._match_stmt(scrut, arms, tags + (["match_guard"] if guard else []))
                if guard:
                    tags.append("match_guard")
                specs.append(("None", []))
            else:
                d = self.borrow(o, "shr")
                scrut = rng.choice(["&%s", "%s.as_ref()"]) % o.name
                specs = [("Some(%s)" % x, [("RS", x, False, d)]), ("None", [])]
                tags.append("match_ref")
            return self._match_stmt(scrut, self.arms(specs, tags), tags)
        if k == "int":
            n = self.pick_ty({"I"})
            if not n:
                return None
            self.touch(n)
            specs = [("0 | 1", []), ("2..=4", []), ("_", [])] if rng.random() < 0.5 else [("1 | 2", []), ("_", [])]
            return self._match_stmt(n.name, self.arms(specs, tags), tags + ["or_pattern"])
        if k == "str":
            s = self.pick_ty({"S", "RS", "B"})
            if not s:
                return None
            self.touch(s)
            specs = [('"hi" | "ab"', []), ("_", [])]
            return self._match_stmt("%s.as_str()" % s.name, self.arms(specs, tags), tags + ["or_pattern"])
        if k == "tuple":
            t = self.pick(lambda v: v.ty == "T" and self.movable(v))
            if not t:
                return None
            x = self.fresh("x")
            if rng.random() < 0.5:
                self.move(t)
                specs = [("(%s, 1)" % x, [("S", x, False, [])]), ("(%s, _)" % (x + "2"), [("S", x + "2", False, [])])]
                self.names.add(x + "2")
                tags.append("match_move")
            else:
                d = self.borrow(t, "shr")
                specs = [("(%s, _)" % x, [("RS", x, False, d)])]
                t_scrut = "&" + t.name
                return self._match_stmt(t_scrut, self.arms(specs, tags), tags + ["match_ref"])
            return self._match_stmt(t.name, self.arms(specs, tags), tags)
        if k == "pair":
            p = self.pick(lambda v: v.ty == "P" and self.movable(v))
            if not p:
                return None
            if rng.random() < 0.5:
                self.move(p)
                a, b = self.fresh("a"), self.fresh("b")
                specs = [("Pair { a: %s, b: %s }" % (a, b), [("S", a, False, []), ("S", b, False, [])])]
                tags.append("match_move")
            else:
                d = self.borrow(p, "shr")
                a = self.fresh("a")
                specs = [("Pair { a: ref %s, .. }" % a, [("RS", a, False, d)])]
                tags.append("pattern_ref")
            return self._match_stmt(p.name, self.arms(specs, tags), tags)
        if k == "slice":
            v = self.pick_ty({"VS"})
            if not v:
                return None
            d = self.borrow(v, "shr")
            x, rest = self.fresh("x"), self.fresh("rest")
            specs = [("[]", []), ("[%s]" % x, [("RS", x, False, d)]),
                     ("[%s, %s @ ..]" % (x + "1", rest), [("RS", x + "1", False, d)])]
            self.names.add(x + "1")
            scrut = rng.choice(["%s.as_slice()", "&%s[..]"]) % v.name
            return self._match_stmt(scrut, self.arms(specs, tags), tags + ["slice_pattern"])
        if k == "array":
            a = self.pick(lambda v: v.ty == "A" and self.movable(v))
            if not a:
                return None
            x, y = self.fresh("x"), self.fresh("y")
            if rng.random() < 0.5:
                self.move(a)
                specs = [("[%s, %s]" % (x, y), [("S", x, False, []), ("S", y, False, [])])]
                return self._match_stmt(a.name, self.arms(specs, tags), tags + ["array_pattern", "match_move"])
            d = self.borrow(a, "shr")
            specs = [("[%s, ..]" % x, [("RS", x, False, d)])]
            return self._match_stmt("&" + a.name, self.arms(specs, tags), tags + ["array_pattern", "match_ref"])
        return None

    def _match_stmt(self, scrut, arms, tags):
        parts = [(0, "match %s {" % scrut)] + [(1, a) for a in arms] + [(0, "}")]
        return Stmt(tags, parts)

    def t_if_let(self):
        rng = self.rng
        k = weighted(rng, [("opt", 3), ("opt_ref", 2), ("opt_mut", 1), ("vfirst", 2), ("slice", 1), ("while", 2)])
        x = self.fresh("x")
        if k == "while":
            v = self.pick(lambda v: v.ty == "VS" and v.mut and self.ok(v))
            if not v:
                return None
            self.touch(v, write=True)
            base = self.snap()
            xv = Var(x, "S", False, [], self.tick, self.depth + 1, self.loop + 1)
            self.types.setdefault(x, "S")
            b, e, brks = self.block(self.n_inner(), loop=True, scope_vars=[xv])
            if not self.loop_ok(base, e):
                self.restore(base)
                return None
            self.merge(base, [base] + ([e] if e else []) + brks)
            return Stmt(["while_let", "loop"], [(0, "while let Some(%s) = %s.pop() {" % (x, v.name)), (1, b), (0, "}")])
        if k == "opt":
            o = self.pick(lambda v: v.ty == "O" and self.movable(v))
            if not o:
                return None
            self.touch(o, write=True)
            scrut, bind, tags = o.name, ("S", []), ["if_let", "if_let_move"]
            moved = o
        elif k == "opt_ref":
            o = self.pick_ty({"O"})
            if not o:
                return None
            scrut = rng.choice(["&%s", "%s.as_ref()"]) % o.name
            bind, tags, moved = ("RS", self.borrow(o, "shr")), ["if_let", "if_let_ref"], None
        elif k == "opt_mut":
            o = self.pick(lambda v: v.ty == "O" and v.mut and self.ok(v))
            if not o:
                return None
            scrut = rng.choice(["&mut %s", "%s.as_mut()"]) % o.name
            bind, tags, moved = ("MS", self.borrow(o, "mut")), ["if_let", "if_let_mut"], None
        elif k == "vfirst":
            v = self.pick_ty({"VS"})
            if not v:
                return None
            scrut = rng.choice(["%s.first()", "%s.last()", "%s.get(0)"]) % v.name
            bind, tags, moved = ("RS", self.borrow(v, "shr")), ["if_let", "method_ret_ref"], None
        else:
            v = self.pick_ty({"VS"})
            if not v:
                return None
            rest = self.fresh("rest")
            d = self.borrow(v, "shr")
            base = self.snap()
            xv = Var(x, "RS", False, d, self.tick, self.depth + 1, self.loop)
            self.types.setdefault(x, "RS")
            b, e, _ = self.block(self.n_inner(), scope_vars=[xv])
            self.merge(base, [base] + ([e] if e else []))
            return Stmt(["if_let", "slice_pattern"], [(0, "if let [%s, %s @ ..] = %s.as_slice() {" % (x, rest, v.name)),
                                                    (1, b), (0, "}")], inline=rng.random() < 0.3)
        base = self.snap()
        if moved:
            moved.state = "moved"
        xv = Var(x, bind[0], False, bind[1], self.tick, self.depth + 1, self.loop)
        self.types.setdefault(x, bind[0])
        b1, e1, _ = self.block(self.n_inner(), scope_vars=[xv])
        outs = [e1] if e1 else []
        parts = [(0, "if let Some(%s) = %s {" % (x, scrut)), (1, b1)]
        if rng.random() < 0.5:
            self.restore(base)
            b2, e2, _ = self.block(self.n_inner())
            outs += [e2] if e2 else []
            parts += [(0, "} else {"), (1, b2), (0, "}")]
            tags.append("if_let_else")
        else:
            outs.append(base)
            parts.append((0, "}"))
        self.merge(base, outs)
        if moved and moved.state == "ok":
            moved.state = "maybe"
        alts = [Stmt(tags[:1], parts[:2] + [(0, "}")])] if len(parts) > 3 else []
        return Stmt(tags, parts, inline=rng.random() < 0.3, alts=alts)

    def diverge(self):
        """A statement that leaves the current block."""
        if self.loop and self.rng.random() < 0.6:
            return self.rng.choice(["break;", "continue;"])
        return "return None;" if self.ret == "opt" else "return;"

    def t_let_else(self):
        rng = self.rng
        k = weighted(rng, [("opt", 3), ("opt_ref", 1), ("vfirst", 2), ("maybe", 1), ("slice", 1)])
        x = self.fresh("x")
        if k == "opt":
            o = self.pick(lambda v: v.ty == "O" and self.movable(v))
            if not o:
                return None
            pat, scrut, ty, deps = "Some(%s)" % x, o.name, "S", []
            self.move(o)
            tags = ["let_else", "let_else_move"]
        elif k == "maybe":
            e, t = self.string_value()
            pat, scrut, ty, deps, tags = "Some(%s)" % x, "maybe(%s)" % e, "S", [], ["let_else", "call_ret_option"]
        elif k == "opt_ref":
            o = self.pick_ty({"O"})
            if not o:
                return None
            pat, scrut, ty, deps = "Some(%s)" % x, "&" + o.name, "RS", self.borrow(o, "shr")
            tags = ["let_else", "let_else_ref"]
        elif k == "vfirst":
            v = self.pick_ty({"VS"})
            if not v:
                return None
            pat, scrut, ty, deps = "Some(%s)" % x, rng.choice(["%s.first()", "%s.last()"]) % v.name, "RS", self.borrow(v, "shr")
            tags = ["let_else", "method_ret_ref"]
        else:
            v = self.pick_ty({"VS"})
            if not v:
                return None
            pat, scrut, ty, deps = "[%s, ..]" % x, "v.as_slice()".replace("v", v.name, 1), "RS", self.borrow(v, "shr")
            tags = ["let_else", "slice_pattern"]
        base = self.snap()
        n = rng.randint(0, 1)
        b, _, _ = self.block(n)
        self.restore(base)
        b.stmts.append(Stmt("diverge", [(0, self.diverge())]))
        self.new_var(ty, rng.random() < 0.2, deps, name=x)
        mut = "mut " if self.scopes[-1][-1].mut else ""
        parts = [(0, "let %s = %s else {" % (pat.replace(x, mut + x, 1), scrut)), (1, b), (0, "};")]
        return Stmt(tags, parts, inline=rng.random() < 0.5)

    def t_let_chain(self):
        rng = self.rng
        x = self.fresh("x")
        o = self.pick(lambda v: v.ty == "O" and self.ok(v))
        v = self.pick_ty({"VS"})
        conds, binds, moved = [], [], None
        if o and rng.random() < 0.6:
            if rng.random() < 0.5 and self.movable(o):
                self.touch(o, write=True)
                conds.append("let Some(%s) = %s" % (x, o.name))
                binds.append(Var(x, "S", False, [], self.tick, self.depth + 1, self.loop))
                moved = o
            else:
                d = self.borrow(o, "shr")
                conds.append("let Some(%s) = &%s" % (x, o.name))
                binds.append(Var(x, "RS", False, d, self.tick, self.depth + 1, self.loop))
            conds.append("%s.len() > 1" % x)
        elif v:
            d = self.borrow(v, "shr")
            conds.append("let Some(%s) = %s.first()" % (x, v.name))
            binds.append(Var(x, "RS", False, d, self.tick, self.depth + 1, self.loop))
        else:
            return None
        if rng.random() < 0.5:
            c = self.cond()
            if c != "true":
                conds.insert(0, c) if rng.random() < 0.5 else conds.append(c)
        if v and rng.random() < 0.4 and not conds[-1].startswith("let Some(%s) = %s.first" % (x, v.name)):
            y = self.fresh("y")
            d = self.borrow(v, "shr")
            conds.append("let Some(%s) = %s.last()" % (y, v.name))
            binds.append(Var(y, "RS", False, d, self.tick, self.depth + 1, self.loop))
        for b in binds:
            self.types.setdefault(b.name, b.ty)
        base = self.snap()
        if moved:
            moved.state = "moved"
        b1, e1, _ = self.block(self.n_inner(), scope_vars=binds)
        outs = [e1] if e1 else []
        parts = [(0, "if %s {" % " && ".join(conds)), (1, b1)]
        if rng.random() < 0.4:
            self.restore(base)
            b2, e2, _ = self.block(self.n_inner())
            outs += [e2] if e2 else []
            parts += [(0, "} else {"), (1, b2), (0, "}")]
        else:
            outs.append(base)
            parts.append((0, "}"))
        self.merge(base, outs)
        if moved and moved.state == "ok":
            moved.state = "maybe"
        return Stmt(["let_chain"] + (["let_chain_move"] if moved else []), parts, inline=rng.random() < 0.2)

    def t_labeled_block(self):
        rng = self.rng
        self.label_n += 1
        lab = "'blk" if self.label_n == 1 else "'blk%d" % self.label_n
        base = self.snap()
        self.scopes.append([])
        self.depth += 1
        stmts = []
        for _ in range(rng.randint(0, 2)):
            s = self.stmt()
            if s:
                stmts.append(s)
            if self.dead:
                break
        c = self.cond()
        brk = self.snap()
        stmts.append(Stmt(["labeled_break", "if"], [(0, "if %s {" % c), (1, Block([Stmt("break", [(0, "break %s;" % lab)])])), (0, "}")], inline=True))
        for _ in range(rng.randint(1, 2)):
            if self.dead:
                break
            s = self.stmt()
            if s:
                stmts.append(s)
        end = None if self.dead else self.snap()
        self.dead = False
        self.depth -= 1
        self.scopes.pop()
        self.merge(base, [brk] + ([end] if end else []))
        return Stmt("labeled_block", [(0, "%s: {" % lab), (1, Block(stmts)), (0, "}")])

    def loop_ok(self, base, end):
        """Loans taken before the loop and used in it must still hold at
        the end of the body (the next iteration uses them again)."""
        if end is None:
            return True
        self_snap = self.snap()
        self.restore(end)
        ok = True
        for key, (v, st, mf, deps, r, w) in base.items():
            if v.r > r and v.deps and not self.deps_ok(v):
                ok = False
        self.restore(self_snap)
        return ok

    def t_loop(self):
        rng = self.rng
        k = weighted(rng, [("loop", 2), ("while", 2), ("for_ref", 2), ("for_move", 1), ("for_range", 2),
                           ("for_iter_mut", 1), ("nested", 1)])
        base = self.snap()
        label = None
        if k == "nested":
            label = "'outer"
            if "'outer" in self.loop_labels:
                return None
        if k == "loop":
            b, e, brks = self.block(self.n_inner(), loop=True)
            br = self.snap() if e else None
            b.stmts.append(Stmt("break", [(0, "break;")]))
            if not self.loop_ok(base, e):
                self.restore(base)
                return None
            self.merge(base, ([br] if br else []) + brks)
            return Stmt("loop", [(0, "loop {"), (1, b), (0, "}")])
        if k == "while":
            n = self.pick(lambda v: v.ty == "I" and v.mut and self.ok(v))
            if not n:
                n = self.new_var("I", True)
                pre = Stmt("let_i32", [(0, "let mut %s = 0;" % n.name)])
                base = self.snap()
            else:
                pre = None
            self.touch(n, write=True)

            def inner(k, _n=n, _first=[True]):
                if _first[0]:
                    _first[0] = False
                    return Stmt("compound_assign", [(0, "%s += 1;" % _n.name)])
                return self.stmt()
            b, e, brks = self.block(self.n_inner() + 1, loop=True, inner=inner)
            if not self.loop_ok(base, e):
                self.restore(base)
                if pre:
                    self.scopes[-1].remove(n)
                return None
            self.merge(base, [base] + ([e] if e else []) + brks)
            st = Stmt(["while", "loop"], [(0, "while %s < 3 {" % n.name), (1, b), (0, "}")])
            return _Seq([pre, st]) if pre else st
        if k in ("for_ref", "for_move", "for_iter_mut"):
            if k == "for_move":
                v = self.pick(lambda v: v.ty in ("VS", "VR") and self.movable(v))
            elif k == "for_iter_mut":
                v = self.pick(lambda v: v.ty == "VS" and v.mut and self.ok(v))
            else:
                v = self.pick_ty({"VS", "VR"})
            if not v:
                return None
            x = self.fresh("e")
            if k == "for_move":
                self.move(v)
                xv = Var(x, "S" if v.ty == "VS" else "STR", False, list(v.deps), self.tick, self.depth + 1, self.loop + 1)
                head = "for %s in %s {" % (x, v.name)
            elif k == "for_iter_mut":
                d = self.borrow(v, "mut")
                xv = Var(x, "MS", False, d, self.tick, self.depth + 1, self.loop + 1)
                head = "for %s in %s.iter_mut() {" % (x, v.name)
            else:
                d = self.borrow(v, "shr")
                xv = Var(x, "RS" if v.ty == "VS" else "STR", False, d, self.tick, self.depth + 1, self.loop + 1)
                head = rng.choice(["for %s in &%s {", "for %s in %s.iter() {"]) % (x, v.name)
            self.types.setdefault(x, xv.ty)
            b, e, brks = self.block(self.n_inner(), loop=True, scope_vars=[xv])
            if not self.loop_ok(base, e):
                self.restore(base)
                return None
            self.merge(base, [base] + ([e] if e else []) + brks)
            return Stmt(["for", k], [(0, head), (1, b), (0, "}")])
        if k == "for_range":
            i = self.fresh("i")
            iv = Var(i, "I", False, [], self.tick, self.depth + 1, self.loop + 1)
            self.types.setdefault(i, "I")
            b, e, brks = self.block(self.n_inner(), loop=True, scope_vars=[iv])
            if not self.loop_ok(base, e):
                self.restore(base)
                return None
            self.merge(base, [base] + ([e] if e else []) + brks)
            return Stmt(["for", "for_range"], [(0, "for %s in 0..2 {" % i), (1, b), (0, "}")])
        # nested labeled loops
        i, j = self.fresh("i"), self.fresh("j")
        self.loop += 1
        self.loop_labels.append(label)
        saved_breaks = self.breaks
        self.breaks = []
        self.depth += 1
        self.scopes.append([Var(i, "I", False, [], self.tick, self.depth, self.loop)])
        self.types.setdefault(i, "I")
        self.types.setdefault(j, "I")
        jv = Var(j, "I", False, [], self.tick, self.depth + 1, self.loop + 1)
        inner, e_in, brks_in = self.block(self.n_inner(), loop=True, scope_vars=[jv])
        c = self.cond()
        outer_brk = self.snap()
        inner.stmts.append(Stmt(["labeled_break", "if"], [(0, "if %s {" % c), (1, Block([Stmt("break", [(0, "break 'outer;")])])), (0, "}")], inline=True))
        self.scopes.pop()
        self.depth -= 1
        brks = self.breaks + [outer_brk]
        self.breaks = saved_breaks
        self.loop -= 1
        self.loop_labels.pop()
        end = self.snap()
        if not self.loop_ok(base, end):
            self.restore(base)
            return None
        self.merge(base, [base, end] + brks)
        return Stmt(["for", "labeled_loop", "nested_loop"],
                    [(0, "'outer: for %s in 0..2 {" % i), (1, Block([Stmt("for", [(0, "for %s in 0..2 {" % j), (1, inner), (0, "}")])])), (0, "}")])

    def t_break_continue(self):
        rng = self.rng
        c = self.cond()
        word = rng.choice(["break", "continue"])
        self.breaks.append(self.snap())
        return Stmt(["if", word], [(0, "if %s {" % c), (1, Block([Stmt(word, [(0, word + ";")])])), (0, "}")],
                    inline=rng.random() < 0.5)

    def t_move_then_break(self):
        """`if c { take(s); break; }`: a move out of a loop that leaves it."""
        rng = self.rng
        c = self.cond()
        base = self.snap()
        self.allow_outer_move = True
        e, t = self.string_value()
        self.allow_outer_move = False
        if t == "lit":
            self.restore(base)
            return None
        call = rng.choice(["take(%s);", "drop(%s);", "take_gen(%s);"]) % e
        word = rng.choice(["break;", self.diverge() if not self.diverge().startswith("continue") else "break;"])
        if word == "break;":
            self.breaks.append(self.snap())
        self.restore(base)
        return Stmt(["if", "move_then_break"], [(0, "if %s {" % c), (1, Block([Stmt("call_move", [(0, call)]), Stmt("break", [(0, word)])])), (0, "}")],
                    inline=rng.random() < 0.4)

    def t_return(self):
        rng = self.rng
        if self.ret == "unit":
            word = "return;"
        else:
            v = self.pick_ty({"S", "RS", "VS", "MS"})
            if v and rng.random() < 0.5:
                self.touch(v)
                word = "return Some(%s.len());" % v.name
            else:
                word = "return None;"
        c = self.cond()
        return Stmt(["if", "early_return"], [(0, "if %s {" % c), (1, Block([Stmt("return", [(0, word)])])), (0, "}")],
                    inline=rng.random() < 0.4)

    def t_try(self):
        rng = self.rng
        k = weighted(rng, [("opt", 2), ("vfirst", 2), ("maybe", 1), ("len_opt", 1), ("last_mut", 1), ("as_mut", 1)])
        if k == "opt":
            o = self.pick(lambda v: v.ty == "O" and self.movable(v))
            if not o:
                return None
            self.move(o)
            x = self.new_var("S", rng.random() < 0.3)
            return Stmt(["try", "try_move"], [(0, "let %s%s = %s?;" % ("mut " if x.mut else "", x.name, o.name))])
        if k == "vfirst":
            v = self.pick_ty({"VS"})
            if not v:
                return None
            d = self.borrow(v, "shr")
            x = self.new_var("RS", deps=d)
            return Stmt(["try", "method_ret_ref"], [(0, "let %s = %s?;" % (x.name, rng.choice(["%s.first()", "%s.last()"]) % v.name))])
        if k == "maybe":
            e, t = self.string_value()
            x = self.new_var("S")
            return Stmt(["try", "call_ret_option"], [(0, "let %s = maybe(%s)?;" % (x.name, e))])
        if k == "len_opt":
            r = self.ref_value()
            if not r:
                return None
            x = self.new_var("I")
            return Stmt(["try", "call_ref"], [(0, "let %s = len_opt(%s)?;" % (x.name, r[0]))])
        if k == "last_mut":
            v = self.pick(lambda v: v.ty == "VS" and v.mut and self.ok(v))
            if not v:
                return None
            d = self.borrow(v, "mut")
            x = self.new_var("MS", deps=d)
            return Stmt(["try", "method_last_mut"], [(0, "let %s = %s.last_mut()?;" % (x.name, v.name))])
        o = self.pick(lambda v: v.ty == "O" and v.mut and self.ok(v))
        if not o:
            return None
        self.touch(o, write=True)
        return Stmt(["try", "method_ret_mut"], [(0, "%s.as_mut()?.push('!');" % o.name)])

    def t_let_branchy(self):
        """`let x = if c { s } else { .. };`, a labeled block or a match as
        the value of a `let` (a conditional move)."""
        rng = self.rng
        k = weighted(rng, [("if", 3), ("label", 2), ("match", 2)])
        base = self.snap()
        if k == "if":
            c = self.cond()
            base = self.snap()
            self.scopes.append([]); self.depth += 1
            pre = self.stmt() if rng.random() < 0.3 else None
            e1, _ = self.string_value()
            self.scopes.pop(); self.depth -= 1
            o1 = self.snap()
            self.restore(base)
            e2, _ = self.string_value()
            o2 = self.snap()
            self.merge(base, [o1, o2])
            x = self.new_var("S", rng.random() < 0.3)
            b1 = Block([pre] if pre else [], tail=e1)
            parts = [(0, "let %s%s = if %s {" % ("mut " if x.mut else "", x.name, c)), (1, b1), (0, "} else {"), (1, Block([], tail=e2)), (0, "};")]
            return Stmt(["let_if_value", "if_else"], parts, inline=not pre and rng.random() < 0.5)
        if k == "label":
            self.label_n += 1
            lab = "'blk" if self.label_n == 1 else "'blk%d" % self.label_n
            c = self.cond()
            base = self.snap()
            e1, t = self.string_value()
            o1 = self.snap()
            self.restore(base)
            e2, _ = self.string_value()
            o2 = self.snap()
            self.merge(base, [o1, o2])
            x = self.new_var("S")
            inner = Block([Stmt(["labeled_break", "if"], [(0, "if %s {" % c), (1, Block([Stmt("break_value", [(0, "break %s %s;" % (lab, e1))])])), (0, "}")], inline=True)], tail=e2)
            return Stmt(["labeled_block_value", "labeled_block"], [(0, "let %s = %s: {" % (x.name, lab)), (1, inner), (0, "};")])
        o = self.pick(lambda v: v.ty == "O" and self.movable(v))
        if not o:
            return None
        self.move(o)
        y = self.fresh("y")
        x = self.new_var("S")
        guard = rng.random() < 0.3
        arms = [Arm("Some(%s)%s" % (y, " if %s.len() > 1" % y if guard else ""), Block([], tail=y), True)]
        if guard:
            arms.append(Arm("Some(_)", Block([], tail="String::new()"), True))
        arms.append(Arm("None", Block([], tail="String::new()"), True))
        parts = [(0, "let %s = match %s {" % (x.name, o.name))] + [(1, a) for a in arms] + [(0, "};")]
        return Stmt(["let_match_value", "match", "match_move"] + (["match_guard"] if guard else []), parts)

    def t_let_deferred(self):
        """`let x; if c { x = ..; } else { x = ..; }`."""
        rng = self.rng
        c = self.cond()
        x = self.new_var("S", state="uninit")
        base = self.snap()
        e1, _ = self.string_value()
        o1 = self.snap()
        self.restore(base)
        e2, _ = self.string_value()
        o2 = self.snap()
        self.merge(base, [o1, o2])
        x.state = "ok"
        self.touch(x, write=True)
        st = Stmt(["let_deferred_init", "if_else"], [(0, "if %s {" % c), (1, Block([Stmt("assign", [(0, "%s = %s;" % (x.name, e1))])])),
                                                    (0, "} else {"), (1, Block([Stmt("assign", [(0, "%s = %s;" % (x.name, e2))])])), (0, "}")],
                   inline=rng.random() < 0.3)
        return _Seq([Stmt("let_uninit", [(0, "let %s;" % x.name)]), st])

    # ── whole programs
    def program(self, seed=None):
        rng = self.rng
        params = []
        if self.fn_kind == "run":
            for ty, base in [("S", "s"), ("RS", "r"), ("MS", "m"), ("O", "o"), ("VS", "v")]:
                if rng.random() < 0.45:
                    mut = ty in ("S", "O", "VS") and rng.random() < 0.6
                    v = self.new_var(ty, mut)
                    params.append("%s%s: %s" % ("mut " if mut else "", v.name, RUST_TY[ty]))
            params.append("flag: bool")
            self.types["flag"] = "bool"
        stmts = []
        # A String or two, and one other kind of value, to work with.
        first = ["S"] * rng.randint(1, 2) + [rng.choice(["S", "VS", "VR", "H", "P", "P", "B", "O", "O", "T", "A"])]
        for ty in first:
            s = self.t_let_owned(ty)
            if s:
                stmts.append(s)
        n = len(stmts) + rng.randint(3, 8)
        tries = 0
        while len(stmts) < n and not self.dead and tries < 50:
            tries += 1
            s = self.stmt()
            if s is None:
                continue
            if isinstance(s, _Seq):
                stmts.extend(s.items)
            else:
                stmts.append(s)
        body = Block(flatten(stmts))
        if self.fn_kind == "run":
            v = self.pick_ty({"S", "RS", "VS", "MS"})
            tail = "Some(%s.len())" % v.name if v and rng.random() < 0.5 else "Some(0)"
            if self.dead:
                tail = None
            return Program("fn run(PARAMS) -> Option<usize> {", body, tail, params, dict(self.types), seed)
        return Program("fn main() {", body, None, [], dict(self.types), seed)


class _Seq:
    """Several statements a template returns together (`let x;` + its
    initializing `if`)."""
    def __init__(self, items):
        self.items = items


def flatten(stmts):
    out = []
    for s in stmts:
        if isinstance(s, _Seq):
            out.extend(flatten(s.items))
        else:
            for _, p in s.parts:
                if isinstance(p, Block):
                    p.stmts = flatten(p.stmts)
                elif isinstance(p, Arm):
                    p.block.stmts = flatten(p.block.stmts)
            out.append(s)
    return out


def weighted(rng, opts):
    total = sum(w for _, w in opts)
    x = rng.random() * total
    for item, w in opts:
        x -= w
        if x < 0:
            return item
    return opts[-1][0]


def generate(seed, index):
    """The program for (seed, index); the same every time."""
    g = Gen(seed, index)
    return g.program(seed="%s:%s" % (seed, index))
