// #952: a method called on a reference to a reference derefs twice:
// `r.len()` with `r: &&String` is `str::len(&***r)` and borrows the
// String `**r`, which `*r` points to, not the reference `*r` that r
// points to.
trait Show {
    fn show_self(&self) -> usize;
}

impl Show for &String {
    fn show_self(&self) -> usize {
        self.len()
    }
}

fn show(r: &&String) -> usize {
    r.len()
}

fn bump(r: &mut &mut String) {
    r.push('!');
}

fn star(q: &&String) -> usize {
    (*q).len()
}

// What's two references away is a value here.
fn count(c: &&[i32]) -> usize {
    c.len()
}

// For comparison: one reference, unchanged.
fn plain(p: &String) -> usize {
    p.len()
}

// A call argument deref-coerced the same way: `plain(&**d)`.
fn pass(d: &&String) -> usize {
    plain(d)
}

fn add(s: &mut String) {
    s.push('+');
}

fn pass_mut(e: &mut &mut String) {
    add(e);
}

fn main() {
    let a = String::from("a");
    let x = &a;
    let n = show(&x);
    let rr = &x;
    let k = rr.len();
    let mut g = String::from("g");
    let mut m = &mut g;
    bump(&mut m);
    pass_mut(&mut m);
    let mm = &mut m;
    mm.push('#');
    let nums = [1, 2, 3];
    let sl: &[i32] = &nums;
    println!("{} {} {} {} {} {} {}", n, k, star(&x), x.show_self(), count(&sl), plain(&a), pass(&x));
}
