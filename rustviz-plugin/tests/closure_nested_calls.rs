// Calling a closure whose body calls another closure runs that
// closure's body too (#278): the variables it borrowed get a dot at
// the outer call.
fn show(_s: &String) {}

fn main() {
    let s = String::from("hi");
    let f = || show(&s);
    let g = || f();
    g();
    let mut t = String::from("a");
    let mut push = || t.push('b');
    let mut twice = |c: bool| if c { push(); };
    twice(true);
    let n = String::from("n");
    let a = || n.len();
    let b = || a() + 1;
    let c = || b() * 2;
    let m = c();
    println!("{} {}", t, m);
}
