// User-defined `macro_rules!` macros are currently invisible to
// the plugin (issue #137) — the timeline shows the user variable's
// binding and out-of-scope but no event at the macro call site.
// `descend_through_expansion` walks past the macro wrappers and
// re-dispatches user-spanned subexpressions, but a bare path
// reference (the `$x` substitution) only updates the RAP's
// lifetime and doesn't emit a print/borrow event.
//
// This fixture pins the no-crash baseline so the plugin keeps
// surviving user-defined macros even after the eventual fix.

macro_rules! show_twice {
    ($x:expr) => {{
        println!("{}", $x);
        println!("{}", $x);
    }};
}

fn main() {
    let s = String::from("hi");
    show_twice!(s);
}
