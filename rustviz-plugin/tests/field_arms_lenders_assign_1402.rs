// #1402's siblings, continued: the struct reassigned from a conditional,
// a field assigned one, `let d;` assigned in each arm, an arm whose value
// uses a variable it declares, an arm whose value is a call's result.
// Every lender the reference field may point into stays borrowed until
// its last use and gets its borrow back there.
struct One<'p> { a: &'p str }
struct Outer<'p> { inner: One<'p>, n: i32 }

fn mk<'p>(s: &'p str) -> One<'p> { One { a: s } }

fn reassign() {
    let r0 = String::from("z");
    let r1 = String::from("hi");
    let r2 = String::from("yo");
    let c = std::env::args().count() > 1;
    let mut d = One { a: &r0 };
    println!("{}", d.a);
    d = if c { One { a: &r1 } } else { One { a: &r2 } };
    println!("{}", d.a);
}

fn field_assign() {
    let f0 = String::from("z");
    let f1 = String::from("hi");
    let f2 = String::from("yo");
    let c = std::env::args().count() > 1;
    let mut o = Outer { inner: One { a: &f0 }, n: 1 };
    o.inner = if c { One { a: &f1 } } else { One { a: &f2 } };
    println!("{} {}", o.inner.a, o.n);
}

fn late_init() {
    let g1 = String::from("hi");
    let g2 = String::from("yo");
    let c = std::env::args().count() > 1;
    let e: One;
    if c {
        e = One { a: &g1 };
    } else {
        e = One { a: &g2 };
    }
    println!("{}", e.a);
}

fn arm_local() {
    let h1 = String::from("hi");
    let h2 = String::from("yo");
    let c = std::env::args().count() > 1;
    let f = if c {
        let t = &h1;
        One { a: t }
    } else {
        One { a: &h2 }
    };
    println!("{}", f.a);
}

fn call_arm() {
    let k1 = String::from("hi");
    let k2 = String::from("yo");
    let c = std::env::args().count() > 1;
    let g = if c { mk(&k1) } else { One { a: &k2 } };
    println!("{}", g.a);
}

fn main() {
    reassign();
    field_assign();
    late_init();
    arm_local();
    call_arm();
}
