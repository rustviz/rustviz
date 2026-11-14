// Borrows of values that need no drop (#867): a unit block, a literal,
// and a struct literal whose only field is Copy. The referent is a
// plain value, never a "resource"; `&String::from("")` stays one.
struct Appy {
    a: u32,
}

fn main() {
    let r = &{};
    let n = &5;
    let k = &mut 7;
    let z = &Appy { a: 4 };
    let m = &mut Appy { a: 1 };
    let y = &String::from("");
}
