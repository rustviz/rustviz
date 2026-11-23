// #1311: a struct borrowed as a whole by a call (`c.reset()`, `inc(&mut
// c)`, `show(&c)`, `let k = c.get();`) is lent and given back on one row.
// Its fields are borrowed with it for the call, and a mutable borrow may
// change them (`reset` writes `self.n`), so each field's column gets a dot
// on that row saying so; they showed nothing there.

struct Counter {
    n: u32,
    name: String,
}

impl Counter {
    fn reset(&mut self) {
        self.n = 0;
    }
    fn get(&self) -> u32 {
        self.n
    }
}

fn inc(c: &mut Counter) {
    c.n += 1;
}

fn show(c: &Counter) {
    println!("{} {}", c.n, c.name);
}

fn method_mut() {
    let mut c = Counter { n: 5, name: String::from("c") };
    c.reset();
    println!("{} {}", c.n, c.name);
}

fn free_fn_mut() {
    let mut c = Counter { n: 5, name: String::from("c") };
    inc(&mut c);
    println!("{} {}", c.n, c.name);
}

fn free_fn_shared() {
    let c = Counter { n: 5, name: String::from("c") };
    show(&c);
    println!("{} {}", c.n, c.name);
}

fn method_shared_in_let() {
    let c = Counter { n: 5, name: String::from("c") };
    let k = c.get();
    println!("{} {} {}", c.n, c.name, k);
}

fn in_branch(b: bool) {
    let mut c = Counter { n: 5, name: String::from("c") };
    if b {
        c.reset();
    }
    println!("{} {}", c.n, c.name);
}

fn main() {
    method_mut();
    free_fn_mut();
    free_fn_shared();
    method_shared_in_let();
    in_branch(true);
}
