// A struct field holding a reborrow of a `&mut` parameter (`Holder { r: &*m
// }`), then overwritten (`h.r = r;`): the reborrow is given back at the
// write, and m's own borrow ends there too, after it, not at m's last use
// before it (#1538). It used to end m first and give the reborrow back to a
// column that had ended.

struct Holder<'a> {
    r: &'a String,
}

fn run(r: &String, m: &mut String) {
    let mut h = Holder { r: &*m };
    h.r = r;
    println!("{}", h.r);
}

// The field read before the write.
fn read_first(r2: &String, m2: &mut String) {
    let mut h2 = Holder { r: &*m2 };
    println!("{}", h2.r);
    h2.r = r2;
    println!("{}", h2.r);
}

fn holder(r: &String) -> Holder<'_> {
    Holder { r }
}

// The struct from a constructor call.
fn from_call(r3: &String, m3: &mut String) {
    let mut h3 = holder(&*m3);
    h3.r = r3;
    println!("{}", h3.r);
}

fn main() {
    let t = String::from("t");
    let mut u = String::from("u");
    run(&t, &mut u);
    read_first(&t, &mut u);
    from_call(&t, &mut u);
}
