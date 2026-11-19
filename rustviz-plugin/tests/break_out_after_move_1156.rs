// #1156: `break k` moves k out of the loop on the iteration that ends it.
// On the other iterations, what the body did to k decides whether the end
// of the body drops it: nothing when the body moved it on the way there,
// the drop flag's choice when it moved it on some paths only.
fn take(_s: String) {}

fn moved(n0: i32) {
    let mut n = n0;
    let found = loop {
        n += 1;
        let k = String::from("a");
        if n > 3 {
            break k;
        }
        take(k);
    };
    println!("{}", found);
}

fn maybe(c: bool) {
    let mut n = 0;
    let found = loop {
        n += 1;
        let m = String::from("a");
        if n > 3 {
            break m;
        }
        if c {
            take(m);
        }
    };
    println!("{}", found);
}

// Kept: dropped at the end of the body on those iterations.
fn kept() {
    let mut n = 0;
    let found = loop {
        n += 1;
        let p = String::from("a");
        if n > 3 {
            break p;
        }
    };
    println!("{}", found);
}

// The loop's value as an argument, and as the function's value (#1171).
fn arg() {
    let mut n = 0;
    take(loop {
        n += 1;
        let q = String::from("a");
        if n > 3 {
            break q;
        }
        drop(q);
    });
}

fn ret() -> String {
    let mut n = 0;
    loop {
        n += 1;
        let r = String::from("a");
        if n > 3 {
            break r;
        }
        drop(r);
    }
}

fn main() {
    moved(0);
    maybe(true);
    kept();
    arg();
    println!("{}", ret());
}
