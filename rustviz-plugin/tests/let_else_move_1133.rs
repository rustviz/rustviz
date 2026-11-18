// A `let … else` whose `else` block moves a variable declared before it
// (#1133): the block runs only when the pattern doesn't match, so the
// move happens on that path only, and the matched path still owns the
// value.

fn take(_s: String) {}

fn dropped(opt: Option<i32>) {
    let a = String::from("a");
    let Some(x) = opt else {
        drop(a);
        return;
    };
    println!("{} {}", a, x);
}

fn panics(opt: Option<i32>) -> i32 {
    let b = String::from("b");
    let Some(x) = opt else {
        take(b);
        panic!("none");
    };
    println!("{}", b);
    x
}

fn method(opt: Option<i32>) {
    let d = String::from("d");
    let Some(x) = opt else {
        let _n = d.into_bytes().len();
        return;
    };
    println!("{} {}", d, x);
}

fn in_if(opt: Option<i32>, c: bool) {
    let e = String::from("e");
    if c {
        let Some(x) = opt else {
            drop(e);
            return;
        };
        println!("{} {}", e, x);
    }
}

fn in_loop(mut it: std::vec::IntoIter<i32>) {
    let f = String::from("f");
    loop {
        let Some(x) = it.next() else {
            drop(f);
            return;
        };
        println!("{} {}", f, x);
    }
}

fn main() {
    dropped(Some(1));
    panics(Some(1));
    method(Some(1));
    in_if(Some(1), true);
    in_loop(vec![1].into_iter());
}
