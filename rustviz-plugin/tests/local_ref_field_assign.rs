// `let p = &mut foo; p.x = …` — per-field RAP registration
// for ref-to-struct locals (mirror of the param-side fix in
// #147). Without this, the assignment was silently dropped
// because `p.x` wasn't in raps. (#152)

struct Foo {
    x: i32,
}

fn main() {
    let mut foo = Foo { x: 0 };
    let p = &mut foo;
    p.x = 5;
}
