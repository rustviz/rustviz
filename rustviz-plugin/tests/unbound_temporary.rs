// #457: a call's result that nothing binds or moves on is a temporary,
// dropped when its statement (or other temporary scope) ends. It gets a
// short `(unbound temporary)` column: acquired at the call, dropped
// where Rust drops it. Timing confirmed with a Drop-printing program:
// `let _ = f();` drops at once, a match scrutinee at the end of the
// match, an `if` condition before the body, a block's tail expression
// before the block's locals.
fn make() -> String {
    String::from("m")
}

fn show(s: &String) {
    println!("{}", s);
}

fn tail() -> usize {
    let _local = String::from("l");
    make().len()
}

fn main() {
    let _ = String::from("x");
    make();
    let n = make().len();
    show(&String::from("a"));
    match Some(String::from("c")) {
        Some(_) => println!("arm"),
        None => {}
    }
    if make().len() > 5 {
        println!("long");
    }
    let flag = n > 0;
    if flag {
        make();
    } else {
        make();
    }
    // Moved on, so nothing is dropped as a temporary:
    let kept = make();
    let bound = match Some(make()) {
        Some(s) => s,
        None => String::new(),
    };
    // Numbers have nothing to drop:
    let k = String::from("abc").len();
    // A one-line match can't hold a column (#459): no temporary drawn.
    match Some(make()) { Some(_) => {} None => {} }
    println!("{} {} {} {} {}", tail(), kept, bound, k, make());
}
