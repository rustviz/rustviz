// #1387: `(a, _) = (make(), make());` is lowered to `{ let (lhs, _) =
// (make(), make()); a = lhs; }`: what `_` leaves stays in the literal's
// temporary, which is dropped once the assignment has taken it apart,
// before a gets its new value. As for `let (a, _) = (make(), make());`
// (#483), the temporary gets a column; a variable the pattern leaves
// moves into it.
fn make() -> String {
    String::from("m")
}

struct P {
    x: String,
    y: String,
}

fn d1() {
    let a: String;
    (a, _) = (make(), make());
    println!("{}", a);
}

fn d2() {
    let s = String::from("s");
    let a: String;
    (a, _, _) = (make(), make(), s);
    println!("{}", a);
}

fn d3() {
    let a: String;
    P { x: a, .. } = P { x: make(), y: make() };
    println!("{}", a);
}

fn d4() {
    let mut a = make();
    println!("{}", a);
    (a, _) = (make(), make());
    println!("{}", a);
}

fn d5(c: bool) {
    let a: String;
    (a, _) = if c {
        (make(), make())
    } else {
        (make(), make())
    };
    println!("{}", a);
}

fn main() {
    d1();
    d2();
    d3();
    d4();
    d5(true);
}
