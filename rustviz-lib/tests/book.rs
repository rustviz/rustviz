//! Rust Book–style regression corpus: `rustviz-plugin/tests/book/*.rs`.
//!
//! 127 short programs in the style of The Rust Programming Language
//! (chapters 4–15, plus a few extras), each reviewed by hand against real
//! Rust semantics. The table below records, per snippet, what the plugin
//! on `main` gets right and what it gets wrong, so that
//!
//!   * a snippet that renders today keeps rendering (well-formed SVG, every
//!     element inside the canvas), and keeps the correct tooltips listed in
//!     `must_contain`;
//!   * a false tooltip that a fix removed stays removed (`must_not_contain`);
//!   * a fix is *noticed*: when a `known_wrong` tooltip disappears, or a
//!     known crash starts rendering, the test prints a note asking you to
//!     update the table (it does not fail).
//!
//! See `rustviz-plugin/tests/book/README.md` for the categories and how to
//! update the table when a fix lands.
//!
//! Tooltip strings are compared after stripping markup and collapsing
//! whitespace. A string written as `"L13 text"` only matches a tooltip
//! drawn on source line 13 (`"L4+ text"` for the unnumbered row after
//! line 4); use it when the same text is right on one line and wrong on
//! another.
//!
//! Environment:
//!   * `RV_BOOK_ONLY=c4_,m_1` runs only snippets whose id starts with one
//!     of the comma-separated prefixes.
//!   * `RV_BOOK_DUMP=<dir>` writes `<dir>/<id>.txt` with every tooltip as
//!     `L<line> <text>`, the format used in this file.
//!   * `RV_BOOK_JOBS=<n>` sets the number of parallel plugin runs (default 4).
//!
//! Like `corpus.rs`, this runs the *installed* plugin (`cargo rv-plugin`).

mod common;

use common::svg::attr;
use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use rustviz_lib::Rustviz;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Status {
    /// Renders, and every tooltip reviewed is true.
    Correct,
    /// Renders, but shows something false about Rust (`known_wrong`).
    Wrong,
    /// Renders and nothing shown is false, but an event is missing or drawn
    /// off-canvas.
    Missing,
    /// The plugin panics on this snippet today. (No snippet crashes on main
    /// right now; the variant stays for the next one that does.)
    #[allow(dead_code)]
    Crash,
}
use Status::*;

struct Snippet {
    /// File stem in `rustviz-plugin/tests/book/`; the prefix is the probe id.
    id: &'static str,
    status: Status,
    /// What's wrong or missing, and where it's tracked.
    note: &'static str,
    /// Correct tooltips that must stay. Missing one fails the test.
    must_contain: &'static [&'static str],
    /// False tooltips that a fix removed. Seeing one again fails the test.
    must_not_contain: &'static [&'static str],
    /// False tooltips that main still prints. When one disappears the test
    /// prints a note: move it to `must_not_contain`, and add the corrected
    /// text to `must_contain`.
    known_wrong: &'static [&'static str],
    /// Some elements are drawn below the SVG's height today. When they come
    /// back on canvas the test prints a note asking to clear this flag.
    off_canvas: bool,
}

const E: Snippet = Snippet {
    id: "",
    status: Correct,
    note: "",
    must_contain: &[],
    must_not_contain: &[],
    known_wrong: &[],
    off_canvas: false,
};

/// Old branch-merge wording: Rust drops at the end of the enclosing scope
/// (with a runtime drop flag), not at the end of the branch (#179). A fix
/// removed it; it must not come back.
const JOIN_OPT: &str = "opt was moved in at least one branch above; in branches where it was not, its resource is dropped at the branch's end.";

const BOOK: &[Snippet] = &[
    // ─── Chapter 4: ownership, references, slices ─────────────────────
    Snippet {
        id: "c4_01_clone",
        must_contain: &[
            "s1 is passed to clone by shared reference (as self, implicitly borrowed). clone may read it",
            "Move from clone to s2",
            "s1 goes out of scope. Its resource is dropped.",
            "s2 goes out of scope. Its resource is dropped.",
        ],
        ..E
    },
    Snippet {
        id: "c4_02_copy_tuple",
        must_contain: &["Copy from p to q", "Copy from p.0 to q.0", "Copy from p.1 to q.1"],
        ..E
    },
    Snippet {
        id: "c4_03_move_inner_block",
        must_contain: &[
            "Move from s1 to s2",
            "L6 s2 goes out of scope. Its resource is dropped.",
            "s1 goes out of scope. No resource is dropped.",
        ],
        ..E
    },
    Snippet {
        id: "c4_04_move_let_plain",
        must_contain: &[
            "Move from s1 to s2",
            "s1 goes out of scope. No resource is dropped.",
            "s2 goes out of scope. Its resource is dropped.",
        ],
        ..E
    },
    Snippet {
        id: "c4_05_gives_ownership",
        must_contain: &[
            "some_string's resource is moved to the caller",
            "some_string goes out of scope. No resource is dropped.",
            "Move from gives_ownership to s1",
        ],
        ..E
    },
    Snippet {
        id: "c4_06_nll_shared_then_mut",
        must_contain: &[
            "Immutable borrow from s to r1",
            "Immutable borrow from s to r2",
            "Mutable borrow from s to r3",
            "r3 holds a mutable reference; it can read and write the borrowed resource",
            "r3's mutable borrow of s ends. This borrow no longer restricts access to s. Ending the borrow doesn't move the resource",
            "s goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &["r3 is the owner of the resource", "Return mutably borrowed resource from r3 to s"],
        ..E
    },
    Snippet {
        id: "c4_07_first_word_loop",
        must_contain: &[
            "s is an immutable borrow from the caller",
            "L2 Immutable borrow from s to bytes: as_bytes's signature gives the reference it returns the same lifetime as the reference passed as its receiver",
            // The loop's iterator reads through bytes until the loop's
            // `}` (#1081).
            "L3 the value bytes points to is immutably borrowed by the `for` loop's iterator, through a copy of bytes, until the loop ends",
            "L7 the `for` loop's iterator no longer borrows the value bytes points to",
            "L7 If bytes holds a shared borrow affecting the data behind s, that borrow ends here. It no longer restricts access to the data behind s",
            "word receives the value first_word() returns",
            // `==` on u8 copies item; it isn't an actor (#908).
            "L4 item's value is copied",
        ],
        must_not_contain: &[
            "Copy from item to ==",
            "===",
            "bytes is the owner of the resource",
            "Copy from as_bytes to bytes",
            "L2 s's resource is no longer immutably borrowed",
            "L2 as_bytes no longer borrows the resource s points to",
            "the resource bytes points to",
            "L3 iter no longer borrows the value bytes points to",
            "L3 bytes's shared borrow of the data behind s ends",
            "L3 bytes's immutable borrow ends",
        ],
        ..E
    },
    Snippet {
        id: "c4_08_string_slices",
        must_contain: &[
            "Immutable borrow from s to hello",
            "Immutable borrow from s to world",
            "If hello holds a shared borrow affecting s, that borrow ends here. It no longer restricts access to s",
            "If world holds a shared borrow affecting s, that borrow ends here. It no longer restricts access to s",
        ],
        must_not_contain: &["hello is the owner of the resource", "world is the owner of the resource"],
        ..E
    },
    Snippet {
        id: "c4_09_first_word_str",
        must_contain: &[
            "s is an immutable borrow from the caller",
            "a reference to s[0..i], reborrowed through s, is returned to the caller; the data it points to is not copied",
            "a reference to s[..], reborrowed through s, is returned to the caller; the data it points to is not copied",
            "L13 word holds an immutable reference",
            "L13 Immutable borrow from my_string to word: first_word's signature gives the reference it returns the same lifetime as its argument",
            "L14 If word holds a shared borrow affecting my_string, that borrow ends here. It no longer restricts access to my_string",
            "my_string goes out of scope. Its resource is dropped.",
            // `==` on u8 copies item; it isn't an actor (#908).
            "L4 item's value is copied",
            // The loop's iterator reads through bytes until the loop's
            // `}` (#1081).
            "L3 the value bytes points to is immutably borrowed by the `for` loop's iterator, through a copy of bytes, until the loop ends",
            "L7 the `for` loop's iterator no longer borrows the value bytes points to",
            "L7 If bytes holds a shared borrow affecting the data behind s, that borrow ends here. It no longer restricts access to the data behind s",
        ],
        must_not_contain: &[
            "Copy from item to ==",
            "===",
            "bytes is the owner of the resource",
            "word is the owner of the resource",
            "s's resource is moved to the caller",
            "word's shared borrow of *word ends",
            "the resource word points to",
            "Copy from first_word to word",
            "L13 my_string's resource is no longer immutably borrowed",
            // `&s[0..i]` is a reference to part of the str s points to,
            // reborrowed through s, not s's reference (#1071).
            "a reference from s is returned to the caller",
            "L3 iter no longer borrows the value bytes points to",
            "L3 bytes's shared borrow of the data behind s ends",
            "L3 bytes's immutable borrow ends",
        ],
        ..E
    },
    Snippet {
        id: "c4_10_str_literal_copy",
        must_contain: &[
            "s is bound to an immutable reference",
            "Copy from s to t",
            "t is initialized with a copy of s",
            "s's reference is copied (not the value it points to)",
            "s's borrow of the string literal ends (string literals live for the whole program)",
            "t's borrow of the string literal ends (string literals live for the whole program)",
        ],
        must_not_contain: &[
            "s's immutable borrow ends (what it borrows from isn't shown)",
            "t's immutable borrow ends (what it borrows from isn't shown)",
            "s is the owner of the resource",
            "s acquires ownership of a resource",
            "s's resource is copied",
            "s's shared borrow of *s ends",
            "t's shared borrow of *t ends",
        ],
        ..E
    },
    Snippet {
        id: "c4_11_array_slice",
        status: Correct,
        must_contain: &[
            "Immutable borrow from a to slice",
            "If slice holds a shared borrow affecting a, that borrow ends here. It no longer restricts access to a",
            "L2 a is bound to a copyable value",
            "a goes out of scope",
        ],
        must_not_contain: &["a goes out of scope. No resource is dropped.", "Return immutably borrowed resource from slice to a"],
        ..E
    },
    Snippet {
        id: "c4_12_move_then_reassign",
        must_contain: &[
            "s acquires ownership from the caller",
            "Move from s to takes",
            "L8 Move from String::from to s",
            "L10 s goes out of scope. Its resource is dropped.",
        ],
        ..E
    },
    Snippet {
        id: "c4_13_change_then_use",
        must_contain: &[
            "some_string is a mutable borrow from the caller",
            "some_string holds a mutable reference; it can read and write the borrowed resource",
            "A mutable reference through some_string is passed to push_str (as self). push_str may read or write through it (mutable reborrow — some_string can't be used until push_str returns)",
            "s is passed to change by mutable reference. change may read or write it",
            "s goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &["some_string is the owner of the resource"],
        ..E
    },
    Snippet {
        id: "c4_14_partial_move_field",
        note: "nit: \"p.age is the owner of the resource\" for a u32 field",
        must_contain: &[
            "Move from p.name to n",
            "n goes out of scope. Its resource is dropped.",
            "p.name goes out of scope. No resource is dropped.",
        ],
        ..E
    },
    Snippet {
        id: "c4_15_mutate_copy_int",
        must_contain: &[
            "x's value is copied to the caller",
            "Copy from a to add_one",
            "b receives the value add_one() returns",
        ],
        must_not_contain: &["x's resource is moved to the caller", "x's resource is copied to the caller"],
        ..E
    },
    // ─── Chapter 5: structs and methods ───────────────────────────────
    Snippet {
        id: "c5_01_struct_field_read",
        note: "nit: bool/u64 fields \"acquire ownership of a resource\"",
        must_contain: &[
            "Move from String::from to user1.username",
            "Move from String::from to user1.email",
            "user1.username goes out of scope. Its resource is dropped.",
            "user1.email goes out of scope. Its resource is dropped.",
        ],
        ..E
    },
    Snippet {
        id: "c5_02_struct_field_assign",
        must_contain: &[
            "user1.email acquires ownership of a new resource; its previous resource is dropped",
            "user1.username goes out of scope. Its resource is dropped.",
            "user1.email goes out of scope. Its resource is dropped.",
        ],
        ..E
    },
    Snippet {
        id: "c5_03_build_user_shorthand",
        must_contain: &[
            "email acquires ownership from the caller",
            "username's resource is moved to the caller",
            "email's resource is moved to the caller",
            "email goes out of scope. No resource is dropped.",
            "username goes out of scope. No resource is dropped.",
            "Move from build_user to u",
            "u.username goes out of scope. Its resource is dropped.",
            "u.email goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &[
            "=email goes out of scope. Its resource is dropped.",
            "=username goes out of scope. Its resource is dropped.",
            "u.username goes out of scope. No resource is dropped.",
            "u.email goes out of scope. No resource is dropped.",
        ],
        ..E
    },
    Snippet {
        id: "c5_04_struct_update",
        must_contain: &[
            "Move from String::from to user2.email",
            "Move from user1.username to user2.username",
            "user1.username goes out of scope. No resource is dropped.",
            "user2.username goes out of scope. Its resource is dropped.",
            "user1.email goes out of scope. Its resource is dropped.",
            "user2.email goes out of scope. Its resource is dropped.",
            "user1 goes out of scope. user1.username was moved out earlier, so only user1.email is dropped.",
        ],
        must_not_contain: &[
            "user1.username goes out of scope. Its resource is dropped.",
            "user2.username goes out of scope. No resource is dropped.",
            "user1 goes out of scope. Its resource is dropped.",
        ],
        ..E
    },
    Snippet {
        id: "c5_05_tuple_struct",
        note: "was a crash at c2037dd (`black.0`)",
        must_contain: &[
            "black is initialized with a Color value",
            "Copy from black.0 to r",
            "println! borrows black.2 for formatting through an implicit shared reference, so black.2 isn't moved",
            "black goes out of scope",
        ],
        // #873: `Color(0, 0, 0)` builds a new value; the constructor is
        // not an actor.
        must_not_contain: &["black goes out of scope. Its resource is dropped.", "Move from Color to black", "=Color"],
        ..E
    },
    Snippet {
        id: "c5_06_method_mut_self",
        status: Missing,
        note: "was a crash at c2037dd. `self.n += 1` through &mut self isn't drawn: c.n's column shows no write (#147)",
        must_contain: &[
            "self is a mutable borrow from the caller",
            "self holds a mutable reference; it can read and write the borrowed value",
            "c is passed to inc by mutable reference (as self, implicitly borrowed). inc may read or write it",
            "c goes out of scope",
        ],
        must_not_contain: &[
            "c goes out of scope. Its resource is dropped.",
            "self holds a mutable reference; it can read and write the borrowed resource",
        ],
        ..E
    },
    Snippet {
        id: "c5_07_method_self_consume",
        must_contain: &[
            "self acquires ownership from the caller",
            "self.name's resource is moved to the caller",
            "self.name goes out of scope. No resource is dropped.",
            "Move from p to into_name",
            "Move from into_name to n",
            "p goes out of scope. No resource is dropped.",
            "p.name goes out of scope. No resource is dropped.",
            "self goes out of scope. self.name was moved out earlier, so no resource is dropped.",
        ],
        must_not_contain: &[
            "p goes out of scope. Its resource is dropped.",
            "p.name goes out of scope. Its resource is dropped.",
            "self goes out of scope. Its resource is dropped.",
        ],
        ..E
    },
    Snippet {
        id: "c5_08_assoc_fn_square",
        must_contain: &["Move from Rectangle::square to sq", "sq goes out of scope"],
        must_not_contain: &["sq goes out of scope. Its resource is dropped."],
        ..E
    },
    Snippet {
        id: "c5_09_can_hold",
        must_contain: &[
            "self is an immutable borrow from the caller",
            "other is an immutable borrow from the caller",
            "rect1 is passed to can_hold by shared reference (as self, implicitly borrowed). can_hold may read it",
            "rect2 is passed to can_hold by shared reference. can_hold may read it",
        ],
        must_not_contain: &["self is the owner of the resource", "other is the owner of the resource"],
        ..E
    },
    Snippet {
        id: "c5_10_one_field_getter",
        must_contain: &[
            "self is an immutable borrow from the caller",
            "c is passed to get by shared reference (as self, implicitly borrowed). get may read it",
            "v receives the value get() returns",
        ],
        must_not_contain: &["self is the owner of the resource"],
        ..E
    },
    Snippet {
        id: "c5_11_derive_debug",
        must_contain: &[
            "rect1 is bound to a value",
            "println! borrows rect1 for formatting through an implicit shared reference, so rect1 isn't moved",
            "rect1 goes out of scope",
        ],
        must_not_contain: &[
            "rect1 goes out of scope. Its resource is dropped.",
            "rect1 acquires ownership of a resource",
            "self is an immutable borrow from the caller",
            "f holds a mutable reference",
            "f is a mutable borrow from the caller",
        ],
        ..E
    },
    Snippet {
        id: "c5_12_dbg_macro",
        must_contain: &[
            "scale is bound to a copyable value",
            "rect1 is passed to dbg! by shared reference. dbg! may read it",
            "rect1 goes out of scope",
        ],
        must_not_contain: &[
            "rect1 goes out of scope. Its resource is dropped.",
            "self is an immutable borrow from the caller",
            "f holds a mutable reference",
            "f is a mutable borrow from the caller",
        ],
        ..E
    },
    // ─── Chapter 6: enums and match ───────────────────────────────────
    Snippet {
        id: "c6_01_enum_data",
        must_contain: &[
            "Move from home to _a",
            "Move from loopback to _b",
            "loopback goes out of scope. No resource is dropped.",
            "_b goes out of scope. Its resource is dropped (including the String it holds, if any).",
            "home acquires ownership of a resource",
        ],
        // #873: `IpAddr::V4(127, 0, 0, 1)` builds a new value; the
        // constructor is not an actor.
        must_not_contain: &["Move from IpAddr::V4 to home", "=IpAddr::V4"],
        ..E
    },
    Snippet {
        id: "c6_02_match_coin",
        must_contain: &[
            "Move from c to value_in_cents",
            "coin goes out of scope",
            "v receives the value value_in_cents() returns",
            "c goes out of scope",
            "c is initialized with a Dime value",
        ],
        must_not_contain: &[
            // #873: the constructor is not an actor.
            "Move from Coin::Dime to c",
            "=Coin::Dime",
            "c goes out of scope. No resource is dropped.",
            "coin goes out of scope. Its resource is dropped.",
        ],
        ..E
    },
    Snippet {
        id: "c6_03_plus_one",
        must_contain: &["Partial copy from x to i", "Copy from five to plus_one", "six receives the value plus_one() returns"],
        ..E
    },
    Snippet {
        id: "c6_04_match_moves_string",
        must_contain: &["Partial move from opt to s", "s goes out of scope. Its resource is dropped.", "opt may be partially moved above, so it can't be used as a whole after the branches. If execution reaches the end of opt's scope while it still holds its resource (including the String it holds, if any), it is dropped there.", "opt goes out of scope. No value inside opt remains to drop here. No resource is dropped. None carries no payload. For Some, the String payload was moved out."],
        must_not_contain: &["payload was moved out. Rust tracks the move with a runtime drop flag.", JOIN_OPT, "opt was moved in at least one branch above,"],
        ..E
    },
    Snippet {
        id: "c6_05_match_on_ref",
        must_contain: &[
            "Partial immutable borrow from opt to s",
            "s's shared borrow of the value inside opt ends. This borrow no longer restricts access to opt",
            "opt goes out of scope. Its resource is dropped (including the String it holds, if any).",
            "L4+ s's immutable borrow ends",
            "L4+ opt's resource is no longer immutably borrowed",
        ],
        must_not_contain: &["s's shared borrow of opt ends", "L4 s's immutable borrow ends", "L4 opt's resource is no longer immutably borrowed"],
        ..E
    },
    Snippet {
        id: "c6_06_if_let_copy",
        // max copies the u8 inside config_max, not all of it (#866).
        must_contain: &[
            "Partial copy from config_max to max",
            "max is initialized with a copy of a value inside config_max",
            "a value inside config_max is copied",
        ],
        must_not_contain: &[
            "max is initialized with a copy of config_max",
            "config_max's value is copied",
        ],
        ..E
    },
    Snippet {
        id: "c6_07_if_let_else_string",
        note: "the None path has no payload, and the Some path moves its payload to n",
        must_contain: &["Move from name to n", "n goes out of scope. Its resource is dropped.", "name may be partially moved above, so it can't be used as a whole after the branches. If execution reaches the end of name's scope while it still holds its resource (including the String it holds, if any), it is dropped there.", "name goes out of scope. No value inside name remains to drop here. No resource is dropped. None carries no payload. For Some, the String payload was moved out."],
        must_not_contain: &["payload was moved out. Rust tracks the move with a runtime drop flag.",
            "name was moved in at least one branch above,",
            "name was moved in at least one branch above; in branches where it was not, its resource is dropped at the branch's end.",
            "name goes out of scope. No resource is dropped.",
        ],
        ..E
    },
    Snippet {
        id: "c6_08_let_else",
        must_contain: &[
            // ElaborateDrops emits only discriminant checks for s at these exits.
            "L3 s goes out of scope. No value inside s remains to drop here. No resource is dropped. None carries no payload.",
            "L6 s goes out of scope. A value inside s was moved out earlier. No value inside s remains to drop here. No resource is dropped. For Some, the String payload was moved out.",
            "The following applies only if the pattern matches. Move from s to x",
            "The following applies only if the pattern matches. x acquires ownership of a resource",
            "x is the owner of the resource",
            "x's resource is moved to the caller",
            "x goes out of scope. No resource is dropped.",
            "Move from describe to d",
            "d goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &[
            "L3 s goes out of scope. Its resource is dropped",
            "runtime drop flag",
            "The following applies only if the pattern matches. x is the owner of the resource",
            "The following applies only if the pattern matches. x's resource is moved to the caller",
            "The following applies only if the pattern matches. x goes out of scope",
            // The stroke into the move mark still depicts the whole Option; its payload moves at the mark.
            "Part of s was moved out, so s can't be used as a whole",
        ],
        ..E
    },
    Snippet {
        id: "c6_09_option_unwrap_or",
        must_contain: &["y receives the value unwrap_or() returns", "x is passed to is_some by shared reference (as self, implicitly borrowed). is_some may read it"],
        ..E
    },
    Snippet {
        id: "c6_10_match_guard",
        must_contain: &[
            "Copy from n to x",
            "msg is bound to an immutable reference (in all branches above)",
            "L8 msg's borrow of the string literal ends (string literals live for the whole program)",
        ],
        must_not_contain: &[
            "L8 msg's immutable borrow ends (what it borrows from isn't shown)",
            "msg is the owner of the resource",
            "msg acquires ownership of a resource",
            "msg acquired ownership of a resource (in all branches above)",
            "msg's shared borrow of *msg ends",
        ],
        ..E
    },
    Snippet {
        id: "c6_11_enum_method",
        must_contain: &[
            "L9 self's immutable borrow ends",
            "L10 Partial immutable borrow from self to s",
            "m is passed to call by shared reference (as self, implicitly borrowed). call may read it",
            "m goes out of scope. Its resource is dropped (including the String it holds, if any).",
            "L10+ s no longer borrows the resource self points to",
            "L10+ s's immutable borrow ends",
            "L10+ self's immutable borrow ends",
        ],
        must_not_contain: &[
            "L10 self's resource is no longer immutably borrowed",
            "L10 s's immutable borrow ends",
            "L10 s no longer borrows the resource self points to",
            // #873: `Message::Quit` builds a new value; not an actor.
            "Move from Message::Quit to q",
            "=Message::Quit",
        ],
        ..E
    },
    // ─── Chapter 8: collections ───────────────────────────────────────
    Snippet {
        id: "c8_01_vec_new_push",
        must_contain: &[
            "Move from Vec::new to v",
            "v is passed to push by mutable reference (as self, implicitly borrowed). push may read or write it",
            "v goes out of scope. Its resource is dropped.",
        ],
        ..E
    },
    Snippet {
        id: "c8_02_vec_index_ref",
        must_contain: &[
            "Immutable borrow from v to third",
            "If third holds a shared borrow affecting v, that borrow ends here. It no longer restricts access to v",
        ],
        must_not_contain: &["third is the owner of the resource"],
        ..E
    },
    Snippet {
        id: "c8_03_vec_get_match",
        status: Correct,
        must_contain: &[
            "get takes a shared reference (as self, implicitly borrowed). It may read through that reference",
            "L3 Immutable borrow from v to third: get's signature gives the value it returns the same lifetime as the reference passed as its receiver; third's value can hold a reference (third isn't a reference itself)",
            "L5 third₂ is initialized with a copy of a value inside third",
            "L5 a value inside third is copied",
            "L5 Immutable borrow from v to third₂: third₂ is a reference copied out of third",
            "L5 After the reference is copied out of third, the shared borrow of v continues in third₂",
            "L7 This segment leads from the None branch to where the branches join. v is the owner of the resource",
            "L7 This segment leads from the None branch to where the branches join. third owns a value that can hold references (third isn't a reference itself). third holds a copyable value",
            // One line for the end and the copy's borrow (#1450).
            "L5 v's resource is no longer borrowed by third, but is still immutably borrowed by third₂; v is read-only while the borrow is live",
            "v goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &[
            "third's shared borrow of third ends",
            "L6 Return immutably borrowed resource from third to v",
            "After the reference is copied out of third, the borrow continues in third₂",
            "Return immutably borrowed resource from third to third",
            "third isn't a reference, but its value holds one",
            "L3 v's resource is no longer immutably borrowed",
            "=L5 Partial copy from third to third",
            // third₂ copies the reference inside third, not third (#866).
            "L5 third₂ is initialized with a copy of third",
            "L5 third's value is copied",
            "L5 third goes out of scope. No resource is dropped.",
            "L8 third goes out of scope. No resource is dropped.",
            "L5 third₂'s immutable borrow ends (what it borrows from isn't shown)",
            // The lend repeated the end's "still immutably borrowed by third₂" (#1450).
            "L5 v's resource is immutably borrowed by third₂",
        ],
        ..E
    },
    Snippet {
        id: "c8_04_vec_for_ref",
        must_contain: &[
            "Immutable borrow from v to i",
            "When the iterator runs out of items, its shared borrow of v ends. This borrow no longer restricts access to v",
            "v goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &["i is the owner of the resource"],
        ..E
    },
    Snippet {
        id: "c8_05_vec_for_mut",
        must_contain: &[
            "Mutable borrow from v to i",
            "i holds a mutable reference; it can read and write the borrowed value",
            "the value i points to is modified through *i",
            "When the iterator runs out of items, its mutable borrow of v ends. This borrow no longer restricts access to v",
        ],
        must_not_contain: &["i is the owner of the resource", "*i acquires ownership of a resource"],
        ..E
    },
    Snippet {
        id: "c8_06_vec_push_string",
        must_contain: &[
            "Move from s to push",
            "v is passed to push by mutable reference (as self, implicitly borrowed). push may read or write it",
            "s goes out of scope. No resource is dropped.",
            "v goes out of scope. Its resource is dropped (including every String it holds).",
        ],
        ..E
    },
    Snippet {
        id: "c8_07_vec_index_copy",
        must_contain: &[
            "v is passed to push by mutable reference (as self, implicitly borrowed). push may read or write it",
            "first is initialized with a copy of v[0]",
            "L6 first goes out of scope",
        ],
        must_not_contain: &["first goes out of scope. No resource is dropped."],
        ..E
    },
    Snippet {
        id: "c8_08_string_push_char",
        must_contain: &["s is passed to push by mutable reference (as self, implicitly borrowed). push may read or write it", "s goes out of scope. Its resource is dropped."],
        ..E
    },
    Snippet {
        id: "c8_09_string_add",
        note: "s3 is drawn getting s1's String back from + (was: s3's value came from nowhere, #1351)",
        must_contain: &[
            "Move from s1 to +",
            "Move from + to s3",
            "s2 is passed to + by shared reference. + may read it",
            "s1 goes out of scope. No resource is dropped.",
            "s2 goes out of scope. Its resource is dropped.",
            "s3 goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &["s1 goes out of scope. Its resource is dropped."],
        ..E
    },
    Snippet {
        id: "c8_10_format_macro",
        must_contain: &[
            "s acquires ownership of a resource",
            "s1 goes out of scope. Its resource is dropped.",
            "s goes out of scope. Its resource is dropped.",
        ],
        ..E
    },
    Snippet {
        id: "c8_11_string_chars",
        note: "the Chars iterator borrows s until the loop ends (was: s unborrowed again on the for line, #868; its segment names the loop's iterator, #871)",
        must_contain: &[
            "chars takes a shared reference (as self, implicitly borrowed). It may read through that reference",
            "L5 s's resource is no longer immutably borrowed",
            "s's resource is immutably borrowed by the `for` loop's iterator; s is read-only until the loop ends",
            "s goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &[
            "L3 s's resource is no longer immutably borrowed",
            // c is a char: no variable shares s, the iterator does (#871).
            "s's resource is being shared by one or more variables",
        ],
        ..E
    },
    Snippet {
        id: "c8_12_hashmap_insert_get",
        must_contain: &[
            "scores is passed to insert by mutable reference (as self, implicitly borrowed). insert may read or write it",
            "scores is passed to get by shared reference (as self, implicitly borrowed). get may read it",
            "team_name is passed to get by shared reference. get may read it",
            "score receives the value unwrap_or() returns",
        ],
        ..E
    },
    Snippet {
        id: "c8_13_hashmap_moves_keys",
        must_contain: &[
            "Move from field_name to insert",
            "Move from field_value to insert",
            "field_name goes out of scope. No resource is dropped.",
            "map goes out of scope. Its resource is dropped (including every key and value it holds).",
        ],
        ..E
    },
    Snippet {
        id: "c8_14_hashmap_entry",
        must_contain: &[
            "text is bound to an immutable reference",
            "split_whitespace takes a shared reference (as self). It may read through that reference",
            // The loop's iterator reads through text until the loop's
            // `}` (#1081).
            "L6 the value text points to is immutably borrowed by the `for` loop's iterator, through a copy of text, until the loop ends",
            "L9 the `for` loop's iterator no longer borrows the value text points to",
            "L9 text's borrow of the string literal ends (string literals live for the whole program)",
            // #1266: the borrow of map comes from `entry`; `or_insert`
            // passes it on (it named `or_insert` before).
            "L7 Mutable borrow from map to count: entry's signature gives the value it returns the same lifetime as the reference passed as its receiver and its argument (the other is a value with no column here), so map stays borrowed while count is in use; or_insert passes the borrow on",
            "the value count points to is modified through *count",
            "If count holds a mutable borrow affecting map, that borrow ends here. It no longer restricts access to map",
            "map goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &[
            "text is the owner of the resource",
            "text acquires ownership of a resource",
            "*count acquires ownership of a resource",
            "map goes out of scope. No resource is dropped.",
            "text's shared borrow of *text ends",
            "text's immutable borrow ends (what it borrows from isn't shown)",
            // split_whitespace is a method, not a lender (#868).
            "word's shared borrow of split_whitespace ends",
            "L6 split_whitespace no longer borrows the value text points to",
            "L6 text's borrow of the string literal ends (string literals live for the whole program)",
        ],
        ..E
    },
    Snippet {
        id: "c8_15_vec_for_by_value",
        // The loop is drawn once; its first dot (s's, on the `for` line)
        // says so (#853).
        must_contain: &["Move of each element of v to s", "s takes ownership of an element of v", "(RustViz draws one pass through this loop, which may run many times; the variables declared inside it are new on each pass.)", "v goes out of scope. No resource is dropped."],
        // #850: was "Move from v to s", as if `s` took the whole Vec.
        must_not_contain: &["Move from v to s"],
        ..E
    },
    Snippet {
        id: "c8_16_as_str",
        must_contain: &[
            "s is passed to as_str by shared reference (as self, implicitly borrowed). as_str may read it",
            "Immutable borrow from s to t: as_str's signature gives the reference it returns the same lifetime as the reference passed as its receiver",
            "If t holds a shared borrow affecting s, that borrow ends here. It no longer restricts access to s",
        ],
        must_not_contain: &["t is the owner of the resource", "L3 s's resource is no longer immutably borrowed"],
        ..E
    },
    // ─── Chapter 9: errors ────────────────────────────────────────────
    Snippet {
        id: "c9_01_result_match",
        must_contain: &[
            "Partial copy from r to n",
            "Partial move from r to e",
            "e goes out of scope. Its resource is dropped.",
            "r may be partially moved above, so it can't be used as a whole after the branches. If execution reaches the end of r's scope while it still holds its resource (including the String it holds, if any), it is dropped there.",
            "r goes out of scope. No value inside r remains to drop here. No resource is dropped. Ok holds a value of type i32 that needs no drop. For Err, the String payload was moved out.",
        ],
        must_not_contain: &["payload was moved out. Rust tracks the move with a runtime drop flag.",
            "r was moved in at least one branch above,",
            "r was moved in at least one branch above; in branches where it was not, its resource is dropped at the branch's end.",
            // #873: `Ok(3)` builds a new value; `Ok` is not an actor.
            "Move from Ok to r",
            "=Ok",
        ],
        ..E
    },
    Snippet {
        id: "c9_02_parse_unwrap",
        must_contain: &["n receives the value unwrap() returns"],
        ..E
    },
    Snippet {
        id: "c9_03_expect_string",
        must_contain: &[
            "Move from maybe to expect",
            "Move from expect to s",
            "maybe goes out of scope. No resource is dropped.",
            "s goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &["maybe goes out of scope. Its resource is dropped."],
        ..E
    },
    Snippet {
        id: "c9_04_question_option",
        status: Correct,
        note: "renders since #155 (was a crash in the probe). x (from v.first()?) borrows *v until L3: the `?` passes first's borrow on, as unwrap does (#1352)",
        must_contain: &[
            "v is an immutable borrow from the caller",
            "first takes a shared reference (as self). It may read through that reference",
            "x holds an immutable reference; it can read the borrowed value",
            "L2 Immutable borrow from v to x: first's signature gives the value it returns the same lifetime as the reference passed as its receiver; the `?` passes the borrow on",
            "L3 If x holds a shared borrow affecting the data behind v, that borrow ends here. It no longer restricts access to the data behind v",
            "L3 v's immutable borrow ends",
            "v is passed to first_plus_one by shared reference. first_plus_one may read it",
            "v goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &[
            "x is the owner of the resource",
            "x's shared borrow of *x ends",
            "L2 v's resource is no longer immutably borrowed",
            "L2 v's immutable borrow ends",
            "x's immutable borrow ends (what it borrows from isn't shown)",
            "L2 first no longer borrows the resource v points to",
            "Copy of the value inside the Some that first() returns to x",
        ],
        ..E
    },
    Snippet {
        id: "c9_05_early_err_return",
        must_contain: &[
            "x's value is copied to the caller",
            // `return Err(String::from(..))`: the call's result goes to the caller (#783).
            "Move from String::from to the caller",
            "Move from check to r",
            "r goes out of scope. Its resource is dropped (including the String it holds, if any).",
        ],
        must_not_contain: &["Move from Err to", "to Err"],
        ..E
    },
    Snippet {
        id: "c9_06_panic_branch",
        must_contain: &["v is passed to len by shared reference (as self, implicitly borrowed). len may read it", "v goes out of scope. Its resource is dropped."],
        ..E
    },
    Snippet {
        id: "c9_07_question_result_string",
        status: Missing,
        note: "renders since #155 (was a crash in the probe); the tail `Ok(n * 2)` is not shown as returned",
        must_contain: &[
            "s is an immutable borrow from the caller",
            "A shared reference through s is passed to parse (as self). parse may read through it",
            "Move from double to r",
            "r goes out of scope",
        ],
        must_not_contain: &["r goes out of scope. Its resource is dropped."],
        ..E
    },
    Snippet {
        id: "c9_08_unwrap_or_else",
        must_contain: &[
            "Move from String::from to r",
            "Move from r to unwrap_or_else",
            "n receives the value unwrap_or_else() returns",
            "r goes out of scope. No resource is dropped.",
        ],
        // #768: `Err` is a constructor, never an actor.
        must_not_contain: &["r goes out of scope. Its resource is dropped.", "Move from Err to r", "to Err"],
        ..E
    },
    // ─── Chapter 10: generics, traits, lifetimes ──────────────────────
    Snippet {
        id: "c10_01_largest_generic",
        must_contain: &[
            "L3 item is bound to an immutable reference; it can read the borrowed value",
            "list is an immutable borrow from the caller",
            "Immutable reborrow from list to largest",
            "a reference from largest is returned to the caller; the data it points to is not copied",
            "number_list is passed to largest by shared reference. largest may read it",
            "item's reference is copied (not the resource it points to)",
            "L5 largest is assigned a copy of item's reference",
            "L13 result holds an immutable reference",
            "L13 Immutable borrow from number_list to result: largest's signature gives the reference it returns the same lifetime as its argument",
            "L14 If result holds a shared borrow affecting number_list, that borrow ends here. It no longer restricts access to number_list",
            "number_list goes out of scope. Its resource is dropped.",
            // #754: list's borrow goes on in the caller, through result.
            "the resource list points to is immutably borrowed by largest",
        ],
        must_not_contain: &[
            "L3 item is bound to an immutable reference; it can read the borrowed resource",
            "result is the owner of the resource",
            "item's resource is copied",
            "list's immutable borrow ends",
            "Immutable borrow from list to largest",
            "result's shared borrow of *result ends",
            "L5 largest is initialized with a copy of item",
            "Copy from largest to result",
            "L13 number_list's resource is no longer immutably borrowed",
        ],
        ..E
    },
    Snippet {
        id: "c10_02_generic_struct_method",
        must_contain: &["self is an immutable borrow from the caller", "p is passed to x by shared reference (as self, implicitly borrowed). x may read it"],
        must_not_contain: &["self is the owner of the resource"],
        ..E
    },
    Snippet {
        id: "c10_03_trait_impl",
        must_contain: &[
            "self is an immutable borrow from the caller",
            "t is passed to summarize by shared reference (as self, implicitly borrowed). summarize may read it",
            "t.username goes out of scope. Its resource is dropped.",
        ],
        ..E
    },
    Snippet {
        id: "c10_04_trait_default",
        must_contain: &["a is passed to summarize by shared reference (as self, implicitly borrowed). summarize may read it", "Move from summarize to s", "a.title goes out of scope. Its resource is dropped."],
        ..E
    },
    Snippet {
        id: "c10_05_impl_trait_param",
        must_contain: &["item is an immutable borrow from the caller", "s is passed to notify by shared reference. notify may read it"],
        ..E
    },
    Snippet {
        id: "c10_06_longest",
        must_contain: &[
            "x holds an immutable reference; it can read the borrowed value",
            "y holds an immutable reference; it can read the borrowed value",
            "x is an immutable borrow from the caller",
            "string2 is bound to an immutable reference",
            "string1 is passed to as_str by shared reference (as self, implicitly borrowed). as_str may read it",
            "L8 string2's borrow of the string literal ends (string literals live for the whole program)",
            "L8 Immutable borrow from string1 to result: longest's signature gives the reference it returns the same lifetime as both arguments (the other is a value with no column here), so string1 stays borrowed while result is in use",
            "L9 If result holds a shared borrow affecting string1, that borrow ends here. It no longer restricts access to string1",
            // Each branch of the tail `if` returns its reference (#674).
            "a reference from x is returned to the caller; the data it points to is not copied",
            "a reference from y is returned to the caller; the data it points to is not copied",
        ],
        must_not_contain: &[
            "string2 is the owner of the resource",
            "string2 acquires ownership of a resource",
            "string2's shared borrow of *string2 ends",
            "result's shared borrow of *result ends",
            "L8 string1's resource is no longer immutably borrowed",
            "string2's immutable borrow ends (what it borrows from isn't shown)",
        ],
        ..E
    },
    Snippet {
        id: "c10_07_longest_inner_scope",
        must_contain: &[
            "x holds an immutable reference; it can read the borrowed value",
            "y holds an immutable reference; it can read the borrowed value",
            "string1 is passed to longest by shared reference. longest may read it",
            "string2 is passed to longest by shared reference. longest may read it",
            // The result is initialized after the argument effects on L9.
            "L9+ result holds an immutable reference",
            "L9 Immutable borrow from string1 to result: longest's signature gives the reference it returns the same lifetime as both arguments. The code in longest has return paths from both tied inputs. At the caller, both stay borrowed while result is in use",
            "L9 Immutable borrow from string2 to result: longest's signature gives the reference it returns the same lifetime as both arguments. The code in longest has return paths from both tied inputs. At the caller, both stay borrowed while result is in use",
            "L10 If result holds a shared borrow affecting string1, that borrow ends here. It no longer restricts access to string1",
            "L10 If result holds a shared borrow affecting string2, that borrow ends here. It no longer restricts access to string2",
            "L11 string2 goes out of scope. Its resource is dropped.",
            // Each branch of the tail `if` returns its reference (#674).
            "a reference from x is returned to the caller; the data it points to is not copied",
            "a reference from y is returned to the caller; the data it points to is not copied",
        ],
        must_not_contain: &[
            "result is the owner of the resource",
            "result's shared borrow of *result ends",
            "the resource result points to",
            "L9 string1's resource is no longer immutably borrowed",
            "L9 string2's resource is no longer immutably borrowed",
        ],
        ..E
    },
    Snippet {
        id: "c10_08_generic_by_value",
        must_contain: &[
            "Move from s to show",
            "Copy from n to show",
            "t goes out of scope. Its resource is dropped.",
            "s goes out of scope. No resource is dropped.",
        ],
        ..E
    },
    // ─── Chapter 13: closures and iterators ───────────────────────────
    Snippet {
        id: "c13_01_closure_param",
        must_contain: &[
            "Closure add_one is bound",
            "L3 Calling add_one borrows add_one by shared reference as the receiver. The call may read through it. When the call returns, its receiver borrow ends",
            "L3 y is initialized with the value add_one() returns",
            "y receives the value add_one() returns",
        ],
        must_not_contain: &["add_one's resource is copied", "Copy of the value add_one() returns to y", "add_one is passed to add_one by shared reference"],
        ..E
    },
    Snippet {
        id: "c13_02_fnonce_move_return",
        must_contain: &[
            "Closure capture (move) from s to f",
            "s goes out of scope. No resource is dropped.",
            "t goes out of scope. Its resource is dropped.",
            // f() consumes f (FnOnce) and moves s out to t (#271).
            "f goes out of scope. No resource is dropped.",
            "t acquires ownership of the resource f() returns",
        ],
        must_not_contain: &[
            "f goes out of scope. Its 1 captured resource is dropped.",
            "t receives the value f() returns",
            "Move from f to t",
        ],
        ..E
    },
    Snippet {
        id: "c13_03_fnmut_counter",
        must_contain: &[
            "Closure capture (mutable borrow) from count to inc",
            "L4 inc's value is mutably borrowed for the call inc(). inc can't be used while the borrow is live",
            "L5 inc's value is mutably borrowed for the call inc(). inc can't be used while the borrow is live",
            "inc is passed to the call inc() by mutable reference. The call may read or write it",
            "If inc holds a mutable borrow affecting count, that borrow ends here. It no longer restricts access to count",
        ],
        must_not_contain: &["Return mutably borrowed resource from inc to count", "inc is passed to inc by mutable reference"],
        ..E
    },
    Snippet {
        id: "c13_04_thread_spawn_move",
        must_contain: &[
            "list acquires ownership of a resource",
            "Move from list into the closure passed to thread::spawn",
            "list's resource is moved into a closure passed to thread::spawn; it's dropped when that closure is dropped",
            "list goes out of scope. No resource is dropped.",
        ],
        must_not_contain: &["list goes out of scope. Its resource is dropped.", "Move from list to thread::spawn"],
        ..E
    },
    Snippet {
        id: "c13_05_closure_to_fn",
        status: Missing,
        note: "the closure's capture of k when passed as an argument is missing (#298)",
        must_contain: &[
            "f acquires ownership from the caller",
            "L2 Calling f borrows f by shared reference as the receiver. The call may read through it. When the call returns, its receiver borrow ends",
            "f goes out of scope. Rust drops the callable value if its concrete type needs drop.",
            "r receives the value apply() returns",
        ],
        must_not_contain: &["f is initialized with a copy of Anonymous resource", "f is the owner of the resource", "f goes out of scope. Its resource is dropped.", "f is passed to f by shared reference"],
        ..E
    },
    Snippet {
        id: "c13_06_iter_map_collect",
        must_contain: &["iter takes a shared reference (as self, implicitly borrowed). It may read through that reference", "Move from collect to v2", "v1 goes out of scope. Its resource is dropped."],
        ..E
    },
    Snippet {
        id: "c13_07_into_iter_filter",
        must_contain: &[
            "Move from shoes to shoes_in_size",
            "Move from shoes to into_iter",
            "L7 shoes goes out of scope. No resource is dropped.",
            "Move from shoes_in_size to mine",
            "shoes goes out of scope. No resource is dropped.",
        ],
        must_not_contain: &["L7 shoes goes out of scope. Its resource is dropped."],
        ..E
    },
    Snippet {
        id: "c13_08_iter_mut",
        status: Correct,
        note: "Each x stops holding its reference at its last use. The iterator keeps v borrowed through exhaustion (#171, #789, #868).",
        must_contain: &[
            "iter_mut takes a mutable reference (as self, implicitly borrowed). It may read or write through that reference",
            "the value x points to is modified through *x",
            "When the iterator runs out of items, its mutable borrow of v ends. This borrow no longer restricts access to v",
            // x's borrow of v, on the `for` line (#1347).
            "Mutable borrow from v to x",
            "x's mutable borrow ends",
            "L5 v's resource is no longer mutably borrowed",
            "v goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &[
            "*x acquires ownership of a resource",
            "x's mutable borrow of iter_mut ends",
            "L3 v's resource is no longer mutably borrowed",
        ],
        ..E
    },
    Snippet {
        id: "c13_09_iter_sum",
        must_contain: &["iter takes a shared reference (as self, implicitly borrowed). It may read through that reference", "total receives the value sum() returns"],
        ..E
    },
    Snippet {
        id: "c13_10_iter_next",
        must_contain: &[
            "iter takes a shared reference (as self, implicitly borrowed). It may read through that reference",
            "v1_iter is passed to next by mutable reference (as self, implicitly borrowed). next may read or write it",
            "L3 Immutable borrow from v1 to v1_iter: iter's signature gives the value it returns the same lifetime as the reference passed as its receiver; v1_iter isn't a reference, but its value holds one",
            "L5 If v1_iter holds a shared borrow affecting v1, that borrow ends here. It no longer restricts access to v1",
        ],
        must_not_contain: &["L3 v1's resource is no longer immutably borrowed"],
        ..E
    },
    Snippet {
        id: "c13_11_sort_by_key_fnmut",
        status: Missing,
        note: "the closure's &mut capture of num_sort_operations is not shown",
        must_contain: &["sort_by_key takes a mutable reference (as self, implicitly borrowed). It may read or write through that reference", "list is bound to a copyable value", "list goes out of scope"],
        must_not_contain: &["list goes out of scope. No resource is dropped."],
        ..E
    },
    Snippet {
        id: "c13_12_closure_borrow_then_use",
        must_contain: &[
            "Closure capture (immutable borrow) from list to only_borrows",
            "L5 Calling only_borrows borrows only_borrows by shared reference as the receiver. The call may read through it. list's resource is being shared by one or more variables. only_borrows holds a shared borrow. When the call returns, its receiver borrow ends",
            "If only_borrows holds a shared borrow affecting list, that borrow ends here. It no longer restricts access to list",
            "list goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &["only_borrows is passed to only_borrows by shared reference"],
        ..E
    },
    // ─── Chapter 15: smart pointers ───────────────────────────────────
    Snippet {
        id: "c15_01_box_deref",
        must_contain: &["Copy from x to Box::new", "Move from Box::new to y", "y goes out of scope. Its resource is dropped."],
        ..E
    },
    Snippet {
        id: "c15_02_cons_list",
        note: "was a crash at c2037dd (nested Cons constructor)",
        must_contain: &[
            "Move from Box::new to list",
            "Move from list to _l",
            "_l goes out of scope. Its resource is dropped (including the Box&lt;List&gt; it holds, if any).",
            "list goes out of scope. No resource is dropped.",
        ],
        // #768: `Cons` is a constructor, never an actor.
        must_not_contain: &["Move from Cons to list", "to Cons"],
        ..E
    },
    Snippet {
        id: "c15_03_ref_deref",
        must_contain: &["Immutable borrow from x to y", "y's shared borrow of x ends. This borrow no longer restricts access to x. Ending the borrow doesn't move the value"],
        must_not_contain: &["y is the owner of the resource", "Return immutably borrowed resource from y to x"],
        ..E
    },
    Snippet {
        id: "c15_04_mybox_deref_coercion",
        status: Missing,
        note: "was a crash at c2037dd. Returning `&self.0` from deref has no event (#266)",
        must_contain: &[
            "self is an immutable borrow from the caller",
            "name is an immutable borrow from the caller",
            "m is passed to hello by shared reference. hello may read it",
            "m.0 goes out of scope. Its resource is dropped.",
        ],
        ..E
    },
    Snippet {
        id: "c15_05_rc_strong_count",
        must_contain: &[
            "a is passed to Rc::clone by shared reference. Rc::clone may read it",
            "Move from Rc::new to a",
            "a goes out of scope. Its resource is dropped (for an Rc, that lowers the reference count; the shared value is dropped only if that was its last Rc).",
            "b goes out of scope. Its resource is dropped (for an Rc, that lowers the reference count; the shared value is dropped only if that was its last Rc).",
            "Rc::clone makes another Rc to the same value (nothing is copied), now owned by b",
            "b is another Rc to the same value (Rc::clone increases the reference count; nothing is copied)",
        ],
        must_not_contain: &["b goes out of scope. Its resource is dropped.", "Move from Rc::clone to b"],
        ..E
    },
    Snippet {
        id: "c15_06_rc_method_clone",
        must_contain: &[
            "a is passed to clone by shared reference (as self, implicitly borrowed). clone may read it",
            "Move from Rc::new to a",
            "b goes out of scope. Its resource is dropped (for an Rc, that lowers the reference count; the shared value is dropped only if that was its last Rc).",
            "clone makes another Rc to the same value (nothing is copied), now owned by b",
            "b is another Rc to the same value (Rc::clone increases the reference count; nothing is copied)",
        ],
        must_not_contain: &["b goes out of scope. Its resource is dropped.", "Move from clone to b"],
        ..E
    },
    Snippet {
        id: "c15_07_refcell_borrow_mut",
        status: Missing,
        note: "the `.push(2)` write through the borrow_mut guard has no event of its own, and RefCell's runtime &mut borrow isn't drawn (#685)",
        must_contain: &[
            "borrow borrows c immutably (a shared borrow); c has interior mutability, so its contents can change even through a shared borrow",
            "borrow_mut borrows c immutably (a shared borrow); exclusive access to its contents is checked at runtime (a second borrow_mut while this one is live would panic)",
            "c goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &["c is passed to borrow_mut by shared reference", "c is passed to borrow by shared reference"],
        ..E
    },
    Snippet {
        id: "c15_08_rc_refcell",
        status: Missing,
        note: "was a crash at c2037dd. The `+= 10` write through the borrow_mut guard has no event of its own (#685)",
        must_contain: &[
            "Move from Rc::new to value",
            "Rc::clone borrows value immutably (a shared borrow); value has interior mutability, so its contents can change even through a shared borrow",
            "borrow_mut borrows value immutably (a shared borrow); exclusive access to its contents is checked at runtime (a second borrow_mut while this one is live would panic)",
            "Rc::clone makes another Rc to the same value (nothing is copied), now owned by a",
            "println! borrows a for formatting through an implicit shared reference, so a isn't moved",
            "a goes out of scope. Its resource is dropped (for an Rc, that lowers the reference count; the shared value is dropped only if that was its last Rc).",
            "value goes out of scope. Its resource is dropped (for an Rc, that lowers the reference count; the shared value is dropped only if that was its last Rc).",
        ],
        must_not_contain: &[
            "value is passed to borrow_mut by shared reference",
            "value is passed to Rc::clone by shared reference",
            "Move from Rc::clone to a",
            "a goes out of scope. Its resource is dropped.",
            "value goes out of scope. Its resource is dropped.",
        ],
        ..E
    },
    Snippet {
        id: "c15_09_drop_early",
        must_contain: &[
            "self is a mutable borrow from the caller",
            "self holds a mutable reference; it can read and write the borrowed resource",
            "Move from c to drop",
            "c goes out of scope. No resource is dropped.",
            "c.data goes out of scope. No resource is dropped.",
        ],
        must_not_contain: &["c.data goes out of scope. Its resource is dropped.", "self is the owner of the resource"],
        ..E
    },
    Snippet {
        id: "c15_10_box_string_move",
        must_contain: &[
            "Move from b to c",
            "b goes out of scope. No resource is dropped.",
            "c goes out of scope. Its resource is dropped (including the String it holds).",
        ],
        ..E
    },
    // ─── Misc: shadowing, loops, patterns, items ──────────────────────
    Snippet {
        id: "m_01_shadow_type_change",
        note: "the second `spaces` is its own column, `spaces₂` (#173)",
        must_contain: &[
            "spaces is bound to an immutable reference",
            "A shared reference through spaces is passed to len (as self). len may read through it",
            "L3 spaces's borrow of the string literal ends (string literals live for the whole program)",
            "L3 spaces₂ is initialized with the value len() returns",
            "L5 spaces₂ goes out of scope",
        ],
        must_not_contain: &[
            "spaces is the owner of the resource",
            "spaces acquires ownership of a resource",
            "spaces's shared borrow of *spaces ends",
            "L4 spaces's borrow of the string literal ends (string literals live for the whole program)",
            "L4 spaces's immutable borrow ends",
            "L3 spaces is assigned the result of len",
        ],
        ..E
    },
    Snippet {
        id: "m_02_shadow_inner_block",
        note: "the inner x is its own column, `x₂`, dropped at L6 (#173)",
        must_contain: &[
            "L2 Move from String::from to x",
            "L4 Move from String::from to x₂",
            "L6 x₂ goes out of scope. Its resource is dropped.",
            "L8 x goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &["=L4 Move from String::from to x"],
        ..E
    },
    Snippet {
        id: "m_03_nested_block_borrow",
        must_contain: &[
            "Immutable borrow from a to r",
            "r's shared borrow of a ends. This borrow no longer restricts access to a. Ending the borrow doesn't move the resource",
            "L7 b goes out of scope. Its resource is dropped.",
            "L9 a goes out of scope. Its resource is dropped.",
        ],
        ..E
    },
    Snippet {
        id: "m_04_loop_break_string",
        must_contain: &[
            "counter is bound to a copyable value",
            "s acquires ownership of a resource",
            "s goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &["=s goes out of scope. No resource is dropped."],
        ..E
    },
    Snippet {
        id: "m_05_while_let_pop",
        must_contain: &["stack is passed to pop by mutable reference (as self, implicitly borrowed). pop may read or write it", "Copy of the value inside the Option pop() returns to top", "stack goes out of scope. Its resource is dropped."],
        ..E
    },
    Snippet {
        id: "m_06_tuple_index",
        must_contain: &[
            "Copy from tup.0 to x",
            "z is initialized with a copy of tup.2",
            "Copy from tup.0 to five_hundred",
            "println! borrows five_hundred for formatting through an implicit shared reference, so five_hundred isn't moved",
        ],
        ..E
    },
    Snippet {
        id: "m_07_tuple_var_destructure_move",
        must_contain: &[
            "Move from pair.0 to a",
            "Move from pair.1 to b",
            "pair goes out of scope. pair.0 and pair.1 were moved out earlier, so no resource is dropped.",
            "a goes out of scope. Its resource is dropped.",
        ],
        ..E
    },
    Snippet {
        id: "m_08_early_return_string",
        must_contain: &[
            "L3 s's resource is moved to the caller",
            "L4 s still owns its resource here: the branch that moved s returned before reaching this point",
            "L6 s goes out of scope. Its resource is dropped.",
            "Move from s to pick",
            "Move from pick to r",
            "s goes out of scope. No resource is dropped.",
        ],
        must_not_contain: &["L3 s goes out of scope. Its resource is dropped."],
        ..E
    },
    Snippet {
        id: "m_09_struct_destructure",
        must_contain: &["Copy from p.x to x", "Copy from p.y to y"],
        // Destructuring copies each i32 field out of p; it isn't a copy of p as a whole (#326).
        must_not_contain: &["Copy from p to x", "Copy from p to y"],
        ..E
    },
    Snippet {
        id: "m_10_for_range_break",
        must_contain: &["L9 s goes out of scope. Its resource is dropped."],
        ..E
    },
    Snippet {
        id: "m_11_unreachable_arm",
        must_contain: &["L4 Move from String::from to s", "L5 Move from String::from to s", "s goes out of scope. Its resource is dropped.", "L7 s acquired ownership of a resource in every branch that reaches this point (the _ arm panicked)"],
        must_not_contain: &["L6 s acquires ownership of a resource"],
        ..E
    },
    Snippet {
        id: "m_12_method_chain_trim_len",
        must_contain: &["trim takes a shared reference (as self, implicitly borrowed). It may read through that reference", "n receives the value len() returns"],
        ..E
    },
    Snippet {
        id: "m_13_return_ref_from_vec",
        must_contain: &[
            "v is an immutable borrow from the caller",
            "a reference to v[0], reborrowed through v, is returned to the caller; the data it points to is not copied",
            "v is passed to first by shared reference. first may read it",
            "L7 f holds an immutable reference",
            "L7 Immutable borrow from v to f: first's signature gives the reference it returns the same lifetime as its argument",
            "L8 If f holds a shared borrow affecting v, that borrow ends here. It no longer restricts access to v",
            "v goes out of scope. Its resource is dropped (including every String it holds).",
        ],
        must_not_contain: &[
            "v's resource is moved to the caller",
            "f is the owner of the resource",
            "f's shared borrow of *f ends",
            "L7 v's resource is no longer immutably borrowed",
            // `&v[0]` is a reference to one element, reborrowed through
            // v, not v's reference (#1071).
            "a reference from v is returned to the caller",
            "v's immutable borrow ends",
        ],
        ..E
    },
    Snippet {
        id: "m_14_while_counter",
        must_contain: &["number is bound to a copyable value"],
        ..E
    },
    Snippet {
        id: "m_15_struct_vec_field_method",
        must_contain: &[
            "self is a mutable borrow from the caller",
            "self holds a mutable reference; it can read and write the borrowed resource",
            "s is passed to push by mutable reference (as self, implicitly borrowed). push may read or write it",
            "s.items is passed to len by shared reference (as self, implicitly borrowed). len may read it",
            "s.items goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &["self is the owner of the resource"],
        ..E
    },
    Snippet {
        id: "m_16_match_range",
        must_contain: &["n is bound to a copyable value"],
        ..E
    },
    Snippet {
        id: "m_17_borrow_mut_then_read",
        must_contain: &[
            "Mutable borrow from x to r",
            "r holds a mutable reference; it can read and write the borrowed value",
            "the value r points to is modified through *r",
            "r's mutable borrow of x ends. This borrow no longer restricts access to x. Ending the borrow doesn't move the value",
            "Copy from x to y",
        ],
        must_not_contain: &["r is the owner of the resource", "*r acquires ownership of a resource", "Return mutably borrowed resource from r to x"],
        ..E
    },
    Snippet {
        id: "m_18_const",
        note: "renders since #155 (was a crash in the probe); the copy names the constant since #690",
        must_contain: &["x is initialized with a copy of the constant MAX_POINTS"],
        must_not_contain: &["MAX_POINTS goes out of scope", "=MAX_POINTS, immutable"],
        ..E
    },
    // ─── Extras: crash minimizations and controls ─────────────────────
    Snippet {
        id: "x01_field_compound_assign_main",
        must_contain: &[
            "L6 c.n is bound to a copyable value",
            "L7 c.n is assigned a new value",
            "c goes out of scope",
        ],
        must_not_contain: &[
            "c goes out of scope. Its resource is dropped.",
            "c.n acquires ownership of a resource",
        ],
        ..E
    },
    Snippet {
        id: "x02_mut_self_field_assign_plain",
        status: Missing,
        note: "was a crash at c2037dd. `self.n = 0` through &mut self isn't drawn: c.n's column shows no write (#147), \
               only that reset borrows it with c and may change it (#1311)",
        must_contain: &[
            "self is a mutable borrow from the caller",
            "L13 c.n is mutably borrowed along with c by reset, which may change c.n; the borrow ends here",
            "c is passed to reset by mutable reference (as self, implicitly borrowed). reset may read or write it",
            "println! borrows c.n for formatting through an implicit shared reference, so c.n isn't moved",
        ],
        ..E
    },
    Snippet {
        id: "x03_free_fn_mut_ref_field_compound",
        status: Missing,
        note: "was a crash at c2037dd. `c.n += 1` through c: &mut Counter isn't drawn: main's c.n shows no write (#147), \
               only that inc borrows it with c and may change it (#1311)",
        must_contain: &[
            "c is a mutable borrow from the caller",
            "L11 c.n is mutably borrowed along with c by inc, which may change c.n; the borrow ends here",
            "c is passed to inc by mutable reference. inc may read or write it",
            "c goes out of scope",
        ],
        must_not_contain: &["c goes out of scope. Its resource is dropped."],
        ..E
    },
    Snippet {
        id: "x04_tuple_field_simple",
        status: Missing,
        note: "was a crash at c2037dd. `let t = (1, 2)` and `let a = t.0` have no events (#189)",
        must_contain: &["println! borrows a for formatting through an implicit shared reference, so a isn't moved"],
        ..E
    },
    Snippet {
        id: "x05_refcell_deref_assign",
        status: Missing,
        note: "was a crash at c2037dd. The `+= 1` write through the borrow_mut guard has no event of its own (#685)",
        must_contain: &[
            "borrow_mut borrows c immutably (a shared borrow); exclusive access to its contents is checked at runtime (a second borrow_mut while this one is live would panic)",
            "Move from RefCell::new to c",
            "println! borrows c for formatting through an implicit shared reference, so c isn't moved",
            "c goes out of scope",
        ],
        must_not_contain: &[
            "c goes out of scope. Its resource is dropped.",
            "c is passed to borrow_mut by shared reference",
        ],
        ..E
    },
    Snippet {
        id: "x06_const_in_expr",
        note: "renders since #155 (was a crash in the probe)",
        must_contain: &["x is bound to a copyable value"],
        ..E
    },
    Snippet {
        id: "x07_cons_qualified",
        note: "single (non-nested) constructor",
        must_contain: &["Move from Box::new to list", "Move from list to _l", "_l goes out of scope. Its resource is dropped (including the Box&lt;List&gt; it holds, if any)."],
        must_not_contain: &["Move from List::Cons to list", "to List::Cons"],
        ..E
    },
    Snippet {
        id: "x08_mut_ref_deref_compound_main",
        must_contain: &[
            "Mutable borrow from n to r",
            "r holds a mutable reference; it can read and write the borrowed value",
            "the value r points to is modified through *r",
            "r's mutable borrow of n ends. This borrow no longer restricts access to n. Ending the borrow doesn't move the value",
        ],
        must_not_contain: &["*r acquires ownership of a resource", "r is the owner of the resource", "Return mutably borrowed resource from r to n"],
        ..E
    },
    Snippet {
        id: "x09_static_item",
        note: "was a crash at c2037dd. The static has a column since #690",
        must_contain: &[
            "L3 GREETING is a static: it lives for the whole program and is never dropped",
            "L4 Copy from GREETING to g",
            "L4 g is initialized with a copy of GREETING",
            "L6 GREETING is a static: it lives for the whole program and is never dropped",
            "println! borrows g for formatting through an implicit shared reference, so g isn't moved",
            "L5 g's borrow of what the static GREETING points to ends (it lives for the whole program)",
        ],
        must_not_contain: &[
            "g's shared borrow of *g ends",
            "g's immutable borrow ends (what it borrows from isn't shown)",
            "GREETING goes out of scope",
        ],
        ..E
    },
    Snippet {
        id: "x10_existing_question_mark",
        note: "copy of tests/question_mark.rs; renders since #155. The tail `Ok(n)` copies n to the caller (#768: no arrow into `Ok`)",
        must_contain: &["Copy of the value inside the Ok that parse() returns to n", "n's value is copied to the caller"],
        must_not_contain: &["Copy from n to Ok"],
        ..E
    },
    Snippet {
        id: "x11_existing_shadowing_basic",
        note: "copy of tests/shadowing_basic.rs: the second `s` is its own column, `s₂`, and both drop at L9 (#173)",
        must_contain: &[
            "L6 Move from String::from to s",
            "L7 Move from String::from to s₂",
            "L9 s goes out of scope. Its resource is dropped.",
            "L9 s₂ goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &["=L7 Move from String::from to s"],
        ..E
    },
    Snippet {
        id: "x12_cons_qualified_nested",
        note: "was a crash at c2037dd (nested List::Cons)",
        must_contain: &[
            "Move from Box::new to list",
            "Move from list to _l",
            "_l goes out of scope. Its resource is dropped (including the Box&lt;List&gt; it holds, if any).",
            "list goes out of scope. No resource is dropped.",
        ],
        must_not_contain: &["Move from List::Cons to list", "to List::Cons"],
        ..E
    },
    Snippet {
        id: "x13_nested_same_fn_call",
        must_contain: &["x receives the value add() returns"],
        ..E
    },
    Snippet {
        id: "x14_nested_string_fn",
        must_contain: &[
            "s acquires ownership from the caller",
            "Move from s to +",
            // The result goes to the caller (#1351).
            "Move from + to the caller",
            "L3 s goes out of scope. No resource is dropped.",
            "Move from shout to s",
            "L8 s goes out of scope. Its resource is dropped.",
        ],
        must_not_contain: &["L3 s goes out of scope. Its resource is dropped."],
        ..E
    },
    Snippet {
        id: "x15_question_in_main_style",
        status: Missing,
        note: "renders since #155 (was a crash in the probe); the tail `Ok(x + y)` is not shown as returned",
        must_contain: &["a is an immutable borrow from the caller", "A shared reference through a is passed to parse (as self). parse may read through it", "A shared reference through b is passed to parse (as self). parse may read through it"],
        ..E
    },
    Snippet {
        id: "x16_match_arm_block_body",
        note: "control for c6_04: a block-bodied arm draws s's drop on canvas",
        must_contain: &["Partial move from opt to s", "L6 s goes out of scope. Its resource is dropped.", "opt may be partially moved above, so it can't be used as a whole after the branches. If execution reaches the end of opt's scope while it still holds its resource (including the String it holds, if any), it is dropped there.", "opt goes out of scope. No value inside opt remains to drop here. No resource is dropped. None carries no payload. For Some, the String payload was moved out."],
        must_not_contain: &["payload was moved out. Rust tracks the move with a runtime drop flag.", JOIN_OPT, "opt was moved in at least one branch above,"],
        ..E
    },
    Snippet {
        id: "x17_match_arm_fn_call_body",
        note: "control: fn-call arm body",
        must_contain: &["Partial move from opt to s", "Move from s to show", "s goes out of scope. No resource is dropped.", "opt may be partially moved above, so it can't be used as a whole after the branches. If execution reaches the end of opt's scope while it still holds its resource (including the String it holds, if any), it is dropped there.", "opt goes out of scope. No value inside opt remains to drop here. No resource is dropped. None carries no payload. For Some, the String payload was moved out."],
        must_not_contain: &["payload was moved out. Rust tracks the move with a runtime drop flag.", JOIN_OPT, "opt was moved in at least one branch above,"],
        ..E
    },
];

// ─── Harness ───────────────────────────────────────────────────────────

fn book_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("rustviz-plugin")
        .join("tests")
        .join("book")
}

fn ensure_plugin_installed() {
    let ok = Command::new("cargo")
        .args(["rv-plugin", "--help"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    assert!(
        ok,
        "`cargo rv-plugin` not on PATH. Run `cargo install --path rustviz-plugin --locked` \
         (or `./setup.sh`) before `cargo test`."
    );
}

enum Outcome {
    Rendered { code: String, timeline: String },
    /// The plugin panicked; the `panicked at ...` line.
    Crashed(String),
    /// Anything else (compile error, bad output): always a failure.
    Error(String),
}

fn run(id: &str) -> Outcome {
    let path = book_dir().join(format!("{}.rs", id));
    let src = match fs::read_to_string(&path) {
        Ok(s) => s,
        Err(e) => return Outcome::Error(format!("read {:?}: {}", path, e)),
    };
    match Rustviz::new(&src) {
        Ok(rv) => Outcome::Rendered {
            code: rv.code_panel_string(),
            timeline: rv.timeline_panel_string(),
        },
        Err(e) => {
            let msg = e.to_string();
            match msg.lines().find(|l| l.contains("panicked at")) {
                Some(line) => Outcome::Crashed(line.trim().to_string()),
                None => Outcome::Error(msg.chars().take(600).collect()),
            }
        }
    }
}

fn leading_number(s: &str) -> Option<(f64, &str)> {
    let end = s.find(|c: char| !(c.is_ascii_digit() || c == '.')).unwrap_or(s.len());
    s[..end].parse().ok().map(|n| (n, &s[end..]))
}

/// The y coordinate an element is drawn at, by the same priority as the
/// probe's driver: `cy`, `y1`, second number of `points`, start of a path
/// `d`, then `y`.
fn element_y(own: &str) -> Option<f64> {
    if let Some(v) = attr(own, "cy").or_else(|| attr(own, "y1")) {
        return leading_number(v).map(|(n, _)| n);
    }
    if let Some(v) = attr(own, "points") {
        let (_, rest) = leading_number(v)?;
        return leading_number(rest.strip_prefix([' ', ','])?).map(|(n, _)| n);
    }
    if let Some(v) = attr(own, "d") {
        let v = v.strip_prefix('M')?.trim_start();
        let (_, rest) = leading_number(v)?;
        return leading_number(rest.strip_prefix([' ', ','])?).map(|(n, _)| n);
    }
    attr(own, "y").and_then(leading_number).map(|(n, _)| n)
}

/// Code-panel rows as (y, line label). Rows without a printed line number
/// (wrapped or blank continuation rows) are labelled `<prev>+`.
fn code_rows(code_svg: &str) -> Vec<(f64, String)> {
    const OPEN: &str = "<text class=\"code\" x=\"";
    let mut rows = Vec::new();
    let mut last = "0".to_string();
    for chunk in code_svg.split(OPEN).skip(1) {
        let Some(after_x) = chunk.find("\" y=\"").map(|i| &chunk[i + 5..]) else { continue };
        let Some((y, rest)) = leading_number(after_x) else { continue };
        // (Past the tag's other attributes: `data-rv-*`, docs/element-ids.md.)
        let Some(rest) = rest.find('>').map(|i| &rest[i + 1..]) else { continue };
        let Some(rest) = rest.strip_prefix(" <tspan class=\"line-number\">") else { continue };
        let digits: String = rest.trim_start().chars().take_while(|c| c.is_ascii_digit()).collect();
        last = if digits.is_empty() {
            format!("{}+", last.trim_end_matches('+'))
        } else {
            digits
        };
        rows.push((y, last.clone()));
    }
    rows
}

fn clean_tooltip(raw: &str) -> String {
    let unescaped = raw
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&amp;", "&");
    let mut text = String::with_capacity(unescaped.len());
    let mut in_tag = false;
    for ch in unescaped.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => text.push(ch),
            _ => {}
        }
    }
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Every tooltip in the timeline as (line label, text). The label is the
/// code row nearest the element's y ("hdr" for column headers, "?" if the
/// element has no y).
fn tooltips(code_svg: &str, timeline_svg: &str) -> Vec<(String, String)> {
    const ATTR: &str = "data-tooltip-text=\"";
    let rows = code_rows(code_svg);
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(i) = timeline_svg[from..].find(ATTR) {
        let attr_start = from + i;
        let value_start = attr_start + ATTR.len();
        let Some(len) = timeline_svg[value_start..].find('"') else { break };
        let value_end = value_start + len;
        from = value_end + 1;

        let start = timeline_svg[..attr_start].rfind('<').unwrap_or(0);
        let tag_end = timeline_svg[start..].find(' ').map_or(start, |j| start + j);
        let is_group = &timeline_svg[start..tag_end] == "<g";
        let end = if is_group {
            timeline_svg[from..].find("</g>")
        } else {
            timeline_svg[from..].find('>')
        }
        .map_or(timeline_svg.len(), |j| from + j);
        let own = format!("{}{}", &timeline_svg[start..attr_start], &timeline_svg[from..end]);

        let line = match element_y(&own) {
            Some(y) if y <= 70.0 => "hdr".to_string(),
            Some(y) => rows
                .iter()
                .min_by(|a, b| (a.0 - 5.0 - y).abs().total_cmp(&(b.0 - 5.0 - y).abs()))
                .map_or("?".to_string(), |r| r.1.clone()),
            None => "?".to_string(),
        };
        // Events on one line of a column share one dot (#309); its
        // tooltip has one bulleted line per event. Match each separately.
        for part in timeline_svg[value_start..value_end].split("&lt;br&gt;") {
            let text = clean_tooltip(part);
            let text = text.strip_prefix("• ").map(str::to_owned).unwrap_or(text);
            out.push((line.clone(), text));
        }
    }
    out
}

/// Does a table string match a rendered tooltip? `"L13 text"` must match
/// on line 13; plain text matches on any line.
/// Like `present`, but `want` need only occur inside a tooltip (used for
/// `must_not_contain`, so a forbidden fragment can't hide in a longer one; #511).
fn present_substr(want: &str, actual: &[(String, String)]) -> bool {
    // A leading `=` means whole-tooltip equality (as `present`), for text that
    // is also a prefix/suffix of a correct tooltip.
    if let Some(whole) = want.strip_prefix('=') {
        return present(whole, actual);
    }
    let mut label = None;
    let mut text = want;
    if let Some(rest) = want.strip_prefix('L') {
        let n = rest.find(|c: char| !c.is_ascii_digit()).unwrap_or(0);
        if n > 0 {
            let (digits, tail) = rest.split_at(n);
            if let Some(t) = tail.strip_prefix("+ ") {
                label = Some(format!("{}+", digits));
                text = t;
            } else if let Some(t) = tail.strip_prefix(' ') {
                label = Some(digits.to_string());
                text = t;
            }
        }
    }
    actual
        .iter()
        .any(|(l, t)| label.as_ref().map_or(true, |x| x == l) && t.contains(text))
}

fn present(want: &str, actual: &[(String, String)]) -> bool {
    if let Some(rest) = want.strip_prefix('L') {
        let label_len = rest.find(|c: char| !c.is_ascii_digit()).unwrap_or(0);
        if label_len > 0 {
            let (digits, tail) = rest.split_at(label_len);
            let (label, text) = match tail.strip_prefix("+ ") {
                Some(text) => (format!("{}+", digits), text),
                None => match tail.strip_prefix(' ') {
                    Some(text) => (digits.to_string(), text),
                    None => return actual.iter().any(|(_, t)| t == want),
                },
            };
            return actual.iter().any(|(l, t)| *l == label && t == text);
        }
    }
    actual.iter().any(|(_, t)| t == want)
}

/// y coordinates outside `0..=height` of the panel's root `<svg>`:
/// elements drawn there are never visible.
fn off_canvas_ys(svg: &str) -> Vec<String> {
    let num = |v: &str| v.trim_end_matches("px").parse::<f64>().ok();
    let root = &svg[..svg.find('>').unwrap_or(svg.len())];
    let Some(height) = attr(root, "height").and_then(num) else {
        return vec!["<svg> has no height".to_string()];
    };
    let mut out = Vec::new();
    let mut rest = svg;
    while let Some(eq) = rest.find("=\"") {
        let name_start = rest[..eq]
            .rfind(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '_'))
            .map_or(0, |i| i + 1);
        let name = &rest[name_start..eq];
        let after = &rest[eq + 2..];
        let end = after.find('"').unwrap_or(after.len());
        let value = &after[..end];
        let ys: Vec<f64> = match name {
            "y" | "y1" | "y2" | "cy" => num(value).into_iter().collect(),
            "points" => value
                .split([' ', ','])
                .filter(|t| !t.is_empty())
                .filter_map(num)
                .skip(1)
                .step_by(2)
                .collect(),
            _ => Vec::new(),
        };
        for y in ys {
            if !(0.0..=height).contains(&y) {
                out.push(format!("{}={} (height {})", name, y, height));
            }
        }
        rest = &after[end.min(after.len())..];
    }
    out
}

fn line_sort_key(label: &str) -> (u32, u32) {
    match label {
        "hdr" => (0, 0),
        "?" => (u32::MAX, 0),
        l => {
            let n = l.trim_end_matches('+').parse().unwrap_or(u32::MAX - 1);
            (n, l.ends_with('+') as u32)
        }
    }
}

fn dump(dir: &str, id: &str, outcome: &Outcome) {
    let mut text = String::new();
    match outcome {
        Outcome::Rendered { code, timeline } => {
            text.push_str("RENDERED\n");
            let set: BTreeSet<_> = tooltips(code, timeline)
                .into_iter()
                .map(|(l, t)| (line_sort_key(&l), l, t))
                .collect();
            for (_, l, t) in set {
                text.push_str(&format!("L{} {}\n", l, t));
            }
            for (panel, svg) in [("code", code), ("timeline", timeline)] {
                for off in off_canvas_ys(svg) {
                    text.push_str(&format!("off-canvas ({}): {}\n", panel, off));
                }
            }
        }
        Outcome::Crashed(p) => text.push_str(&format!("CRASH {}\n", p)),
        Outcome::Error(e) => text.push_str(&format!("ERROR {}\n", e)),
    }
    let _ = fs::create_dir_all(dir);
    let _ = fs::write(PathBuf::from(dir).join(format!("{}.txt", id)), text);
}

fn timeline_dot<'a, 'input>(
    doc: &'a roxmltree::Document<'input>,
    var: &str,
    line: &str,
    event: &str,
) -> ((f64, f64), roxmltree::Node<'a, 'input>) {
    let mut dots = doc.descendants().filter(|n| {
        n.attribute("data-rv-kind") == Some("dot")
            && n.attribute("data-rv-var") == Some(var)
            && n.attribute("data-rv-line") == Some(line)
            && n.attribute("data-rv-event")
                .is_some_and(|events| events.split_whitespace().any(|e| e == event))
    });
    let dot = dots.next().unwrap_or_else(|| panic!("missing {var} {event} dot on L{line}"));
    assert!(dots.next().is_none(), "repeated {var} {event} dot on L{line}");
    let circle = dot.descendants().find(|n| n.has_tag_name("circle")).expect("dot has a circle");
    let coordinate = |attr| circle.attribute(attr).unwrap().parse().unwrap();
    ((coordinate("cx"), coordinate("cy")), dot)
}

fn check_book_paths(id: &str, timeline: &str) {
    use common::geometry::{near, parse};

    let tooltip = |n: roxmltree::Node| clean_tooltip(n.attribute("data-tooltip-text").unwrap_or_default());
    match id {
        "c6_11_enum_method" => {
            // MIR switches on the discriminant of *self. Only Write borrows
            // its payload through self. Quit prints without using self again.
            let doc = roxmltree::Document::parse(timeline).unwrap();
            let geo = parse(&doc);
            let (fork, _) = timeline_dot(&doc, "self", "8", "branch");
            let (quit_end, _) = timeline_dot(&doc, "self", "9", "static_die");
            let (write_lend, _) = timeline_dot(&doc, "self", "10", "static_lend");
            let (write_end, _) = timeline_dot(&doc, "self", "10", "static_die");
            let (give_back, give_back_dot) = timeline_dot(&doc, "self", "10", "static_reacquire");
            let (payload_borrow, _) = timeline_dot(&doc, "s", "10", "static_borrow");
            let (payload_end, _) = timeline_dot(&doc, "s", "10", "static_die");
            // The branch groups link to code-panel rows. Quit starts on L9
            // and its entry carries a state tooltip. Write starts on L10
            // and is named in the tooltip on its dashed entry.
            let quit_rows = "9 9";
            let write_rows = "10 11";
            let labels = |rows| {
                doc.descendants()
                    .filter(move |n| {
                        n.attribute("data-rv-kind") == Some("branch_line")
                            && n.attribute("data-rv-var") == Some("self")
                            && n.parent().and_then(|g| g.attribute("data-arm-rows")) == Some(rows)
                    })
                    .map(tooltip)
                    .filter(|tip| tip.starts_with("This segment leads into the ") || tip.starts_with("This segment is for the "))
            };
            let write_labels: Vec<_> = labels(write_rows).collect();
            assert!(
                write_labels.iter().any(|tip| tip.starts_with("This segment leads into the Message::Write branch"))
                    && write_labels.iter().all(|tip| tip.contains("Message::Write branch"))
                    && labels(quit_rows).all(|tip| tip.contains("Message::Quit branch")),
                "{id}: branch labels must match the Quit and Write source arms"
            );
            let arm = |rows| geo.branch_segments("self").filter(move |&(_, _, arm)| arm == Some(rows));
            assert!(
                arm(quit_rows).any(|(from, to, _)| near(from, fork) && near(to, quit_end)),
                "{id}: Quit borrow ends at its leg's entry"
            );
            assert!(
                arm(write_rows).any(|(from, entry, _)| {
                    near(from, fork) && !near(entry, quit_end)
                        && arm(write_rows).any(|(from, to, _)| near(from, entry) && near(to, write_lend))
                }),
                "{id}: Write reaches the reborrow on its own leg"
            );
            assert!(
                geo.arrow_connects("self", "s", "10", (write_lend, payload_borrow), |tip| {
                    clean_tooltip(tip) == "Partial immutable borrow from self to s"
                }),
                "{id}: the reborrow arrow must connect Write's self dot to the s borrow dot"
            );
            assert!(
                near((write_lend.0, give_back.1), give_back)
                    && tooltip(give_back_dot).contains("s no longer borrows the resource self points to")
                    && geo.arrow_connects("s", "self", "10", (payload_end, give_back), |tip| {
                        clean_tooltip(tip) == "s's shared borrow of the value inside *self ends. This borrow no longer restricts access to *self"
                    }),
                "{id}: the payload borrow must return from s to self on Write's leg"
            );
            assert!(
                write_lend.1 < give_back.1 && give_back.1 <= write_end.1
                    && near((write_lend.0, write_end.1), write_end)
                    && arm(write_rows).any(|(from, to, _)| near(from, write_lend) && near(to, write_end)),
                "{id}: Write's self borrow must last through the payload give-back"
            );
        }
        "m_08_early_return_string" => {
            // MIR moves the parameter into the return place on the true
            // edge. Only the false edge reaches the parameter's drop.
            let doc = roxmltree::Document::parse(timeline).unwrap();
            let geo = parse(&doc);
            let (fork, _) = timeline_dot(&doc, "s", "2", "branch");
            let (ret, ret_dot) = timeline_dot(&doc, "s", "3", "move");
            let (join, _) = timeline_dot(&doc, "s", "4", "branch_join");
            let (drop, drop_dot) = timeline_dot(&doc, "s", "6", "owner_go_out_of_scope");
            assert_eq!(
                tooltip(ret_dot), "s's resource is moved to the caller",
                "{id}: the return dot describes moving s to the caller without dropping it"
            );
            assert!(
                !doc.descendants().any(|n| {
                    n.attribute("data-rv-kind") == Some("dot")
                        && n.attribute("data-rv-var") == Some("s")
                        && n.attribute("data-rv-line") == Some("3")
                        && n.attribute("data-rv-drop") == Some("true")
                }),
                "{id}: s has no drop mark on the return row"
            );
            let legs = || geo.branch_segments("s");
            let return_rows = Some("3 4");
            assert!(
                legs().any(|(from, to, rows)| rows == return_rows && near(from, fork) && near(to, ret)),
                "{id}: returning leg reaches the move"
            );
            assert!(
                ret.1 < join.1
                    && legs().all(|(from, to, rows)| {
                        !near(from, ret) && (rows != return_rows || (from.1 <= ret.1 && to.1 <= ret.1))
                    }),
                "{id}: returning leg stops at the move above the join"
            );
            let continuing_leg = legs().any(|(from, entry, rows)| {
                rows.is_none() && near(from, fork) && !near(entry, ret)
                    && legs().any(|(from, to, rows)| rows.is_none() && near(from, entry) && near(to, join))
            });
            assert!(
                continuing_leg && legs().filter(|&(_, to, _)| near(to, join)).count() == 1,
                "{id}: only the continuing leg reaches the join"
            );
            assert!(
                join.1 < drop.1 && near((join.0, drop.1), drop)
                    && geo.main_segments("s", "full_privilege").any(|(from, to)| near(from, join) && near(to, drop))
                    && drop_dot.attribute("data-rv-drop") == Some("true"),
                "{id}: the continuing ownership line connects the join to the drop"
            );
        }
        _ => {}
    }
}

/// Checks one snippet's outcome against its table entry, pushing to
/// `failures` (test fails) or `notes` (table is out of date, test passes).
fn check(s: &Snippet, outcome: &Outcome, failures: &mut Vec<String>, notes: &mut Vec<String>) {
    let (code, timeline) = match (outcome, s.status) {
        (Outcome::Error(e), _) => {
            failures.push(format!("{}: did not compile or run: {}", s.id, e));
            return;
        }
        (Outcome::Crashed(_), Crash) => return,
        (Outcome::Crashed(p), _) => {
            failures.push(format!("{}: newly crashes: {}", s.id, p));
            return;
        }
        (Outcome::Rendered { .. }, Crash) => {
            notes.push(format!(
                "{}: known crash now renders. Promote it: set its status (Correct / Wrong / Missing) \
                 after reviewing the tooltips (RV_BOOK_DUMP), and add must_contain / known_wrong.",
                s.id
            ));
            return;
        }
        (Outcome::Rendered { code, timeline }, _) => (code, timeline),
    };

    if !code.trim_start().starts_with("<svg") || !timeline.trim_start().starts_with("<svg") {
        failures.push(format!("{}: a panel is not SVG", s.id));
        return;
    }
    for (panel, svg) in [("code", code), ("timeline", timeline)] {
        if let Some(e) = common::svg::xml_error(svg) {
            failures.push(format!("{}: {} panel is not well-formed XML (#1049): {}", s.id, panel, e));
            return;
        }
    }
    // Every hover target has a unique, structured `data-rv-id`
    // (docs/element-ids.md in rustviz-plugin).
    for (panel, svg) in [("code", code), ("timeline", timeline)] {
        let problems = common::svg::rv_problems(svg);
        if !problems.is_empty() {
            failures.push(format!("{}: {} panel's data-rv-* attributes: {:?}", s.id, panel, problems));
            return;
        }
    }
    if !timeline.contains("tooltip-trigger") {
        failures.push(format!("{}: timeline has no tooltip-trigger elements", s.id));
        return;
    }
    // Every tooltip says something (#919: a join dot's was empty).
    let empty = timeline.split("data-tooltip-text=\"").skip(1)
        .filter(|c| c.split('"').next().is_some_and(|t| t.trim().is_empty()))
        .count();
    if empty > 0 {
        failures.push(format!("{}: {} empty tooltip(s) in the timeline", s.id, empty));
    }

    let mut off = off_canvas_ys(code);
    off.extend(off_canvas_ys(timeline));
    match (off.is_empty(), s.off_canvas) {
        (false, false) => failures.push(format!(
            "{}: {} element(s) drawn outside the SVG: {:?}",
            s.id,
            off.len(),
            &off[..off.len().min(5)]
        )),
        (true, true) => notes.push(format!(
            "{}: every element is on canvas now. Clear `off_canvas` in tests/book.rs.",
            s.id
        )),
        _ => {}
    }

    let actual = tooltips(code, timeline);
    if matches!(s.id, "c10_06_longest" | "c10_07_longest_inner_scope") {
        let doc = roxmltree::Document::parse(timeline).expect("validated SVG");
        for name in ["x", "y"] {
            let readable = format!("{name} holds an immutable reference; it can read the borrowed value");
            let initialized_lines: Vec<_> = doc.descendants().filter(|node|
                node.attribute("data-rv-kind") == Some("line")
                    && node.attribute("data-rv-var") == Some(name)
                    && node.attribute("data-rv-line") == Some("1")).collect();
            if initialized_lines.is_empty() || initialized_lines.iter().any(|node|
                node.attribute("data-tooltip-text").is_none_or(|tip| clean_tooltip(tip) != readable)) {
                failures.push(format!("{}: the first {name} segment is missing its read-access clause", s.id));
            }
        }
    }
    let missing: Vec<_> = s.must_contain.iter().filter(|w| !present(w, &actual)).collect();
    if !missing.is_empty() {
        failures.push(format!("{}: missing correct tooltip(s) {:?}", s.id, missing));
    }
    let back: Vec<_> = s.must_not_contain.iter().filter(|w| present_substr(w, &actual)).collect();
    if !back.is_empty() {
        failures.push(format!("{}: false tooltip(s) are back {:?}", s.id, back));
    }
    let gone: Vec<_> = s.known_wrong.iter().filter(|w| !present(w, &actual)).collect();
    if !gone.is_empty() {
        notes.push(format!(
            "{}: known-wrong tooltip(s) no longer shown {:?}. If a fix removed them, move them to \
             must_not_contain and add the corrected text to must_contain.",
            s.id, gone
        ));
    }
    check_book_paths(s.id, timeline);
}

/// The table itself must be consistent with the files on disk.
fn check_table() -> Vec<String> {
    let mut problems = Vec::new();
    let mut on_disk: BTreeSet<String> = BTreeSet::new();
    for entry in fs::read_dir(book_dir()).expect("read tests/book") {
        let name = entry.unwrap().file_name().to_string_lossy().into_owned();
        if let Some(stem) = name.strip_suffix(".rs") {
            on_disk.insert(stem.to_string());
        }
    }
    let mut seen = BTreeSet::new();
    for s in BOOK {
        if !seen.insert(s.id) {
            problems.push(format!("{}: listed twice", s.id));
        }
        if !on_disk.contains(s.id) {
            problems.push(format!("{}: no file tests/book/{}.rs", s.id, s.id));
        }
        match s.status {
            Correct if !s.known_wrong.is_empty() || s.off_canvas => {
                problems.push(format!("{}: Correct but has known_wrong / off_canvas", s.id))
            }
            Crash if !(s.must_contain.is_empty() && s.known_wrong.is_empty()) => {
                problems.push(format!("{}: Crash entries carry no tooltip lists", s.id))
            }
            Wrong | Missing if s.note.is_empty() => {
                problems.push(format!("{}: say what's wrong or missing in `note`", s.id))
            }
            _ => {}
        }
    }
    for stem in on_disk.difference(&seen.iter().map(|s| s.to_string()).collect()) {
        problems.push(format!("tests/book/{}.rs has no entry in BOOK", stem));
    }
    problems
}

#[test]
fn book_corpus() {
    let table_problems = check_table();
    assert!(table_problems.is_empty(), "BOOK table:\n  {}", table_problems.join("\n  "));

    ensure_plugin_installed();
    env::set_var("RV_RUNNER", "local");

    let only: Vec<String> = env::var("RV_BOOK_ONLY")
        .map(|v| v.split(',').map(|p| p.trim().to_string()).filter(|p| !p.is_empty()).collect())
        .unwrap_or_default();
    let selected: Vec<&Snippet> = BOOK
        .iter()
        .filter(|s| only.is_empty() || only.iter().any(|p| s.id.starts_with(p.as_str())))
        .collect();
    let jobs: usize = env::var("RV_BOOK_JOBS").ok().and_then(|j| j.parse().ok()).unwrap_or(4).max(1);
    let dump_dir = env::var("RV_BOOK_DUMP").ok();

    let next = AtomicUsize::new(0);
    let outcomes: Mutex<Vec<Option<Outcome>>> = Mutex::new((0..selected.len()).map(|_| None).collect());
    std::thread::scope(|scope| {
        for _ in 0..jobs.min(selected.len()) {
            scope.spawn(|| loop {
                let i = next.fetch_add(1, Ordering::SeqCst);
                let Some(s) = selected.get(i) else { break };
                let outcome = run(s.id);
                if let Some(dir) = &dump_dir {
                    dump(dir, s.id, &outcome);
                }
                outcomes.lock().unwrap()[i] = Some(outcome);
            });
        }
    });
    let outcomes = outcomes.into_inner().unwrap();

    let mut failures = Vec::new();
    let mut notes = Vec::new();
    let (mut correct, mut wrong, mut missing, mut crashes, mut promoted) = (0, 0, 0, 0, 0);
    for (s, outcome) in selected.iter().zip(&outcomes) {
        let outcome = outcome.as_ref().expect("every snippet ran");
        check(s, outcome, &mut failures, &mut notes);
        match s.status {
            Correct => correct += 1,
            Wrong => wrong += 1,
            Missing => missing += 1,
            Crash => {
                crashes += 1;
                if matches!(outcome, Outcome::Rendered { .. }) {
                    promoted += 1;
                }
            }
        }
    }

    // Written straight to stderr so it shows without `--nocapture`.
    let mut report = format!(
        "book corpus: {} render ({} correct, {} known-wrong, {} missing), {} known crashes",
        correct + wrong + missing,
        correct,
        wrong,
        missing,
        crashes
    );
    if promoted > 0 {
        report.push_str(&format!(" ({} now render)", promoted));
    }
    report.push_str(&format!("; {} failure(s), {} note(s)\n", failures.len(), notes.len()));
    for n in &notes {
        report.push_str(&format!("  note: {}\n", n));
    }
    let _ = std::io::stderr().write_all(report.as_bytes());

    assert!(
        failures.is_empty(),
        "{} book-corpus regression(s):\n  {}\n(dump tooltips with RV_BOOK_DUMP=<dir> to see what the plugin printed)",
        failures.len(),
        failures.join("\n  ")
    );
}
