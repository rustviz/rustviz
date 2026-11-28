#![feature(core_intrinsics, custom_mir)]
#![allow(internal_features)]
use std::intrinsics::mir::*;

// The call's normal successor is also the other branch's successor.
// Ordinary Rust lowering can insert a cleanup block between these edges.
#[custom_mir(dialect = "built")]
fn string_first<'a>(test: bool, owned: &'a String, other: &'a str) -> &'a str {
    mir! {
        { match test { true => from_string, _ => from_slice } }
        from_string = {
            Call(RET = <String as std::ops::Deref>::deref(owned), ReturnTo(done), UnwindContinue())
        }
        from_slice = {
            RET = other;
            Goto(done)
        }
        done = { Return() }
    }
}

#[custom_mir(dialect = "built")]
fn string_second<'a>(test: bool, owned: &'a String, other: &'a str) -> &'a str {
    mir! {
        { match test { true => from_slice, _ => from_string } }
        from_slice = {
            RET = other;
            Goto(done)
        }
        from_string = {
            Call(RET = <String as std::ops::Deref>::deref(owned), ReturnTo(done), UnwindContinue())
        }
        done = { Return() }
    }
}

fn main() {
    let owned = String::from("owned");
    assert_eq!(string_first(true, &owned, "other"), "owned");
    assert_eq!(string_first(false, &owned, "other"), "other");
    assert_eq!(string_second(true, &owned, "other"), "other");
    assert_eq!(string_second(false, &owned, "other"), "owned");
}
