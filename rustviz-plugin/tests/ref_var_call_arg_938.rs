// #938: `show(&x)` with x a reference variable and a `&&String`
// parameter borrows x itself: the parameter points to x, not to what
// x points to (that would be `show(&*x)` with a `&String` parameter).
// The call-argument form of #925 (`let rr = &x;`), and of #914 for
// `&mut m`. (Inside show / show_m / bump / show_self, `r.len()` borrows
// the String two references away: "the resource *r points to is
// immutably borrowed by len", #952.)
fn show(r: &&String) -> usize {
    r.len()
}

fn show_m(r: &&mut String) -> usize {
    r.len()
}

fn bump(r: &mut &mut String) {
    r.push('!');
}

fn show_str(s: &String) -> usize {
    s.len()
}

trait Show {
    fn show_self(&self) -> usize;
}

// `show_self(&self)` on a `&String` takes a `&&String`.
impl Show for &String {
    fn show_self(&self) -> usize {
        self.len()
    }
}

fn call_args() {
    let a = String::from("a");
    let x = &a;
    let n = show(&x);
    let mut g = String::from("g");
    let mut m = &mut g;
    let k = show_m(&m);
    bump(&mut m);
    m.push('?');
    println!("{} {}", n, k);
}

// `show_str(&*y)` and a plain `show_str(y)` borrow what y points to.
fn reborrows() {
    let b = String::from("b");
    let y = &b;
    let n = show_str(&*y);
    let k = show_str(y);
    println!("{} {}", n, k);
}

// A method receiver: `w.show_self()` autorefs w itself.
fn receivers() {
    let c = String::from("c");
    let w = &c;
    let n = w.show_self();
    let k = (&w).show_self();
    println!("{} {}", n, k);
}

fn main() {
    call_args();
    reborrows();
    receivers();
}
