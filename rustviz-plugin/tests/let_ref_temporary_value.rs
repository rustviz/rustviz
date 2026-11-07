// #483: `let ref t = <value>;` borrows a temporary that rustc keeps
// alive to the end of the block and drops there (checked with a
// Drop-printing program). It is a `(temporary N)` column, like the one
// `let r = &<value>;` has (#400), taking the value from the call that
// made it.
struct P {
    name: String,
    age: i32,
}

fn make() -> String {
    String::from("m")
}

fn make_p() -> P {
    P { name: make(), age: 1 }
}

fn main() {
    let ref t = String::from("tmp");
    let ref mut m = make();
    let P { ref name, .. } = make_p();
    println!("{} {}", t, name);
    m.push('x');
}
