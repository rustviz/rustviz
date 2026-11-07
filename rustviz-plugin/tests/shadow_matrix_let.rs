// Shadowing matrix, part 1: `let` re-binding one name in one block.
// Each fn uses its own names, so every tooltip names one case. Every
// `let` declares a new variable (`a`, `a₂`, …); the shadowed one keeps
// its value until the end of its scope (shadowing never drops it).

// Both Strings live to the end of the fn and are dropped there.
fn owned_twice() {
    let a = String::from("one");
    let a = String::from("two");
    println!("{}", a);
}

// Copy values: x₂ is computed from x, then copied into y.
fn copies() {
    let x = 5;
    let x = x + 1;
    let y = x;
    println!("{} {}", x, y);
}

// A different type: w is a `&str`, w₂ a `usize`.
fn type_change() {
    let w = "hello";
    let w = w.len();
    println!("{}", w);
}

// `let m = m;` moves the String into the new variable.
fn move_into_new() {
    let m = String::from("m");
    let m = m;
    println!("{}", m);
}

// `let r = &r;` borrows the shadowed variable, which still owns it.
fn borrow_shadowed() {
    let r = String::from("r");
    let r = &r;
    println!("{}", r);
}

// `let t = t.trim();` borrows from the shadowed String too.
fn trim_shadowed() {
    let t = String::from("  t  ");
    let t = t.trim();
    println!("{}", t);
}

// `let mut u = u;`: a new, mutable variable.
fn mut_rebind() {
    let u = String::from("u");
    let mut u = u;
    u.push_str("!");
    println!("{}", u);
}

// A borrow of the first q is used after the second q is declared.
fn borrow_outlives_shadowing() {
    let q = String::from("q");
    let p = &q;
    let q = 5;
    println!("{} {}", p, q);
}

// A reference variable shadowed by another reference.
fn ref_shadowed() {
    let g = String::from("g");
    let h = String::from("h");
    let k = &g;
    let k = &h;
    println!("{} {}", k, g);
}

// Three variables in a row: c moves into c₂, c₂ into c₃.
fn three() {
    let c = String::from("c");
    let c = c + "+";
    let c = c + "+";
    println!("{}", c);
}

// `let d;` then `d = …`: the assignment initializes the second d.
fn deferred_init() {
    let d = String::from("d1");
    let d;
    d = String::from("d2");
    println!("{}", d);
}

// A shorthand field init `W { e }` moves the second e.
struct W {
    e: String,
}
fn shorthand_field() {
    let e = String::from("e");
    let e = e + "!";
    let w = W { e };
    println!("{}", w.e);
}

fn main() {
    owned_twice();
    copies();
    type_change();
    move_into_new();
    borrow_shadowed();
    trim_shadowed();
    mut_rebind();
    borrow_outlives_shadowing();
    ref_shadowed();
    three();
    deferred_init();
    shorthand_field();
}
