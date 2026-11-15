// #968: a `while` loop ends when its condition turns out false, which can
// be after the condition moved a variable declared before the loop. The
// body gives it a new value, but not after the last test of the condition.
// Checked with a `Drop`-printing type: in `main`, with [1, -1] both values
// are dropped inside the loop (by `drop(w)`) and nothing at the end; with
// [1], "b" is dropped at the end of `main`.

fn first(p: String) -> Option<char> {
    p.chars().next()
}

fn main() {
    let mut it = vec![1, -1].into_iter();
    let mut s = String::from("a");
    while let Some(x) = it.next() && { let w = s; drop(w); x > 0 } {
        s = String::from("b");
    }
}

// The condition moves u every time it's tested, so u is moved after the
// loop. Assigning it again drops nothing.
fn scrutinee() {
    let mut u = String::from("ab");
    while let Some(c) = first(u) {
        u = String::from("");
        println!("{}", c);
    }
    u = String::from("c");
    println!("{}", u);
}

// A plain `while` condition: v is moved every time it's tested.
fn plain(n: i32) {
    let mut v = String::from("a");
    let mut i = 0;
    while { let z = v; drop(z); i < n } {
        v = String::from("b");
        i += 1;
    }
}

// A chain whose move comes after a part that can fail: q may still hold
// "b" after the loop, so its reassignment drops it only if the condition
// didn't move it.
fn reassigned(o: Vec<i32>) {
    let mut it = o.into_iter();
    let mut q = String::from("a");
    while let Some(x) = it.next() && { let w = q; drop(w); x > 0 } {
        q = String::from("b");
    }
    q = String::from("c");
    println!("{}", q);
}
