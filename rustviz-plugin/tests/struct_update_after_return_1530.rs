// #1530: a struct that takes another's reference field (`..h`, `let h2 =
// h;`) after a conditional that returns. On the returning path the new
// struct doesn't exist yet: the loan is still the old field's, and that
// field's borrow ends at the `return`. The same for a plain reference
// moved after the conditional (`let r2 = r;`).
struct Holder<'a> {
    r: &'a String,
    n: i32,
}

struct MutHolder<'a> {
    m: &'a mut String,
    n: i32,
}

fn maybe(s: String) -> Option<String> {
    Some(s)
}

fn update_after_return(c: bool) {
    let s = String::from("hi");
    let h = Holder { r: &s, n: 0 };
    if c {
        return;
    }
    let h2 = Holder { n: 2, ..h };
    println!("{}", h2.n);
}

fn update_after_let_else() {
    let s = String::from("hi");
    let k = Holder { r: &s, n: 0 };
    let Some(x) = maybe(String::from("x")) else {
        return;
    };
    let k2 = Holder { n: 2, ..k };
    println!("{} {}", k2.r, x);
}

fn update_straight() {
    let x = String::from("x");
    let r = &x;
    let a = Holder { r: r, n: 1 };
    println!("{} {}", a.r, x);
    let a2 = Holder { n: 2, ..a };
    println!("{}", a2.n);
}

fn moved_whole_after_return(c: bool) {
    let s = String::from("hi");
    let w = Holder { r: &s, n: 0 };
    if c {
        return;
    }
    let w2 = w;
    println!("{}", w2.r);
}

fn mut_field_after_return(c: bool) {
    let mut s = String::from("hi");
    let m = MutHolder { m: &mut s, n: 0 };
    if c {
        return;
    }
    let m2 = MutHolder { n: 2, ..m };
    m2.m.push('!');
    println!("{}", s);
}

fn assigned_after_return(c: bool) {
    let s = String::from("hi");
    let t = String::from("t");
    let g = Holder { r: &s, n: 0 };
    let mut g2 = Holder { r: &t, n: 1 };
    println!("{}", g2.r);
    if c {
        return;
    }
    g2 = Holder { n: 2, ..g };
    println!("{}", g2.r);
}

fn ref_moved_after_return(c: bool) {
    let mut s = String::from("hi");
    let p = &mut s;
    if c {
        return;
    }
    let p2 = p;
    p2.push('!');
    println!("{}", s);
}

fn main() {
    update_after_return(false);
    update_after_let_else();
    update_straight();
    moved_whole_after_return(false);
    mut_field_after_return(false);
    assigned_after_return(false);
    ref_moved_after_return(false);
}
