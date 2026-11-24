// #1396: `(a, n) = loop { break (make(), 1); };` with `let a; let n;`
// assigns a and n on the loop's only way out, its `break`: they have
// values after the loop (rustc accepts the uses below). They were drawn
// "may have no value after the loop", and `let mut a;` got "On a later
// iteration of the loop ... dropped here first", though the `break`
// leaves the loop right after assigning.
fn make() -> String {
    String::from("m")
}

fn f1() {
    let a: String;
    let n: i32;
    (a, n) = loop {
        break (make(), 1);
    };
    println!("{} {}", a, n);
}

// A `break` out of a loop nested inside it.
fn f2() {
    let b: String;
    let k: i32;
    (b, k) = 'outer: loop {
        loop {
            break 'outer (make(), 1);
        }
    };
    println!("{} {}", b, k);
}

// A struct pattern, an array pattern, `_`.
struct P {
    name: String,
    n: i32,
}

fn f3() {
    let c: String;
    let m: i32;
    P { name: c, n: m } = loop {
        break P { name: String::from("p"), n: 1 };
    };
    println!("{} {}", c, m);
}

fn f4() {
    let d: String;
    let e: String;
    [d, e] = loop {
        break [String::from("d"), String::from("e")];
    };
    println!("{} {}", d, e);
}

fn f5() {
    let g: String;
    (g, _) = loop {
        break (make(), make());
    };
    println!("{}", g);
}

// A variable moved by the `break`, and a `while` inside the loop.
fn f6() {
    let s = String::from("s");
    let h: String;
    let mut i = 0;
    (h, i) = loop {
        while i < 3 {
            i += 1;
        }
        break (s, i);
    };
    println!("{} {}", h, i);
}

// `let mut`: assigned once, then reassigned (dropping the first value).
fn f7() {
    let mut x: String;
    (x, _) = loop {
        break (make(), 1);
    };
    println!("{}", x);
    x = make();
    println!("{}", x);
}

// Inside another loop, which a plain `break` leaves.
fn f8() {
    let y: String;
    let z: i32;
    loop {
        (y, z) = loop {
            break (make(), 1);
        };
        break;
    }
    println!("{} {}", y, z);
}

fn main() {
    f1();
    f2();
    f3();
    f4();
    f5();
    f6();
    f7();
    f8();
}
