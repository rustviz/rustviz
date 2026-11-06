// `a == b` on Strings is `PartialEq::eq(&a, &b)` and `a < b` is
// `PartialOrd::lt(&a, &b)`: rustc inserts a `&` on each operand, so both
// are borrowed for the comparison, not moved. A comparison of integers
// (`i < n`) is built in and draws nothing.
#[derive(PartialEq)]
struct P { name: String, age: u32 }

fn main() {
    let a = String::from("x");
    let b = String::from("y");
    let same = a == b;
    let less = a < b;
    let other = a != "z";
    let p = P { name: String::from("p"), age: 1 };
    let q = P { name: String::from("q"), age: 2 };
    let eqp = p == q;
    let names = p.name == q.name;
    let i = 1;
    let n = 2;
    let ints = i < n;
    println!("{} {} {} {} {} {} {} {}", same, less, other, eqp, names, ints, a, b);
}
